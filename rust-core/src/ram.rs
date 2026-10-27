use libc::{
    c_int, c_void, integer_t, mach_msg_type_number_t, mach_port_t, natural_t, size_t,
    sysctlbyname,
};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct StatsRamUsage {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub active: u64,
    pub inactive: u64,
    pub wired: u64,
    pub compressed: u64,
    pub app: u64,
    pub cache: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub swap_free: u64,
    pub swapins: i64,
    pub swapouts: i64,
    pub pressure_level: u32,
}

const HOST_VM_INFO64: c_int = 4;

#[repr(C)]
#[derive(Default)]
struct VmStatistics64 {
    free_count: natural_t,
    active_count: natural_t,
    inactive_count: natural_t,
    wire_count: natural_t,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: natural_t,
    speculative_count: natural_t,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: natural_t,
    throttled_count: natural_t,
    external_page_count: natural_t,
    internal_page_count: natural_t,
    total_uncompressed_pages_in_compressor: u64,
}

#[repr(C)]
#[derive(Default)]
struct XswUsage {
    xsu_total: u64,
    xsu_avail: u64,
    xsu_used: u64,
    xsu_pagesize: u32,
    xsu_encrypted: u32,
}

extern "C" {
    fn mach_host_self() -> mach_port_t;
    fn host_statistics64(
        host_priv: mach_port_t,
        flavor: c_int,
        host_info64_out: *mut integer_t,
        host_info64_outCnt: *mut mach_msg_type_number_t,
    ) -> c_int;
}

static TOTAL_RAM_BYTES: AtomicU64 = AtomicU64::new(0);
static PAGE_SIZE_BYTES: AtomicU64 = AtomicU64::new(0);

fn get_total_ram() -> u64 {
    let cached = TOTAL_RAM_BYTES.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }

    let mut memsize: u64 = 0;
    let mut len: size_t = std::mem::size_of::<u64>();
    let ret = unsafe {
        sysctlbyname(
            b"hw.memsize\0".as_ptr() as *const _,
            &mut memsize as *mut _ as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    };
    if ret == 0 && memsize > 0 {
        TOTAL_RAM_BYTES.store(memsize, Ordering::Relaxed);
        memsize
    } else {
        0
    }
}

fn get_page_size() -> u64 {
    let cached = PAGE_SIZE_BYTES.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }

    let mut pagesize: u64 = 0;
    let mut len: size_t = std::mem::size_of::<u64>();
    let ret = unsafe {
        sysctlbyname(
            b"hw.pagesize\0".as_ptr() as *const _,
            &mut pagesize as *mut _ as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    };
    let val = if ret == 0 && pagesize > 0 {
        pagesize
    } else {
        4096
    };
    PAGE_SIZE_BYTES.store(val, Ordering::Relaxed);
    val
}

pub fn poll_ram(out: &mut StatsRamUsage) -> bool {
    let total_ram = get_total_ram();
    let page_size = get_page_size();
    if total_ram == 0 {
        return false;
    }

    let mut stats = VmStatistics64::default();
    let mut count = (std::mem::size_of::<VmStatistics64>() / std::mem::size_of::<integer_t>()) as mach_msg_type_number_t;
    let ret = unsafe {
        host_statistics64(
            mach_host_self(),
            HOST_VM_INFO64,
            &mut stats as *mut _ as *mut integer_t,
            &mut count,
        )
    };
    if ret != 0 {
        return false;
    }

    let active = (stats.active_count as u64) * page_size;
    let speculative = (stats.speculative_count as u64) * page_size;
    let inactive = (stats.inactive_count as u64) * page_size;
    let wired = (stats.wire_count as u64) * page_size;
    let compressed = (stats.compressor_page_count as u64) * page_size;
    let purgeable = (stats.purgeable_count as u64) * page_size;
    let external = (stats.external_page_count as u64) * page_size;

    // used = active + inactive + speculative + wired + compressed - purgeable - external
    let gross_used = active + inactive + speculative + wired + compressed;
    let cache = purgeable + external;
    let used = gross_used.saturating_sub(cache);
    let free = total_ram.saturating_sub(used);
    let app = used.saturating_sub(wired + compressed);

    // Memory pressure level
    let mut pressure_level: u32 = 1; // 1 = normal
    let mut len: size_t = std::mem::size_of::<u32>();
    unsafe {
        sysctlbyname(
            b"kern.memorystatus_vm_pressure_level\0".as_ptr() as *const _,
            &mut pressure_level as *mut _ as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        );
    }

    // Swap usage
    let mut swap = XswUsage::default();
    let mut swap_len: size_t = std::mem::size_of::<XswUsage>();
    unsafe {
        sysctlbyname(
            b"vm.swapusage\0".as_ptr() as *const _,
            &mut swap as *mut _ as *mut c_void,
            &mut swap_len,
            ptr::null_mut(),
            0,
        );
    }

    out.total = total_ram;
    out.used = used;
    out.free = free;
    out.active = active;
    out.inactive = inactive;
    out.wired = wired;
    out.compressed = compressed;
    out.app = app;
    out.cache = cache;
    out.swap_total = swap.xsu_total;
    out.swap_used = swap.xsu_used;
    out.swap_free = swap.xsu_avail;
    out.swapins = stats.swapins as i64;
    out.swapouts = stats.swapouts as i64;
    out.pressure_level = pressure_level;

    true
}
