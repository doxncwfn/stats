use libc::{c_int, c_void};
use std::sync::Mutex;

pub const STATS_CORE_MAX_TOP_PROCESSES: usize = 48;
pub const STATS_CORE_MAX_PROCESS_NAME: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsProcessInfo {
    pub pid: i32,
    pub cpu_usage: f64,
    pub ram_bytes: u64,
    pub name: [u8; STATS_CORE_MAX_PROCESS_NAME],
}

impl Default for StatsProcessInfo {
    fn default() -> Self {
        Self {
            pid: 0,
            cpu_usage: 0.0,
            ram_bytes: 0,
            name: [0; STATS_CORE_MAX_PROCESS_NAME],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsTopProcesses {
    pub count: u32,
    pub processes: [StatsProcessInfo; STATS_CORE_MAX_TOP_PROCESSES],
}

impl Default for StatsTopProcesses {
    fn default() -> Self {
        Self {
            count: 0,
            processes: [StatsProcessInfo::default(); STATS_CORE_MAX_TOP_PROCESSES],
        }
    }
}

const PROC_ALL_PIDS: u32 = 1;
const PROC_PIDTASKINFO: c_int = 4;
const RUSAGE_INFO_V4: c_int = 4;

#[repr(C)]
#[derive(Default)]
struct RusageInfoV4 {
    ri_uuid: [u8; 16],
    ri_user_time: u64,
    ri_system_time: u64,
    ri_pkg_idle_wkups: u64,
    ri_interrupt_wkups: u64,
    ri_pageins: u64,
    ri_wired_size: u64,
    ri_resident_size: u64,
    ri_phys_footprint: u64,
    ri_proc_start_abstime: u64,
    ri_proc_exit_abstime: u64,
    ri_child_user_time: u64,
    ri_child_system_time: u64,
    ri_child_pkg_idle_wkups: u64,
    ri_child_interrupt_wkups: u64,
    ri_child_pageins: u64,
    ri_child_elapsed_abstime: u64,
    ri_diskio_bytesread: u64,
    ri_diskio_byteswritten: u64,
    ri_cpu_time_qos_default: u64,
    ri_cpu_time_qos_maintenance: u64,
    ri_cpu_time_qos_background: u64,
    ri_cpu_time_qos_utility: u64,
    ri_cpu_time_qos_legacy: u64,
    ri_cpu_time_qos_user_initiated: u64,
    ri_cpu_time_qos_user_interactive: u64,
    ri_billed_system_time: u64,
    ri_serviced_system_time: u64,
}

#[repr(C)]
#[derive(Default)]
struct ProcTaskinfo {
    pti_virtual_size: u64,
    pti_resident_size: u64,
    pti_total_user: u64,
    pti_total_system: u64,
    pti_threads_user: u64,
    pti_threads_system: u64,
    pti_policy: i32,
    pti_faults: i32,
    pti_pageins: i32,
    pti_cow_faults: i32,
    pti_messages_sent: i32,
    pti_messages_received: i32,
    pti_syscalls_mach: i32,
    pti_syscalls_unix: i32,
    pti_csw: i32,
    pti_threadnum: i32,
    pti_numrunning: i32,
    pti_priority: i32,
}

#[repr(C)]
#[derive(Default)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

extern "C" {
    fn proc_listpids(proc_type: u32, typeinfo: u32, buffer: *mut c_void, buffersize: c_int) -> c_int;
    fn proc_pidinfo(pid: c_int, flavor: c_int, arg: u64, buffer: *mut c_void, buffersize: c_int) -> c_int;
    fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut c_void) -> c_int;
    fn proc_name(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> c_int;
}

const PID_HISTORY_SIZE: usize = 512;

#[derive(Clone, Copy, Default)]
struct PidCpuEntry {
    pid: i32,
    cpu_time_ns: u64,
    last_seen_tick: u64,
}

struct PidCpuHistory {
    entries: [PidCpuEntry; PID_HISTORY_SIZE],
    last_poll_time: u64,
    current_tick: u64,
    timebase_numer: u64,
    timebase_denom: u64,
}

static CPU_HISTORY: Mutex<PidCpuHistory> = Mutex::new(PidCpuHistory {
    entries: [PidCpuEntry { pid: 0, cpu_time_ns: 0, last_seen_tick: 0 }; PID_HISTORY_SIZE],
    last_poll_time: 0,
    current_tick: 0,
    timebase_numer: 0,
    timebase_denom: 0,
});

#[derive(Clone, Copy, Default)]
struct PidEnergyEntry {
    pid: i32,
    cpu_time_ns: u64,
    pkg_idle_wkups: u64,
    last_seen_tick: u64,
}

struct PidEnergyHistory {
    entries: [PidEnergyEntry; PID_HISTORY_SIZE],
    last_poll_time: u64,
    current_tick: u64,
    timebase_numer: u64,
    timebase_denom: u64,
}

static ENERGY_HISTORY: Mutex<PidEnergyHistory> = Mutex::new(PidEnergyHistory {
    entries: [PidEnergyEntry { pid: 0, cpu_time_ns: 0, pkg_idle_wkups: 0, last_seen_tick: 0 }; PID_HISTORY_SIZE],
    last_poll_time: 0,
    current_tick: 0,
    timebase_numer: 0,
    timebase_denom: 0,
});

pub fn top_ram_processes(limit: u32, out: &mut StatsTopProcesses) -> bool {
    let limit = (limit as usize).clamp(1, STATS_CORE_MAX_TOP_PROCESSES);
    let mut pids = [0i32; 2048];
    let bytes_ret = unsafe {
        proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut c_void,
            (pids.len() * std::mem::size_of::<i32>()) as c_int,
        )
    };
    if bytes_ret <= 0 {
        return false;
    }

    let n_pids = (bytes_ret as usize / std::mem::size_of::<i32>()).min(pids.len());
    let mut top = [StatsProcessInfo::default(); STATS_CORE_MAX_TOP_PROCESSES];
    let mut count: usize = 0;

    let mut taskinfo = ProcTaskinfo::default();
    let taskinfo_size = std::mem::size_of::<ProcTaskinfo>() as c_int;

    for &pid in &pids[..n_pids] {
        if pid <= 0 {
            continue;
        }

        let mut rusage = RusageInfoV4::default();
        let ret = unsafe {
            proc_pid_rusage(
                pid,
                RUSAGE_INFO_V4,
                &mut rusage as *mut _ as *mut c_void,
            )
        };

        let ram_bytes = if ret == 0 && rusage.ri_phys_footprint > 0 {
            rusage.ri_phys_footprint
        } else {
            let ret = unsafe {
                proc_pidinfo(
                    pid,
                    PROC_PIDTASKINFO,
                    0,
                    &mut taskinfo as *mut _ as *mut c_void,
                    taskinfo_size,
                )
            };
            if ret < taskinfo_size {
                continue;
            }
            taskinfo.pti_resident_size
        };

        if ram_bytes == 0 {
            continue;
        }

        // If qualifying for top-N
        if count < limit || ram_bytes > top[count - 1].ram_bytes {
            let mut name = [0u8; STATS_CORE_MAX_PROCESS_NAME];
            unsafe {
                proc_name(pid, name.as_mut_ptr() as *mut c_void, (STATS_CORE_MAX_PROCESS_NAME - 1) as u32);
            }
            name[STATS_CORE_MAX_PROCESS_NAME - 1] = 0;

            let item = StatsProcessInfo {
                pid,
                cpu_usage: 0.0,
                ram_bytes,
                name,
            };

            let mut insert_pos = count;
            while insert_pos > 0 && top[insert_pos - 1].ram_bytes < ram_bytes {
                if insert_pos < limit {
                    top[insert_pos] = top[insert_pos - 1];
                }
                insert_pos -= 1;
            }
            if insert_pos < limit {
                top[insert_pos] = item;
            }
            if count < limit {
                count += 1;
            }
        }
    }

    out.count = count as u32;
    out.processes = top;
    true
}

pub fn top_cpu_processes(limit: u32, out: &mut StatsTopProcesses) -> bool {
    let limit = (limit as usize).clamp(1, STATS_CORE_MAX_TOP_PROCESSES);
    let mut pids = [0i32; 2048];
    let bytes_ret = unsafe {
        proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut c_void,
            (pids.len() * std::mem::size_of::<i32>()) as c_int,
        )
    };
    if bytes_ret <= 0 {
        return false;
    }

    let n_pids = (bytes_ret as usize / std::mem::size_of::<i32>()).min(pids.len());

    let mut history = match CPU_HISTORY.lock() {
        Ok(h) => h,
        Err(p) => p.into_inner(),
    };

    if history.timebase_denom == 0 {
        let mut info = MachTimebaseInfo::default();
        unsafe {
            mach_timebase_info(&mut info);
        }
        history.timebase_numer = if info.numer > 0 { info.numer as u64 } else { 1 };
        history.timebase_denom = if info.denom > 0 { info.denom as u64 } else { 1 };
    }

    let now_mach = unsafe { mach_absolute_time() };
    let prev_poll_mach = history.last_poll_time;
    history.last_poll_time = now_mach;
    history.current_tick += 1;
    let cur_tick = history.current_tick;

    let delta_mach = if prev_poll_mach > 0 && now_mach > prev_poll_mach {
        now_mach - prev_poll_mach
    } else {
        if history.timebase_numer > 0 && history.timebase_denom > 0 {
            (1_000_000_000u64 * history.timebase_denom) / history.timebase_numer
        } else {
            24_000_000
        }
    };

    let mut top = [StatsProcessInfo::default(); STATS_CORE_MAX_TOP_PROCESSES];
    let mut count: usize = 0;

    let mut taskinfo = ProcTaskinfo::default();
    let taskinfo_size = std::mem::size_of::<ProcTaskinfo>() as c_int;

    for &pid in &pids[..n_pids] {
        if pid <= 0 {
            continue;
        }

        let ret = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTASKINFO,
                0,
                &mut taskinfo as *mut _ as *mut c_void,
                taskinfo_size,
            )
        };
        if ret < taskinfo_size {
            continue;
        }

        let total_cpu_ticks = taskinfo.pti_total_user + taskinfo.pti_total_system;

        // Lookup in history
        let mut found_idx: Option<usize> = None;
        let mut empty_idx: Option<usize> = None;
        let mut oldest_idx = 0;
        let mut oldest_tick = u64::MAX;

        for (i, entry) in history.entries.iter().enumerate() {
            if entry.pid == pid {
                found_idx = Some(i);
                break;
            }
            if entry.pid == 0 && empty_idx.is_none() {
                empty_idx = Some(i);
            }
            if entry.last_seen_tick < oldest_tick {
                oldest_tick = entry.last_seen_tick;
                oldest_idx = i;
            }
        }

        let cpu_usage = if let Some(idx) = found_idx {
            let prev_cpu = history.entries[idx].cpu_time_ns;
            history.entries[idx].cpu_time_ns = total_cpu_ticks;
            history.entries[idx].last_seen_tick = cur_tick;
            let delta_cpu = total_cpu_ticks.saturating_sub(prev_cpu);
            (delta_cpu as f64 / delta_mach as f64) * 100.0
        } else {
            let target_idx = empty_idx.unwrap_or(oldest_idx);
            history.entries[target_idx] = PidCpuEntry {
                pid,
                cpu_time_ns: total_cpu_ticks,
                last_seen_tick: cur_tick,
            };
            0.0
        };

        if count < limit || cpu_usage > top[count - 1].cpu_usage {
            let mut name = [0u8; STATS_CORE_MAX_PROCESS_NAME];
            unsafe {
                proc_name(pid, name.as_mut_ptr() as *mut c_void, (STATS_CORE_MAX_PROCESS_NAME - 1) as u32);
            }
            name[STATS_CORE_MAX_PROCESS_NAME - 1] = 0;

            let item = StatsProcessInfo {
                pid,
                cpu_usage,
                ram_bytes: taskinfo.pti_resident_size,
                name,
            };

            let mut insert_pos = count;
            while insert_pos > 0 && top[insert_pos - 1].cpu_usage < cpu_usage {
                if insert_pos < limit {
                    top[insert_pos] = top[insert_pos - 1];
                }
                insert_pos -= 1;
            }
            if insert_pos < limit {
                top[insert_pos] = item;
            }
            if count < limit {
                count += 1;
            }
        }
    }

    if cur_tick % 15 == 0 {
        for entry in history.entries.iter_mut() {
            if entry.pid != 0 && cur_tick.saturating_sub(entry.last_seen_tick) > 15 {
                *entry = PidCpuEntry::default();
            }
        }
    }

    out.count = count as u32;
    out.processes = top;
    true
}

pub fn top_energy_processes(limit: u32, out: &mut StatsTopProcesses) -> bool {
    let limit = (limit as usize).clamp(1, STATS_CORE_MAX_TOP_PROCESSES);
    let mut pids = [0i32; 2048];
    let bytes_ret = unsafe {
        proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut c_void,
            (pids.len() * std::mem::size_of::<i32>()) as c_int,
        )
    };
    if bytes_ret <= 0 {
        return false;
    }

    let n_pids = (bytes_ret as usize / std::mem::size_of::<i32>()).min(pids.len());
    let now = unsafe { mach_absolute_time() };

    let mut history = match ENERGY_HISTORY.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if history.timebase_denom == 0 {
        let mut info = MachTimebaseInfo::default();
        unsafe {
            mach_timebase_info(&mut info);
        }
        history.timebase_numer = if info.numer != 0 { info.numer as u64 } else { 1 };
        history.timebase_denom = if info.denom != 0 { info.denom as u64 } else { 1 };
    }

    let prev_time = history.last_poll_time;
    history.last_poll_time = now;
    history.current_tick += 1;
    let cur_tick = history.current_tick;

    let delta_time_ns = if prev_time > 0 && now > prev_time {
        let elapsed_ticks = now - prev_time;
        (elapsed_ticks * history.timebase_numer) / history.timebase_denom
    } else {
        1_000_000_000
    };

    let delta_time_sec = (delta_time_ns as f64) / 1_000_000_000.0;

    let mut top = [StatsProcessInfo::default(); STATS_CORE_MAX_TOP_PROCESSES];
    let mut count: usize = 0;

    let mut rusage = RusageInfoV4::default();

    for &pid in &pids[..n_pids] {
        if pid <= 0 {
            continue;
        }

        let ret = unsafe {
            proc_pid_rusage(
                pid,
                RUSAGE_INFO_V4,
                &mut rusage as *mut _ as *mut c_void,
            )
        };
        if ret != 0 {
            continue;
        }

        let total_cpu_ns = rusage.ri_user_time + rusage.ri_system_time;
        let pkg_wkups = rusage.ri_pkg_idle_wkups;

        // Lookup in history
        let mut found_idx: Option<usize> = None;
        let mut empty_idx: Option<usize> = None;
        let mut oldest_idx = 0;
        let mut oldest_tick = u64::MAX;

        for (i, entry) in history.entries.iter().enumerate() {
            if entry.pid == pid {
                found_idx = Some(i);
                break;
            }
            if entry.pid == 0 && empty_idx.is_none() {
                empty_idx = Some(i);
            }
            if entry.last_seen_tick < oldest_tick {
                oldest_tick = entry.last_seen_tick;
                oldest_idx = i;
            }
        }

        let energy_score = if let Some(idx) = found_idx {
            let prev_cpu = history.entries[idx].cpu_time_ns;
            let prev_wkups = history.entries[idx].pkg_idle_wkups;
            history.entries[idx].cpu_time_ns = total_cpu_ns;
            history.entries[idx].pkg_idle_wkups = pkg_wkups;
            history.entries[idx].last_seen_tick = cur_tick;

            let delta_cpu = total_cpu_ns.saturating_sub(prev_cpu);
            let delta_wkups = pkg_wkups.saturating_sub(prev_wkups);

            let cpu_pct = (delta_cpu as f64 / delta_time_ns as f64) * 100.0;
            let wkups_per_sec = if delta_time_sec > 0.0 {
                (delta_wkups as f64) / delta_time_sec
            } else {
                0.0
            };
            // Energy impact = CPU% + 0.1 * package idle wakeups/sec
            cpu_pct + (wkups_per_sec * 0.1)
        } else {
            let target_idx = empty_idx.unwrap_or(oldest_idx);
            history.entries[target_idx] = PidEnergyEntry {
                pid,
                cpu_time_ns: total_cpu_ns,
                pkg_idle_wkups: pkg_wkups,
                last_seen_tick: cur_tick,
            };
            0.0
        };

        if count < limit || energy_score > top[count - 1].cpu_usage {
            let mut name = [0u8; STATS_CORE_MAX_PROCESS_NAME];
            unsafe {
                proc_name(pid, name.as_mut_ptr() as *mut c_void, (STATS_CORE_MAX_PROCESS_NAME - 1) as u32);
            }
            name[STATS_CORE_MAX_PROCESS_NAME - 1] = 0;

            let item = StatsProcessInfo {
                pid,
                cpu_usage: energy_score,
                ram_bytes: rusage.ri_phys_footprint,
                name,
            };

            let mut insert_pos = count;
            while insert_pos > 0 && top[insert_pos - 1].cpu_usage < energy_score {
                if insert_pos < limit {
                    top[insert_pos] = top[insert_pos - 1];
                }
                insert_pos -= 1;
            }
            if insert_pos < limit {
                top[insert_pos] = item;
            }
            if count < limit {
                count += 1;
            }
        }
    }

    if cur_tick % 15 == 0 {
        for entry in history.entries.iter_mut() {
            if entry.pid != 0 && cur_tick.saturating_sub(entry.last_seen_tick) > 15 {
                *entry = PidEnergyEntry::default();
            }
        }
    }

    out.count = count as u32;
    out.processes = top;
    true
}
