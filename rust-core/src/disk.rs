use libc::{c_char, c_int, c_void, statfs, MNT_NOWAIT};
use std::sync::Mutex;
use std::time::Instant;

pub const STATS_CORE_MAX_DISKS: usize = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsDiskInfo {
    pub bsd_name: [u8; 32],
    pub mount_point: [u8; 64],
    pub fs_type: [u8; 16],
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub read_bytes_per_sec: u64,
    pub write_bytes_per_sec: u64,
}

impl Default for StatsDiskInfo {
    fn default() -> Self {
        Self {
            bsd_name: [0; 32],
            mount_point: [0; 64],
            fs_type: [0; 16],
            total_bytes: 0,
            free_bytes: 0,
            read_bytes_per_sec: 0,
            write_bytes_per_sec: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsDiskTelemetry {
    pub count: u32,
    pub total_read_rate: u64,
    pub total_write_rate: u64,
    pub disks: [StatsDiskInfo; STATS_CORE_MAX_DISKS],
}

impl Default for StatsDiskTelemetry {
    fn default() -> Self {
        Self {
            count: 0,
            total_read_rate: 0,
            total_write_rate: 0,
            disks: [StatsDiskInfo::default(); STATS_CORE_MAX_DISKS],
        }
    }
}

extern "C" {
    fn getfsstat(buf: *mut statfs, bufsize: c_int, mode: c_int) -> c_int;
}

// IOKit types and functions for disk activity stats
type IoObjectT = u32;
type IoIteratorT = u32;
type IoRegistryEntryT = u32;
type MachPortT = u32;
type CfDictionaryRef = *const c_void;
type CfMutableDictionaryRef = *mut c_void;
type CfStringRef = *const c_void;
type CfTypeRef = *const c_void;

extern "C" {
    static kIOMainPortDefault: MachPortT;
    fn IOServiceMatching(name: *const c_char) -> CfMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: MachPortT,
        matching: CfMutableDictionaryRef,
        existing: *mut IoIteratorT,
    ) -> c_int;
    fn IOIteratorNext(iterator: IoIteratorT) -> IoObjectT;
    fn IOObjectRelease(object: IoObjectT) -> c_int;
    fn IORegistryEntryCreateCFProperties(
        entry: IoRegistryEntryT,
        properties: *mut CfMutableDictionaryRef,
        allocator: *const c_void,
        options: u32,
    ) -> c_int;

    fn CFDictionaryGetValue(theDict: CfDictionaryRef, key: *const c_void) -> *const c_void;
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        cStr: *const c_char,
        encoding: u32,
    ) -> CfStringRef;
    fn CFNumberGetValue(number: *const c_void, theType: c_int, valuePtr: *mut c_void) -> bool;
    fn CFRelease(cf: CfTypeRef);
}

const K_CF_NUMBER_SINT64_TYPE: c_int = 4;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

#[derive(Clone, Copy, Default)]
struct DiskIoSnapshot {
    total_read_bytes: u64,
    total_write_bytes: u64,
    last_sample_time: Option<Instant>,
}

static DISK_IO_STATE: Mutex<DiskIoSnapshot> = Mutex::new(DiskIoSnapshot {
    total_read_bytes: 0,
    total_write_bytes: 0,
    last_sample_time: None,
});

fn read_iokit_disk_io() -> (u64, u64) {
    let mut total_read: u64 = 0;
    let mut total_write: u64 = 0;

    unsafe {
        let matching = IOServiceMatching(b"IOBlockStorageDriver\0".as_ptr() as *const _);
        if matching.is_null() {
            return (0, 0);
        }

        let mut iterator: IoIteratorT = 0;
        let ret = IOServiceGetMatchingServices(kIOMainPortDefault, matching, &mut iterator);
        if ret != 0 || iterator == 0 {
            return (0, 0);
        }

        let stats_key = CFStringCreateWithCString(
            std::ptr::null(),
            b"Statistics\0".as_ptr() as *const _,
            K_CF_STRING_ENCODING_UTF8,
        );
        let read_key = CFStringCreateWithCString(
            std::ptr::null(),
            b"Bytes (Read)\0".as_ptr() as *const _,
            K_CF_STRING_ENCODING_UTF8,
        );
        let write_key = CFStringCreateWithCString(
            std::ptr::null(),
            b"Bytes (Write)\0".as_ptr() as *const _,
            K_CF_STRING_ENCODING_UTF8,
        );

        loop {
            let entry = IOIteratorNext(iterator);
            if entry == 0 {
                break;
            }

            let mut props: CfMutableDictionaryRef = std::ptr::null_mut();
            if IORegistryEntryCreateCFProperties(entry, &mut props, std::ptr::null(), 0) == 0
                && !props.is_null()
            {
                let stats = CFDictionaryGetValue(props as CfDictionaryRef, stats_key);
                if !stats.is_null() {
                    let r_num = CFDictionaryGetValue(stats as CfDictionaryRef, read_key);
                    if !r_num.is_null() {
                        let mut val: i64 = 0;
                        if CFNumberGetValue(r_num, K_CF_NUMBER_SINT64_TYPE, &mut val as *mut _ as *mut _) && val > 0 {
                            total_read += val as u64;
                        }
                    }

                    let w_num = CFDictionaryGetValue(stats as CfDictionaryRef, write_key);
                    if !w_num.is_null() {
                        let mut val: i64 = 0;
                        if CFNumberGetValue(w_num, K_CF_NUMBER_SINT64_TYPE, &mut val as *mut _ as *mut _) && val > 0 {
                            total_write += val as u64;
                        }
                    }
                }
                CFRelease(props as CfTypeRef);
            }
            IOObjectRelease(entry);
        }

        IOObjectRelease(iterator);
        if !stats_key.is_null() { CFRelease(stats_key); }
        if !read_key.is_null() { CFRelease(read_key); }
        if !write_key.is_null() { CFRelease(write_key); }
    }

    (total_read, total_write)
}

pub fn poll_disks(out: &mut StatsDiskTelemetry) -> bool {
    let mut fs_list: [statfs; 32] = unsafe { std::mem::zeroed() };
    let n = unsafe {
        getfsstat(
            fs_list.as_mut_ptr(),
            (fs_list.len() * std::mem::size_of::<statfs>()) as c_int,
            MNT_NOWAIT,
        )
    };

    if n <= 0 {
        return false;
    }

    let mut count: usize = 0;
    let limit = (n as usize).min(fs_list.len());

    for stat in &fs_list[..limit] {
        if count >= STATS_CORE_MAX_DISKS {
            break;
        }

        // Filter out read-only virtual system mounts (e.g. devfs, autofs, nullfs)
        let mntfrom = unsafe { std::ffi::CStr::from_ptr(stat.f_mntfromname.as_ptr()) };
        let mnton = unsafe { std::ffi::CStr::from_ptr(stat.f_mntonname.as_ptr()) };
        let fstype = unsafe { std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()) };

        let mntfrom_bytes = mntfrom.to_bytes();
        let mnton_bytes = mnton.to_bytes();
        let fstype_bytes = fstype.to_bytes();

        // Must start with /dev/ or /Volumes/
        if !mntfrom_bytes.starts_with(b"/dev/") && !mnton_bytes.starts_with(b"/Volumes/") {
            continue;
        }
        if mntfrom_bytes.starts_with(b"/dev/null") {
            continue;
        }

        let mut item = StatsDiskInfo::default();

        // BSD name from /dev/disk...
        let bsd_str = if mntfrom_bytes.starts_with(b"/dev/") {
            &mntfrom_bytes[5..]
        } else {
            mntfrom_bytes
        };
        let bsd_len = bsd_str.len().min(item.bsd_name.len() - 1);
        item.bsd_name[..bsd_len].copy_from_slice(&bsd_str[..bsd_len]);

        let mnt_len = mnton_bytes.len().min(item.mount_point.len() - 1);
        item.mount_point[..mnt_len].copy_from_slice(&mnton_bytes[..mnt_len]);

        let fs_len = fstype_bytes.len().min(item.fs_type.len() - 1);
        item.fs_type[..fs_len].copy_from_slice(&fstype_bytes[..fs_len]);

        let bsize = stat.f_bsize as u64;
        item.total_bytes = (stat.f_blocks as u64) * bsize;
        item.free_bytes = (stat.f_bavail as u64) * bsize;

        if item.total_bytes > 0 {
            out.disks[count] = item;
            count += 1;
        }
    }

    // Disk I/O activity rates
    let (cur_read, cur_write) = read_iokit_disk_io();
    let now = Instant::now();

    if let Ok(mut io) = DISK_IO_STATE.lock() {
        if let Some(prev_time) = io.last_sample_time {
            let elapsed = now.duration_since(prev_time).as_secs_f64();
            if elapsed > 0.0 && cur_read >= io.total_read_bytes && cur_write >= io.total_write_bytes {
                let delta_read = cur_read - io.total_read_bytes;
                let delta_write = cur_write - io.total_write_bytes;
                out.total_read_rate = (delta_read as f64 / elapsed) as u64;
                out.total_write_rate = (delta_write as f64 / elapsed) as u64;

                // Assign to root disk
                if count > 0 {
                    out.disks[0].read_bytes_per_sec = out.total_read_rate;
                    out.disks[0].write_bytes_per_sec = out.total_write_rate;
                }
            }
        }
        io.total_read_bytes = cur_read;
        io.total_write_bytes = cur_write;
        io.last_sample_time = Some(now);
    }

    out.count = count as u32;
    true
}
