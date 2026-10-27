//
//  readers.swift
//  CPU
//
//  Created by Serhiy Mytrovtsiy on 10/04/2020.
//  Using Swift 5.0.
//  Running on macOS 10.15.
//
//  Copyright © 2020 Serhiy Mytrovtsiy. All rights reserved.
//

import Cocoa
import Kit

internal class LoadReader: Reader<CPU_Load> {
    private let CPUUsageLock: NSLock = NSLock()
    private var hasHyperthreadingCores = false
    
    private var response: CPU_Load = CPU_Load()
    private var usagePerCore: [Double] = []
    private var cores: [core_s]? = nil
    
    public override func setup() {
        self.hasHyperthreadingCores = sysctlByName("hw.physicalcpu") != sysctlByName("hw.logicalcpu")
        self.cores = SystemKit.shared.device.info.cpu?.cores
    }
    
    public override func read() {
        self.CPUUsageLock.lock()
        defer { self.CPUUsageLock.unlock() }
        
        var raw = StatsCpuLoad()
        guard stats_core_poll_cpu(&raw) else {
            self.callback(nil)
            return
        }
        
        self.response.totalUsage = raw.total_usage
        self.response.systemLoad = raw.system_load
        self.response.userLoad = raw.user_load
        self.response.idleLoad = raw.idle_load
        
        let nCores = Int(raw.core_count)
        self.usagePerCore = []
        withUnsafePointer(to: raw.usage_per_core) { ptr in
            ptr.withMemoryRebound(to: Double.self, capacity: nCores) { buf in
                for i in 0..<nCores {
                    self.usagePerCore.append(buf[i])
                }
            }
        }
        
        let showHyperthratedCores = Store.shared.bool(key: "CPU_hyperhreading", defaultValue: false)
        if showHyperthratedCores || !self.hasHyperthreadingCores {
            self.response.usagePerCore = self.usagePerCore
        } else {
            var i = 0
            var a = 0
            
            self.response.usagePerCore = []
            while i < Int(self.usagePerCore.count/2) {
                a = i*2
                if self.usagePerCore.indices.contains(a) && self.usagePerCore.indices.contains(a+1) {
                    self.response.usagePerCore.append((Double(self.usagePerCore[a]) + Double(self.usagePerCore[a+1])) / 2)
                }
                i += 1
            }
        }
        
        if let cores = self.cores {
            let eCoresList: [Double] = cores.filter({ $0.type == .efficiency }).enumerated().compactMap { (i, c) -> Double? in
                if self.response.usagePerCore.indices.contains(Int(c.id)) {
                    return self.response.usagePerCore[Int(c.id)]
                }
                return i < self.usagePerCore.count ? self.usagePerCore[i] : 0
            }
            let pCoresList: [Double] = cores.filter({ $0.type == .performance }).enumerated().compactMap { (i, c) -> Double? in
                if self.response.usagePerCore.indices.contains(Int(c.id)) {
                    return self.response.usagePerCore[Int(c.id)]
                }
                return i < self.usagePerCore.count ? self.usagePerCore[i] : 0
            }
            let sCoresList: [Double] = cores.filter({ $0.type == .super }).enumerated().compactMap { (i, c) -> Double? in
                if self.response.usagePerCore.indices.contains(Int(c.id)) {
                    return self.response.usagePerCore[Int(c.id)]
                }
                return i < self.usagePerCore.count ? self.usagePerCore[i] : 0
            }
            
            if !eCoresList.isEmpty {
                self.response.usageECores = eCoresList.reduce(0, +)/Double(eCoresList.count)
            }
            if !pCoresList.isEmpty {
                self.response.usagePCores = pCoresList.reduce(0, +)/Double(pCoresList.count)
            }
            if !sCoresList.isEmpty {
                self.response.usageSCores = sCoresList.reduce(0, +)/Double(sCoresList.count)
            }
        }
        
        let response = self.response
        self.callback(response)
    }
}

public class ProcessReader: Reader<[TopProcess]> {
    private let title: String = "CPU"
    
    private var numberOfProcesses: Int {
        get { Store.shared.int(key: "\(self.title)_processes", defaultValue: 8) }
    }
    
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
        guard stats_core_top_cpu_processes(UInt32(self.numberOfProcesses), &procs) else {
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
                    if name.contains("com.apple.Virtua") && name.contains("Docker") {
                        name = "Docker"
                    }
                    processes.append(TopProcess(pid: pid, name: name, usage: info.cpu_usage))
                }
            }
        }
        
        self.callback(processes)
    }
}

public class TemperatureReader: Reader<Double> {
    var list: [String] = []
    
    public override func setup() {
        self.popup = true
        switch SystemKit.shared.device.platform {
        case .m1, .m1Pro, .m1Max, .m1Ultra:
            self.list = ["Tp09", "Tp0T", "Tp01", "Tp05", "Tp0D", "Tp0H", "Tp0L", "Tp0P", "Tp0X", "Tp0b"]
        case .m2, .m2Pro, .m2Max, .m2Ultra:
        self.list = ["Tp1h", "Tp1t", "Tp1p", "Tp1l", "Tp01", "Tp05", "Tp09", "Tp0D", "Tp0X", "Tp0b", "Tp0f", "Tp0j"]
        case .m3, .m3Pro, .m3Max, .m3Ultra:
            self.list = ["Te05", "Te0L", "Te0P", "Te0S", "Tf04", "Tf09", "Tf0A", "Tf0B", "Tf0D", "Tf0E", "Tf44", "Tf49", "Tf4A", "Tf4B", "Tf4D", "Tf4E"]
        case .m4, .m4Pro, .m4Max:
            self.list = ["Te05", "Te09", "Te0H", "Te0S", "Tp01", "Tp05", "Tp09", "Tp0D", "Tp0V", "Tp0Y", "Tp0b", "Tp0e"]
        case .m5, .m5Pro, .m5Max, .m5Ultra:
            self.list = ["Tp00", "Tp04", "Tp08", "Tp0C", "Tp0G", "Tp0K", "Tp0O", "Tp0R", "Tp0U", "Tp0X", "Tp0a", "Tp0d", "Tp0g", "Tp0j", "Tp0m", "Tp0p", "Tp0u", "Tp0y"]
        default: break
        }
    }
    
    public override func read() {
        var temperature: Double? = nil
        
        if let value = SMC.shared.getValue("TC0D"), value < 110 {
            temperature = value
        } else if let value = SMC.shared.getValue("TC0E"), value < 110 {
            temperature = value
        } else if let value = SMC.shared.getValue("TC0F"), value < 110 {
            temperature = value
        } else if let value = SMC.shared.getValue("TC0P"), value < 110 {
            temperature = value
        } else if let value = SMC.shared.getValue("TC0H"), value < 110 {
            temperature = value
        } else {
            var total: Double = 0
            var counter: Double = 0
            self.list.forEach { (key: String) in
                if let value = SMC.shared.getValue(key), value >= 10 && value <= 120 {
                    total += value
                    counter += 1
                }
            }
            if counter != 0 {
                temperature = total / counter
            }
        }
        
        self.callback(temperature)
    }
}

// inspired by https://github.com/shank03/StatsBar/blob/e175aa71c914ce882ce2e90163f3eb18262a8e25/StatsBar/Service/IOReport.swift
public class FrequencyReader: Reader<CPU_Frequency> {
    private var eCoreFreqs: [Int32] = []
    private var pCoreFreqs: [Int32] = []
    private var sCoreFreqs: [Int32] = []
    private var eCoreCount: Double = 0
    private var pCoreCount: Double = 0
    private var sCoreCount: Double = 0
    
    private var channels: CFMutableDictionary? = nil
    private var subscription: IOReportSubscriptionRef? = nil
    private var prev: (samples: CFDictionary, time: TimeInterval)? = nil
    
    private let measurementCount: Int = 4
    private let isReadingQueue = DispatchQueue(label: "com.example.isReadingQueue")
    
    private var _isReading: Bool = false
    private var isReading: Bool {
        get { self.isReadingQueue.sync { self._isReading } }
        set { self.isReadingQueue.sync { self._isReading = newValue } }
    }
    
    private struct IOSample {
        let group: String
        let subGroup: String
        let channel: String
        let unit: String
        let delta: CFDictionary
    }
    
    public override func setup() {
        self.popup = true
        self.preview = true
        self.eCoreFreqs = SystemKit.shared.device.info.cpu?.eCoreFrequencies ?? []
        self.pCoreFreqs = SystemKit.shared.device.info.cpu?.pCoreFrequencies ?? []
        self.sCoreFreqs = SystemKit.shared.device.info.cpu?.sCoreFrequencies ?? []
        self.eCoreCount = Double(SystemKit.shared.device.info.cpu?.eCores ?? 0)
        self.pCoreCount = Double(SystemKit.shared.device.info.cpu?.pCores ?? 0)
        self.sCoreCount = Double(SystemKit.shared.device.info.cpu?.sCores ?? 0)
        self.channels = self.getChannels()
        var dict: Unmanaged<CFMutableDictionary>?
        self.subscription = IOReportCreateSubscription(nil, self.channels, &dict, 0, nil)
        dict?.release()
    }
    
    public override func read() {
        guard !self.isReading, !self.eCoreFreqs.isEmpty || !self.pCoreFreqs.isEmpty || !self.sCoreFreqs.isEmpty, self.channels != nil, self.subscription != nil else { return }
        self.isReading = true
        let minECoreFreq = Double(self.eCoreFreqs.min() ?? 0)
        let minPCoreFreq = Double(self.pCoreFreqs.min() ?? 0)
        let minSCoreFreq = Double(self.sCoreFreqs.min() ?? 0)
        
        Task {
            var eCores: [Double] = []
            var sCores: [Double] = []
            var pCores: [Double] = []
            
            for (samples, _) in await self.getSamples() {
                var eCore: [Double] = []
                var pCore: [Double] = []
                var sCore: [Double] = []
                
                for sample in samples {
                    guard sample.group == "CPU Stats" else { continue }
                    if sample.channel.contains("ECPU"), let frequency = self.calculateFrequencies(dict: sample.delta, freqs: self.eCoreFreqs) {
                        eCore.append(frequency)
                    }
                    if sample.channel.contains(self.sCoreCount == 0 ? "PCPU" : "MCPU"), let frequency = self.calculateFrequencies(dict: sample.delta, freqs: self.pCoreFreqs) {
                        pCore.append(frequency)
                    }
                    if self.sCoreCount != 0 {
                        if sample.channel.contains("PCPU"), let frequency = self.calculateFrequencies(dict: sample.delta, freqs: self.sCoreFreqs) {
                            sCore.append(frequency)
                        }
                    }
                }
                
                if !eCore.isEmpty {
                    eCores.append(max(eCore.reduce(0.0, +) / Double(eCore.count), minECoreFreq))
                }
                if !pCore.isEmpty {
                    pCores.append(max(pCore.reduce(0.0, +) / Double(pCore.count), minPCoreFreq))
                }
                if !sCore.isEmpty {
                    sCores.append(max(sCore.reduce(0.0, +) / Double(sCore.count), minSCoreFreq))
                }
            }
            
            let eFreq: Double? = eCores.isEmpty ? nil : eCores.reduce(0, +) / Double(eCores.count)
            let pFreq: Double? = pCores.isEmpty ? nil : pCores.reduce(0, +) / Double(pCores.count)
            let sFreq: Double? = sCores.isEmpty ? nil : sCores.reduce(0, +) / Double(sCores.count)
            
            var activeCores: Double = 0
            var totalFreq: Double = 0
            
            if let freq = eFreq {
                activeCores += self.eCoreCount
                totalFreq += freq * self.eCoreCount
            }
            if let freq = pFreq {
                activeCores += self.pCoreCount
                totalFreq += freq * self.pCoreCount
            }
            if let freq = sFreq {
                activeCores += self.sCoreCount
                totalFreq += freq * self.sCoreCount
            }
            let value: Double? = activeCores > 0 ? totalFreq / activeCores : nil
            
            self.callback(CPU_Frequency(value: value, eCore: eFreq, pCore: pFreq, sCore: sFreq))
            self.isReading = false
        }
    }
    
    private func calculateFrequencies(dict: CFDictionary, freqs: [Int32]) -> Double? {
        guard !freqs.isEmpty else { return nil }
        let items = self.getResidencies(dict: dict)
        guard let offset = items.firstIndex(where: { $0.0 != "IDLE" && $0.0 != "DOWN" && $0.0 != "OFF" }), items.count - offset == freqs.count else { return nil }
        let usage = items.dropFirst(offset).reduce(0.0) { $0 + Double($1.f) }
        let count = freqs.count
        var avgFreq: Double = 0
        
        for i in 0..<count {
            let key = i + offset
            if !items.indices.contains(key) { continue }
            let percent = usage == 0 ? 0 : Double(items[key].f) / usage
            avgFreq += percent * Double(freqs[i])
        }
        
        return avgFreq
    }
    
    func getResidencies(dict: CFDictionary) -> [(ns: String, f: Int64)] {
        let count = IOReportStateGetCount(dict)
        var res: [(String, Int64)] = []
        
        for i in 0..<count {
            let name = IOReportStateGetNameForIndex(dict, i)?.takeUnretainedValue() ?? ("" as CFString)
            let val = IOReportStateGetResidency(dict, i)
            res.append((name as String, val))
        }
        
        return res
    }
    
    private func getChannels() -> CFMutableDictionary? {
        let channelNames = [
            ("CPU Stats", "CPU Complex Performance States"),
            ("CPU Stats", "CPU Core Performance States")
        ]
        
        var channels: [CFDictionary] = []
        for (gname, sname) in channelNames {
            let channel = IOReportCopyChannelsInGroup(gname as CFString?, sname as CFString?, 0, 0, 0)
            guard let channel = channel?.takeRetainedValue() else { continue }
            channels.append(channel)
        }
        
        guard let chan = channels.first else { return nil }
        for i in 1..<channels.count {
            IOReportMergeChannels(chan, channels[i], nil)
        }
        
        let size = CFDictionaryGetCount(chan)
        guard let channel = CFDictionaryCreateMutableCopy(kCFAllocatorDefault, size, chan),
              let chan = channel as? [String: Any], chan["IOReportChannels"] != nil else {
            return nil
        }
        
        return channel
    }
    
    private func getSamples() async -> [([IOSample], TimeInterval)] {
        let duration = 500
        let step = UInt64(duration / self.measurementCount)
        var samples = [([IOSample], TimeInterval)]()
        guard let initialSample = self.getSample() else { return samples }
        var prev = self.prev ?? initialSample
        
        for _ in 0..<self.measurementCount {
            let milliseconds = UInt64(step) * 1_000_000
            do {
                try await Task.sleep(nanoseconds: milliseconds)
            } catch {
                if Task.isCancelled { return [] }
                continue
            }
            
            guard let next = self.getSample() else { continue }
            
            if let diffCF = IOReportCreateSamplesDelta(prev.samples, next.samples, nil) {
                let diff = diffCF.takeRetainedValue()
                let elapsed = next.time - prev.time
                samples.append((self.collectIOSamples(data: diff), max(elapsed, TimeInterval(1))))
            }
            prev = next
        }
        
        self.prev = prev
        return samples
    }
    
    private func getSample() -> (samples: CFDictionary, time: TimeInterval)? {
        guard let sample = IOReportCreateSamples(self.subscription, self.channels, nil)?.takeRetainedValue() else {
            return nil
        }
        return (sample, Date().timeIntervalSince1970)
    }
    
    private func collectIOSamples(data: CFDictionary) -> [IOSample] {
        guard let dict = data as? [String: Any],
              let channelsList = dict["IOReportChannels"] as? NSArray else {
            return []
        }
        let items = channelsList as CFArray
        let itemSize = CFArrayGetCount(items)
        
        var samples = [IOSample]()
        
        for index in 0..<itemSize {
            let dict = CFArrayGetValueAtIndex(items, index)
            let item = unsafeBitCast(dict, to: CFDictionary.self)
            
            let group = IOReportChannelGetGroup(item)?.takeUnretainedValue() ?? ("" as CFString)
            let subGroup = IOReportChannelGetSubGroup(item)?.takeUnretainedValue() ?? ("" as CFString)
            let channel = IOReportChannelGetChannelName(item)?.takeUnretainedValue() ?? ("" as CFString)
            let unit = IOReportChannelGetUnitLabel(item)?.takeUnretainedValue() ?? ("" as CFString)
            
            samples.append(IOSample(group: group as String, subGroup: subGroup as String, channel: channel as String, unit: unit as String, delta: item))
        }
        
        return samples
    }
}

public class LimitReader: Reader<CPU_Limit> {
    private var limits: CPU_Limit = CPU_Limit()
    
    public override func read() {
        let thermal = ProcessInfo.processInfo.thermalState
        switch thermal {
        case .nominal:
            self.limits.speed = 100
            self.limits.cpus = 100
            self.limits.scheduler = 100
        case .fair:
            self.limits.speed = 90
            self.limits.cpus = 100
            self.limits.scheduler = 100
        case .serious:
            self.limits.speed = 70
            self.limits.cpus = 80
            self.limits.scheduler = 80
        case .critical:
            self.limits.speed = 50
            self.limits.cpus = 50
            self.limits.scheduler = 50
        @unknown default:
            self.limits.speed = 100
            self.limits.cpus = 100
            self.limits.scheduler = 100
        }
        self.callback(self.limits)
    }
}

public class AverageLoadReader: Reader<CPU_AverageLoad> {
    private var load: CPU_AverageLoad = CPU_AverageLoad()
    
    public override func setup() {
        self.popup = false
        self.setInterval(15)
    }
    
    public override func read() {
        var raw = StatsLoadAvg()
        if stats_core_get_load_average(&raw) {
            self.load.load1 = raw.one_min
            self.load.load5 = raw.five_min
            self.load.load15 = raw.fifteen_min
            self.callback(self.load)
        }
    }
}
