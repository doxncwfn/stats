pub mod cpu;
pub mod ram;
pub mod process;
pub mod disk;
pub mod net;
pub mod smc;

pub use cpu::StatsCpuLoad;
pub use ram::StatsRamUsage;
pub use process::{StatsProcessInfo, StatsTopProcesses};
pub use disk::{StatsDiskInfo, StatsDiskTelemetry};
pub use net::{StatsInterfaceInfo, StatsNetworkTelemetry};
pub use smc::{StatsFanInfo, StatsSensorInfo, StatsSmcTelemetry, StatsLoadAvg};

#[link(name = "IOKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {}

#[no_mangle]
pub extern "C" fn stats_core_init() -> bool {
    let mut cpu = StatsCpuLoad::default();
    cpu::poll_cpu(&mut cpu);
    let mut ram = StatsRamUsage::default();
    ram::poll_ram(&mut ram);
    let mut disk = StatsDiskTelemetry::default();
    disk::poll_disks(&mut disk);
    let mut net = StatsNetworkTelemetry::default();
    net::poll_network(&mut net);
    let mut smc_val = StatsSmcTelemetry::default();
    smc::poll_smc(&mut smc_val);
    true
}

#[no_mangle]
pub extern "C" fn stats_core_poll_cpu(out: *mut StatsCpuLoad) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { cpu::poll_cpu(&mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_poll_ram(out: *mut StatsRamUsage) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { ram::poll_ram(&mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_top_cpu_processes(limit: u32, out: *mut StatsTopProcesses) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { process::top_cpu_processes(limit, &mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_top_ram_processes(limit: u32, out: *mut StatsTopProcesses) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { process::top_ram_processes(limit, &mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_top_energy_processes(limit: u32, out: *mut StatsTopProcesses) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { process::top_energy_processes(limit, &mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_poll_disks(out: *mut StatsDiskTelemetry) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { disk::poll_disks(&mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_poll_network(out: *mut StatsNetworkTelemetry) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { net::poll_network(&mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_poll_smc(out: *mut StatsSmcTelemetry) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { smc::poll_smc(&mut *out) }
}

#[no_mangle]
pub extern "C" fn stats_core_get_load_average(out: *mut StatsLoadAvg) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe { smc::get_load_average(&mut *out) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    #[test]
    fn test_struct_sizes_and_alignments() {
        assert_eq!(size_of::<StatsCpuLoad>(), 552);
        assert_eq!(align_of::<StatsCpuLoad>(), 8);

        assert_eq!(size_of::<StatsRamUsage>(), 120);
        assert_eq!(align_of::<StatsRamUsage>(), 8);

        assert_eq!(size_of::<StatsProcessInfo>(), 88);
        assert_eq!(align_of::<StatsProcessInfo>(), 8);

        assert_eq!(size_of::<StatsTopProcesses>(), 4232);
        assert_eq!(align_of::<StatsTopProcesses>(), 8);

        assert_eq!(size_of::<StatsDiskInfo>(), 144);
        assert_eq!(align_of::<StatsDiskInfo>(), 8);

        assert_eq!(size_of::<StatsDiskTelemetry>(), 2328);
        assert_eq!(align_of::<StatsDiskTelemetry>(), 8);

        assert_eq!(size_of::<StatsInterfaceInfo>(), 56);
        assert_eq!(align_of::<StatsInterfaceInfo>(), 8);

        assert_eq!(size_of::<StatsNetworkTelemetry>(), 920);
        assert_eq!(align_of::<StatsNetworkTelemetry>(), 8);

        assert_eq!(size_of::<StatsFanInfo>(), 32);
        assert_eq!(align_of::<StatsFanInfo>(), 8);

        assert_eq!(size_of::<StatsSensorInfo>(), 16);
        assert_eq!(align_of::<StatsSensorInfo>(), 8);

        assert_eq!(size_of::<StatsSmcTelemetry>(), 672);
        assert_eq!(align_of::<StatsSmcTelemetry>(), 8);

        assert_eq!(size_of::<StatsLoadAvg>(), 24);
        assert_eq!(align_of::<StatsLoadAvg>(), 8);
    }

    #[test]
    fn test_null_pointer_guards() {
        assert!(!stats_core_poll_cpu(std::ptr::null_mut()));
        assert!(!stats_core_poll_ram(std::ptr::null_mut()));
        assert!(!stats_core_top_cpu_processes(10, std::ptr::null_mut()));
        assert!(!stats_core_top_ram_processes(10, std::ptr::null_mut()));
        assert!(!stats_core_top_energy_processes(10, std::ptr::null_mut()));
        assert!(!stats_core_poll_disks(std::ptr::null_mut()));
        assert!(!stats_core_poll_network(std::ptr::null_mut()));
        assert!(!stats_core_poll_smc(std::ptr::null_mut()));
        assert!(!stats_core_get_load_average(std::ptr::null_mut()));
    }

    #[test]
    fn test_telemetry_polling_smoke() {
        assert!(stats_core_init());

        let mut cpu = StatsCpuLoad::default();
        assert!(stats_core_poll_cpu(&mut cpu));
        assert!(cpu.core_count > 0);

        let mut ram = StatsRamUsage::default();
        assert!(stats_core_poll_ram(&mut ram));
        assert!(ram.total > 0);
        assert!(ram.used > 0);

        let mut procs = StatsTopProcesses::default();
        assert!(stats_core_top_ram_processes(10, &mut procs));
        assert!(procs.count > 0);

        let mut disks = StatsDiskTelemetry::default();
        assert!(stats_core_poll_disks(&mut disks));

        let mut net = StatsNetworkTelemetry::default();
        assert!(stats_core_poll_network(&mut net));

        let mut smc_val = StatsSmcTelemetry::default();
        assert!(stats_core_poll_smc(&mut smc_val));

        let mut load = StatsLoadAvg::default();
        assert!(stats_core_get_load_average(&mut load));
    }
}
