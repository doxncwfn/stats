use libc::{c_int, getifaddrs, freeifaddrs, ifaddrs, AF_LINK, IFF_UP, IFF_RUNNING};
use std::sync::Mutex;
use std::time::Instant;

pub const STATS_CORE_MAX_NET_INTERFACES: usize = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsInterfaceInfo {
    pub name: [u8; 16],
    pub upload_bytes_per_sec: u64,
    pub download_bytes_per_sec: u64,
    pub total_upload_bytes: u64,
    pub total_download_bytes: u64,
    pub is_up: bool,
}

impl Default for StatsInterfaceInfo {
    fn default() -> Self {
        Self {
            name: [0; 16],
            upload_bytes_per_sec: 0,
            download_bytes_per_sec: 0,
            total_upload_bytes: 0,
            total_download_bytes: 0,
            is_up: false,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsNetworkTelemetry {
    pub count: u32,
    pub total_upload_rate: u64,
    pub total_download_rate: u64,
    pub interfaces: [StatsInterfaceInfo; STATS_CORE_MAX_NET_INTERFACES],
}

impl Default for StatsNetworkTelemetry {
    fn default() -> Self {
        Self {
            count: 0,
            total_upload_rate: 0,
            total_download_rate: 0,
            interfaces: [StatsInterfaceInfo::default(); STATS_CORE_MAX_NET_INTERFACES],
        }
    }
}

#[repr(C)]
struct IfData64 {
    ifi_type: u8,
    ifi_typelen: u8,
    ifi_physical: u8,
    ifi_addrlen: u8,
    ifi_hdrlen: u8,
    ifi_recvquota: u8,
    ifi_xmitquota: u8,
    ifi_unused1: u8,
    ifi_mtu: u32,
    ifi_metric: u32,
    ifi_baudrate: u64,
    ifi_ipackets: u64,
    ifi_ierrors: u64,
    ifi_opackets: u64,
    ifi_oerrors: u64,
    ifi_collisions: u64,
    ifi_ibytes: u64,
    ifi_obytes: u64,
    ifi_imcasts: u64,
    ifi_omcasts: u64,
    ifi_iqdrops: u64,
    ifi_noproto: u64,
    ifi_recvtiming: u32,
    ifi_xmittiming: u32,
    ifi_lastchange: [u32; 4],
}

#[derive(Clone, Copy, Default)]
struct IfaceHistory {
    name: [u8; 16],
    ibytes: u64,
    obytes: u64,
    last_sample_time: Option<Instant>,
}

static NET_HISTORY: Mutex<[IfaceHistory; STATS_CORE_MAX_NET_INTERFACES]> =
    Mutex::new([IfaceHistory {
        name: [0; 16],
        ibytes: 0,
        obytes: 0,
        last_sample_time: None,
    }; STATS_CORE_MAX_NET_INTERFACES]);

pub fn poll_network(out: &mut StatsNetworkTelemetry) -> bool {
    let mut ifap: *mut ifaddrs = std::ptr::null_mut();
    if unsafe { getifaddrs(&mut ifap) } != 0 || ifap.is_null() {
        return false;
    }

    let now = Instant::now();
    let mut history = match NET_HISTORY.lock() {
        Ok(h) => h,
        Err(_) => {
            unsafe { freeifaddrs(ifap); }
            return false;
        }
    };

    let mut count: usize = 0;
    let mut agg_up: u64 = 0;
    let mut agg_down: u64 = 0;

    let mut curr = ifap;
    while !curr.is_null() && count < STATS_CORE_MAX_NET_INTERFACES {
        let ifa = unsafe { &*curr };
        curr = ifa.ifa_next;

        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() || ifa.ifa_data.is_null() {
            continue;
        }

        let sa_family = unsafe { (*ifa.ifa_addr).sa_family as c_int };
        if sa_family != AF_LINK {
            continue;
        }

        let c_name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) };
        let name_bytes = c_name.to_bytes();

        // Skip loopback "lo0"
        if name_bytes.starts_with(b"lo") {
            continue;
        }

        let ifd = unsafe { &*(ifa.ifa_data as *const IfData64) };
        let cur_ibytes = ifd.ifi_ibytes;
        let cur_obytes = ifd.ifi_obytes;
        let is_up = (ifa.ifa_flags as i32 & (IFF_UP | IFF_RUNNING)) == (IFF_UP | IFF_RUNNING);

        let mut item = StatsInterfaceInfo::default();
        let name_len = name_bytes.len().min(item.name.len() - 1);
        item.name[..name_len].copy_from_slice(&name_bytes[..name_len]);
        item.total_download_bytes = cur_ibytes;
        item.total_upload_bytes = cur_obytes;
        item.is_up = is_up;

        // Lookup or allocate history slot
        let mut slot_idx: Option<usize> = None;
        let mut empty_idx: Option<usize> = None;
        for (i, h) in history.iter().enumerate() {
            if h.name == item.name {
                slot_idx = Some(i);
                break;
            }
            if h.name[0] == 0 && empty_idx.is_none() {
                empty_idx = Some(i);
            }
        }

        let target_idx = slot_idx.or(empty_idx).unwrap_or(count);
        if target_idx < history.len() {
            let h = &mut history[target_idx];
            if h.name == item.name {
                if let Some(prev_time) = h.last_sample_time {
                    let elapsed = now.duration_since(prev_time).as_secs_f64();
                    if elapsed > 0.0 && cur_ibytes >= h.ibytes && cur_obytes >= h.obytes {
                        let delta_in = cur_ibytes - h.ibytes;
                        let delta_out = cur_obytes - h.obytes;
                        item.download_bytes_per_sec = (delta_in as f64 / elapsed) as u64;
                        item.upload_bytes_per_sec = (delta_out as f64 / elapsed) as u64;

                        // Add to aggregate rates
                        agg_down += item.download_bytes_per_sec;
                        agg_up += item.upload_bytes_per_sec;
                    }
                }
            } else {
                h.name = item.name;
            }
            h.ibytes = cur_ibytes;
            h.obytes = cur_obytes;
            h.last_sample_time = Some(now);
        }

        out.interfaces[count] = item;
        count += 1;
    }

    unsafe { freeifaddrs(ifap); }

    out.count = count as u32;
    out.total_download_rate = agg_down;
    out.total_upload_rate = agg_up;
    true
}
