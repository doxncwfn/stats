use stats_core::{
    stats_core_init, stats_core_poll_cpu, stats_core_poll_ram,
    stats_core_poll_disks, stats_core_poll_network, stats_core_poll_smc,
    stats_core_get_load_average,
    StatsCpuLoad, StatsRamUsage,
    StatsDiskTelemetry, StatsNetworkTelemetry, StatsSmcTelemetry, StatsLoadAvg,
};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    println!("=== Testing libstats_core Telemetry (Milestone 4) ===");
    stats_core_init();

    // Sleep 1 second to allow rate deltas to accumulate
    sleep(Duration::from_millis(1000));

    // Test CPU
    let mut cpu = StatsCpuLoad::default();
    assert!(stats_core_poll_cpu(&mut cpu), "stats_core_poll_cpu failed");
    println!(
        "CPU Load: Total: {:.1}%, System: {:.1}%, User: {:.1}%, Idle: {:.1}% across {} cores",
        cpu.total_usage * 100.0,
        cpu.system_load * 100.0,
        cpu.user_load * 100.0,
        cpu.idle_load * 100.0,
        cpu.core_count
    );

    // Test Load Average (replaces uptime subprocess)
    let mut load = StatsLoadAvg::default();
    if stats_core_get_load_average(&mut load) {
        println!("Load Average: 1m: {:.2}, 5m: {:.2}, 15m: {:.2}", load.one_min, load.five_min, load.fifteen_min);
    }

    // Test RAM
    let mut ram = StatsRamUsage::default();
    assert!(stats_core_poll_ram(&mut ram), "stats_core_poll_ram failed");
    let to_gb = |b: u64| (b as f64) / (1024.0 * 1024.0 * 1024.0);
    println!(
        "RAM Usage: Used: {:.2} GB / Total: {:.2} GB (Free: {:.2} GB, App: {:.2} GB, Wired: {:.2} GB, Compressed: {:.2} GB, Cache: {:.2} GB)",
        to_gb(ram.used),
        to_gb(ram.total),
        to_gb(ram.free),
        to_gb(ram.app),
        to_gb(ram.wired),
        to_gb(ram.compressed),
        to_gb(ram.cache)
    );

    let mut top_ram = stats_core::StatsTopProcesses::default();
    stats_core::stats_core_top_ram_processes(15, &mut top_ram);
    println!("\nTop RAM Processes (requested 15, got {}):", top_ram.count);
    for i in 0..top_ram.count as usize {
        let p = &top_ram.processes[i];
        let name = std::ffi::CStr::from_bytes_until_nul(&p.name).map(|s| s.to_string_lossy()).unwrap_or_default();
        println!("  [{:>2}] PID {:>5}: {:<25} {:.1} MB", i + 1, p.pid, name, (p.ram_bytes as f64) / 1024.0 / 1024.0);
    }

    // Warm up CPU process baseline then sample
    let mut top_cpu = stats_core::StatsTopProcesses::default();
    stats_core::stats_core_top_cpu_processes(10, &mut top_cpu);
    std::thread::sleep(std::time::Duration::from_millis(500));
    stats_core::stats_core_top_cpu_processes(10, &mut top_cpu);
    println!("\nTop CPU Processes (requested 10, got {}):", top_cpu.count);
    for i in 0..top_cpu.count as usize {
        let p = &top_cpu.processes[i];
        let name = std::ffi::CStr::from_bytes_until_nul(&p.name).map(|s| s.to_string_lossy()).unwrap_or_default();
        println!("  [{:>2}] PID {:>5}: {:<25} {:.1}%", i + 1, p.pid, name, p.cpu_usage);
    }

    // Test Disk
    let mut disks = StatsDiskTelemetry::default();
    assert!(stats_core_poll_disks(&mut disks), "stats_core_poll_disks failed");
    println!("\nDisks Telemetry (found {} disks):", disks.count);
    for i in 0..disks.count as usize {
        let d = &disks.disks[i];
        let bsd = std::ffi::CStr::from_bytes_until_nul(&d.bsd_name).map(|s| s.to_string_lossy()).unwrap_or_default();
        let mnt = std::ffi::CStr::from_bytes_until_nul(&d.mount_point).map(|s| s.to_string_lossy()).unwrap_or_default();
        let fs = std::ffi::CStr::from_bytes_until_nul(&d.fs_type).map(|s| s.to_string_lossy()).unwrap_or_default();
        println!(
            "  [{}] mnt: {:<20} fs: {:<6} total: {:.1} GB, free: {:.1} GB (R: {} B/s, W: {} B/s)",
            bsd, mnt, fs, to_gb(d.total_bytes), to_gb(d.free_bytes), d.read_bytes_per_sec, d.write_bytes_per_sec
        );
    }

    // Test Network
    let mut net = StatsNetworkTelemetry::default();
    assert!(stats_core_poll_network(&mut net), "stats_core_poll_network failed");
    let to_kb = |b: u64| (b as f64) / 1024.0;
    println!("\nNetwork Telemetry (Aggregate: Down {:.1} KB/s, Up {:.1} KB/s):", to_kb(net.total_download_rate), to_kb(net.total_upload_rate));
    for i in 0..net.count as usize {
        let ifa = &net.interfaces[i];
        let name = std::ffi::CStr::from_bytes_until_nul(&ifa.name).map(|s| s.to_string_lossy()).unwrap_or_default();
        if ifa.is_up || ifa.download_bytes_per_sec > 0 || ifa.upload_bytes_per_sec > 0 {
            println!(
                "  [{}] Down: {:>7.1} KB/s  Up: {:>7.1} KB/s  (Total In: {:.1} MB, Out: {:.1} MB, Up: {})",
                name, to_kb(ifa.download_bytes_per_sec), to_kb(ifa.upload_bytes_per_sec),
                to_gb(ifa.total_download_bytes) * 1024.0, to_gb(ifa.total_upload_bytes) * 1024.0, ifa.is_up
            );
        }
    }

    // Test SMC & Hardware Sensors
    let mut smc = StatsSmcTelemetry::default();
    if stats_core_poll_smc(&mut smc) {
        println!("\nSMC Telemetry:");
        println!("  CPU Temp: {:.1} °C, GPU Temp: {:.1} °C", smc.cpu_temp, smc.gpu_temp);
        println!("  Fans found: {}", smc.fan_count);
        for i in 0..smc.fan_count as usize {
            let f = &smc.fans[i];
            println!("    Fan #{}: {:.0} RPM (min: {:.0}, max: {:.0})", f.id, f.current_rpm, f.min_rpm, f.max_rpm);
        }
    }

    println!("\n=== Verification Successful! ===");
}
