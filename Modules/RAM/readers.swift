//
//  readers.swift
//  Memory
//
//  Created by Serhiy Mytrovtsiy on 12/04/2020.
//  Using Swift 5.0.
//  Running on macOS 10.15.
//
//  Copyright © 2020 Serhiy Mytrovtsiy. All rights reserved.
//

import Cocoa
import Kit

internal class UsageReader: Reader<RAM_Usage> {
    public var totalSize: Double = 0
    
    public override func setup() {
        var raw = StatsRamUsage()
        if stats_core_poll_ram(&raw) {
            self.totalSize = Double(raw.total)
        }
    }
    
    public override func read() {
        var raw = StatsRamUsage()
        guard stats_core_poll_ram(&raw) else {
            return
        }
        
        self.totalSize = Double(raw.total)
        
        var pressureValue: RAMPressure
        switch raw.pressure_level {
        case 2: pressureValue = .warning
        case 4: pressureValue = .critical
        default: pressureValue = .normal
        }
        
        self.callback(RAM_Usage(
            total: Double(raw.total),
            used: Double(raw.used),
            free: Double(raw.free),
            
            active: Double(raw.active),
            inactive: Double(raw.inactive),
            wired: Double(raw.wired),
            compressed: Double(raw.compressed),
            
            app: Double(raw.app),
            cache: Double(raw.cache),
            
            swap: Swap(
                total: Double(raw.swap_total),
                used: Double(raw.swap_used),
                free: Double(raw.swap_free)
            ),
            pressure: Pressure(level: Int(raw.pressure_level), value: pressureValue),
            
            swapins: raw.swapins,
            swapouts: raw.swapouts
        ))
    }
}

public class ProcessReader: Reader<[TopProcess]> {
    private let title: String = "RAM"
    
    private var numberOfProcesses: Int {
        get { Store.shared.int(key: "\(self.title)_processes", defaultValue: 8) }
    }
    
    private var combinedProcesses: Bool{
        get { Store.shared.bool(key: "\(self.title)_combinedProcesses", defaultValue: false) }
    }
    
    private typealias dynGetResponsiblePidFuncType = @convention(c) (CInt) -> CInt
    
    public override func setup() {
        self.popup = true
        self.preview = true
        self.setInterval(Store.shared.int(key: "\(self.title)_updateTopInterval", defaultValue: 1))
    }
    
    public override func read() {
        if self.numberOfProcesses == 0 {
            return
        }
        
        var procs = StatsTopProcesses()
        let queryLimit: UInt32 = self.combinedProcesses ? 48 : UInt32(min(self.numberOfProcesses, 48))
        guard stats_core_top_ram_processes(queryLimit, &procs) else {
            return
        }
        
        var processes: [TopProcess] = []
        let count = Int(procs.count)
        withUnsafePointer(to: procs.processes) { ptr in
            ptr.withMemoryRebound(to: StatsProcessInfo.self, capacity: count) { buf in
                for i in 0..<count {
                    let info = buf[i]
                    let pid = Int(info.pid)
                    var name = withUnsafeBytes(of: info.name) { rawPtr in
                        String(cString: rawPtr.baseAddress!.assumingMemoryBound(to: CChar.self))
                    }
                    if let app = NSRunningApplication(processIdentifier: pid_t(pid)), let n = app.localizedName {
                        name = n
                    }
                    processes.append(TopProcess(pid: pid, name: name, usage: Double(info.ram_bytes)))
                }
            }
        }
        
        if !self.combinedProcesses {
            self.callback(Array(processes.prefix(self.numberOfProcesses)))
            return
        }
        
        var processGroups: [String: [TopProcess]] = [:]
        for process in processes {
            let responsiblePid = ProcessReader.getResponsiblePid(process.pid)
            let groupKey = "\(responsiblePid)"
            
            if processGroups[groupKey] != nil {
                processGroups[groupKey]!.append(process)
            } else {
                processGroups[groupKey] = [process]
            }
        }
        
        var result: [TopProcess] = []
        for (_, processes) in processGroups {
            let totalUsage = processes.reduce(0) { $0 + $1.usage }
            let firstProcess = processes.first!
            let name: String
            
            if let app = NSRunningApplication(processIdentifier: pid_t(ProcessReader.getResponsiblePid(firstProcess.pid))),
               let appName = app.localizedName {
                name = appName
            } else {
                name = firstProcess.name
            }
            
            result.append(TopProcess(
                pid: ProcessReader.getResponsiblePid(firstProcess.pid),
                name: name,
                usage: totalUsage
            ))
        }
        
        result.sort { $0.usage > $1.usage }
        self.callback(Array(result.prefix(self.numberOfProcesses)))
    }
    
    private static let dynGetResponsiblePidFunc: UnsafeMutableRawPointer? = {
        let result = dlsym(UnsafeMutableRawPointer(bitPattern: -1), "responsibility_get_pid_responsible_for_pid")
        if result == nil {
            error("Error loading responsibility_get_pid_responsible_for_pid")
        }
        return result
    }()
    
    static func getResponsiblePid(_ childPid: Int) -> Int {
        guard ProcessReader.dynGetResponsiblePidFunc != nil else {
            return childPid
        }
        let responsiblePid = unsafeBitCast(ProcessReader.dynGetResponsiblePidFunc, to: dynGetResponsiblePidFuncType.self)(CInt(childPid))
        guard responsiblePid != -1 else {
            return childPid
        }
        return Int(responsiblePid)
    }
    
    static public func parseProcess(_ raw: String) -> TopProcess {
        var str = raw.trimmingCharacters(in: .whitespaces)
        let pidString = str.find(pattern: "^\\d+")
        
        if let range = str.range(of: pidString) {
            str = str.replacingCharacters(in: range, with: "")
        }
        
        var arr = str.split(separator: " ")
        if arr.first == "*" {
            arr.removeFirst()
        }
        
        var usageString = str.suffix(6)
        if let lastElement = arr.last {
            usageString = lastElement
            arr.removeLast()
        }
        
        var command = arr.joined(separator: " ")
            .replacingOccurrences(of: pidString, with: "")
            .trimmingCharacters(in: .whitespaces)
        
        if let regex = try? NSRegularExpression(pattern: " (\\+|\\-)*$", options: .caseInsensitive) {
            command = regex.stringByReplacingMatches(in: command, options: [], range: NSRange(location: 0, length: command.count), withTemplate: "")
        }
        
        let pid = Int(pidString.filter("01234567890.".contains)) ?? 0
        var usage = Double(usageString.filter("01234567890.".contains)) ?? 0
        if usageString.last == "G" {
            usage *= 1024 // apply gigabyte multiplier
        } else if usageString.last == "K" {
            usage /= 1024 // apply kilobyte divider
        } else if usageString.last == "M" && usageString.count == 5 {
            usage /= 1024
            usage *= 1000
        }
        
        var name: String = command
        if let app = NSRunningApplication(processIdentifier: pid_t(pid)), let n = app.localizedName {
            name = n
        }
        
        if command.contains("com.apple.Virtua") && name.contains("Docker") {
            name = "Docker"
        }
        
        return TopProcess(pid: pid, name: name, usage: usage * Double(1000 * 1000))
    }
}
