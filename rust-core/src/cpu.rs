use std::sync::Mutex;
use libc::{
    c_int, integer_t, mach_msg_type_number_t, mach_port_t, natural_t,
    processor_info_array_t,
};

pub const STATS_CORE_MAX_CORES: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsCpuLoad {
    pub total_usage: f64,
    pub system_load: f64,
    pub user_load: f64,
    pub idle_load: f64,
    pub core_count: u32,
    pub usage_per_core: [f64; STATS_CORE_MAX_CORES],
}

impl Default for StatsCpuLoad {
    fn default() -> Self {
        Self {
            total_usage: 0.0,
            system_load: 0.0,
            user_load: 0.0,
            idle_load: 0.0,
            core_count: 0,
            usage_per_core: [0.0; STATS_CORE_MAX_CORES],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct CoreTicks {
    user: u32,
    system: u32,
    idle: u32,
    nice: u32,
}

#[derive(Clone, Copy, Default)]
struct GlobalTicks {
    user: u32,
    system: u32,
    idle: u32,
    nice: u32,
}

struct CpuState {
    initialized: bool,
    prev_global: GlobalTicks,
    prev_cores: [CoreTicks; STATS_CORE_MAX_CORES],
    core_count: usize,
}

static CPU_STATE: Mutex<CpuState> = Mutex::new(CpuState {
    initialized: false,
    prev_global: GlobalTicks { user: 0, system: 0, idle: 0, nice: 0 },
    prev_cores: [CoreTicks { user: 0, system: 0, idle: 0, nice: 0 }; STATS_CORE_MAX_CORES],
    core_count: 0,
});

const PROCESSOR_CPU_LOAD_INFO: c_int = 2;
const HOST_CPU_LOAD_INFO: c_int = 3;
const CPU_STATE_USER: usize = 0;
const CPU_STATE_SYSTEM: usize = 1;
const CPU_STATE_IDLE: usize = 2;
const CPU_STATE_NICE: usize = 3;
const CPU_STATE_MAX: usize = 4;

#[repr(C)]
struct HostCpuLoadInfo {
    cpu_ticks: [natural_t; CPU_STATE_MAX],
}

extern "C" {
    fn mach_host_self() -> mach_port_t;
    fn mach_task_self() -> mach_port_t;
    fn host_processor_info(
        host: mach_port_t,
        flavor: c_int,
        out_processor_count: *mut natural_t,
        out_processor_info: *mut processor_info_array_t,
        out_processor_infoCnt: *mut mach_msg_type_number_t,
    ) -> c_int;
    fn host_statistics(
        host_priv: mach_port_t,
        flavor: c_int,
        host_info_out: *mut integer_t,
        host_info_outCnt: *mut mach_msg_type_number_t,
    ) -> c_int;
}

pub fn poll_cpu(out: &mut StatsCpuLoad) -> bool {
    let mut global_info = HostCpuLoadInfo { cpu_ticks: [0; CPU_STATE_MAX] };
    let mut count = (std::mem::size_of::<HostCpuLoadInfo>() / std::mem::size_of::<integer_t>()) as mach_msg_type_number_t;
    let kerr = unsafe {
        host_statistics(
            mach_host_self(),
            HOST_CPU_LOAD_INFO,
            &mut global_info as *mut _ as *mut integer_t,
            &mut count,
        )
    };
    if kerr != 0 {
        return false;
    }

    let mut processor_count: natural_t = 0;
    let mut info_array: processor_info_array_t = std::ptr::null_mut();
    let mut info_count: mach_msg_type_number_t = 0;

    let res = unsafe {
        host_processor_info(
            mach_host_self(),
            PROCESSOR_CPU_LOAD_INFO,
            &mut processor_count,
            &mut info_array,
            &mut info_count,
        )
    };

    if res != 0 || info_array.is_null() {
        return false;
    }

    let n_cores = (processor_count as usize).min(STATS_CORE_MAX_CORES);
    let mut cur_cores = [CoreTicks::default(); STATS_CORE_MAX_CORES];

    unsafe {
        let slice = std::slice::from_raw_parts(info_array, info_count as usize);
        for i in 0..n_cores {
            let base = i * CPU_STATE_MAX;
            if base + CPU_STATE_NICE < slice.len() {
                cur_cores[i] = CoreTicks {
                    user: slice[base + CPU_STATE_USER] as u32,
                    system: slice[base + CPU_STATE_SYSTEM] as u32,
                    idle: slice[base + CPU_STATE_IDLE] as u32,
                    nice: slice[base + CPU_STATE_NICE] as u32,
                };
            }
        }

        // MANDATORY: Immediately release Mach VM memory to prevent kernel memory leak
        libc::vm_deallocate(
            mach_task_self(),
            info_array as usize,
            (info_count as usize) * std::mem::size_of::<integer_t>(),
        );
    }

    let mut state = match CPU_STATE.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let cur_global = GlobalTicks {
        user: global_info.cpu_ticks[CPU_STATE_USER],
        system: global_info.cpu_ticks[CPU_STATE_SYSTEM],
        idle: global_info.cpu_ticks[CPU_STATE_IDLE],
        nice: global_info.cpu_ticks[CPU_STATE_NICE],
    };

    if !state.initialized {
        state.initialized = true;
        state.prev_global = cur_global;
        state.prev_cores = cur_cores;
        state.core_count = n_cores;
        out.core_count = n_cores as u32;
        return true;
    }

    // Global CPU percentages
    let user_diff = cur_global.user.wrapping_sub(state.prev_global.user) as f64;
    let sys_diff = cur_global.system.wrapping_sub(state.prev_global.system) as f64;
    let idle_diff = cur_global.idle.wrapping_sub(state.prev_global.idle) as f64;
    let nice_diff = cur_global.nice.wrapping_sub(state.prev_global.nice) as f64;
    let total_ticks = user_diff + sys_diff + idle_diff + nice_diff;

    if total_ticks > 0.0 {
        out.system_load = (sys_diff / total_ticks).clamp(0.0, 1.0);
        out.user_load = (user_diff / total_ticks).clamp(0.0, 1.0);
        out.idle_load = (idle_diff / total_ticks).clamp(0.0, 1.0);
        out.total_usage = ((user_diff + sys_diff + nice_diff) / total_ticks).clamp(0.0, 1.0);
    }

    // Per-core percentages
    out.core_count = n_cores as u32;
    for i in 0..n_cores {
        let u = cur_cores[i].user.wrapping_sub(state.prev_cores[i].user) as f64;
        let s = cur_cores[i].system.wrapping_sub(state.prev_cores[i].system) as f64;
        let n = cur_cores[i].nice.wrapping_sub(state.prev_cores[i].nice) as f64;
        let id = cur_cores[i].idle.wrapping_sub(state.prev_cores[i].idle) as f64;
        let in_use = u + s + n;
        let total = in_use + id;
        out.usage_per_core[i] = if total > 0.0 { (in_use / total).clamp(0.0, 1.0) } else { 0.0 };
    }

    state.prev_global = cur_global;
    state.prev_cores = cur_cores;
    state.core_count = n_cores;

    true
}
