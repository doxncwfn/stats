use libc::{c_char, c_int, c_void, mach_port_t, size_t};
use std::sync::Mutex;

pub const STATS_CORE_MAX_FANS: usize = 4;
pub const STATS_CORE_MAX_SENSORS: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsFanInfo {
    pub id: u32,
    pub current_rpm: f64,
    pub min_rpm: f64,
    pub max_rpm: f64,
}

impl Default for StatsFanInfo {
    fn default() -> Self {
        Self {
            id: 0,
            current_rpm: 0.0,
            min_rpm: 0.0,
            max_rpm: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsSensorInfo {
    pub key: [u8; 8],
    pub value: f64,
}

impl Default for StatsSensorInfo {
    fn default() -> Self {
        Self {
            key: [0; 8],
            value: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsSmcTelemetry {
    pub cpu_temp: f64,
    pub gpu_temp: f64,
    pub fan_count: u32,
    pub fans: [StatsFanInfo; STATS_CORE_MAX_FANS],
    pub sensor_count: u32,
    pub sensors: [StatsSensorInfo; STATS_CORE_MAX_SENSORS],
}

impl Default for StatsSmcTelemetry {
    fn default() -> Self {
        Self {
            cpu_temp: 0.0,
            gpu_temp: 0.0,
            fan_count: 0,
            fans: [StatsFanInfo::default(); STATS_CORE_MAX_FANS],
            sensor_count: 0,
            sensors: [StatsSensorInfo::default(); STATS_CORE_MAX_SENSORS],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StatsLoadAvg {
    pub one_min: f64,
    pub five_min: f64,
    pub fifteen_min: f64,
}

impl Default for StatsLoadAvg {
    fn default() -> Self {
        Self {
            one_min: 0.0,
            five_min: 0.0,
            fifteen_min: 0.0,
        }
    }
}

extern "C" {
    fn getloadavg(loadavg: *mut f64, nelem: c_int) -> c_int;
}

// IOKit & AppleSMC C-ABI
type IoConnectT = mach_port_t;
type IoObjectT = mach_port_t;
type IoIteratorT = mach_port_t;
type CfMutableDictionaryRef = *mut c_void;

extern "C" {
    static kIOMainPortDefault: mach_port_t;
    fn mach_task_self() -> mach_port_t;
    fn IOServiceMatching(name: *const c_char) -> CfMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: mach_port_t,
        matching: CfMutableDictionaryRef,
        existing: *mut IoIteratorT,
    ) -> c_int;
    fn IOIteratorNext(iterator: IoIteratorT) -> IoObjectT;
    fn IOObjectRelease(object: IoObjectT) -> c_int;
    fn IOServiceOpen(service: IoObjectT, owningTask: mach_port_t, type_: u32, connect: *mut IoConnectT) -> c_int;
    fn IOServiceClose(connect: IoConnectT) -> c_int;
    fn IOConnectCallStructMethod(
        connection: IoConnectT,
        selector: u32,
        inputStruct: *const c_void,
        inputStructCnt: size_t,
        outputStruct: *mut c_void,
        outputStructCnt: *mut size_t,
    ) -> c_int;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SmcKeyDataVers {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SmcKeyDataLimit {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SmcKeyDataKeyInfo {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SmcKeyData {
    key: u32,
    vers: SmcKeyDataVers,
    p_limit_data: SmcKeyDataLimit,
    key_info: SmcKeyDataKeyInfo,
    padding: u16,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

impl Default for SmcKeyData {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_READ_KEYINFO: u8 = 9;
const SMC_KERNEL_INDEX: u32 = 2;

struct SmcConnection {
    conn: IoConnectT,
}

impl SmcConnection {
    fn open() -> Option<Self> {
        unsafe {
            let matching = IOServiceMatching(b"AppleSMC\0".as_ptr() as *const _);
            if matching.is_null() {
                return None;
            }

            let mut iterator: IoIteratorT = 0;
            if IOServiceGetMatchingServices(kIOMainPortDefault, matching, &mut iterator) != 0 || iterator == 0 {
                return None;
            }

            let device = IOIteratorNext(iterator);
            IOObjectRelease(iterator);
            if device == 0 {
                return None;
            }

            let mut conn: IoConnectT = 0;
            let ret = IOServiceOpen(device, mach_task_self(), 0, &mut conn);
            IOObjectRelease(device);

            if ret == 0 && conn != 0 {
                Some(Self { conn })
            } else {
                None
            }
        }
    }

    fn read_key(&self, key_str: &[u8; 4]) -> Option<f64> {
        let key = u32::from_be_bytes(*key_str);

        // 1. Read key info
        let mut input = SmcKeyData::default();
        let mut output = SmcKeyData::default();
        input.key = key;
        input.data8 = SMC_CMD_READ_KEYINFO;

        let input_size = std::mem::size_of::<SmcKeyData>();
        let mut output_size = input_size;

        let ret = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                SMC_KERNEL_INDEX,
                &input as *const _ as *const c_void,
                input_size,
                &mut output as *mut _ as *mut c_void,
                &mut output_size,
            )
        };

        if ret != 0 || output.result != 0 {
            return None;
        }

        let key_info = output.key_info;
        if key_info.data_size == 0 || key_info.data_size > 32 {
            return None;
        }

        // 2. Read key bytes
        input.key_info.data_size = key_info.data_size;
        input.data8 = SMC_CMD_READ_BYTES;
        output = SmcKeyData::default();
        output_size = input_size;

        let ret = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                SMC_KERNEL_INDEX,
                &input as *const _ as *const c_void,
                input_size,
                &mut output as *mut _ as *mut c_void,
                &mut output_size,
            )
        };

        if ret != 0 || output.result != 0 {
            return None;
        }

        // 3. Unpack value based on data type
        let dtype = key_info.data_type.to_be_bytes();
        let bytes = output.bytes;

        match &dtype {
            b"sp78" => {
                let v = ((bytes[0] as i16) << 8) | (bytes[1] as i16);
                Some(v as f64 / 256.0)
            }
            b"fpe2" => {
                let v = ((bytes[0] as u16) << 8) | (bytes[1] as u16);
                Some(v as f64 / 4.0)
            }
            b"flt " if key_info.data_size == 4 => {
                let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
                Some(f32::from_ne_bytes(arr) as f64)
            }
            b"ui8 " | b"ui8\0" => Some(bytes[0] as f64),
            b"ui16" => {
                let v = ((bytes[0] as u16) << 8) | (bytes[1] as u16);
                Some(v as f64)
            }
            b"ui32" => {
                let v = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                Some(v as f64)
            }
            _ => None,
        }
    }
}

impl Drop for SmcConnection {
    fn drop(&mut self) {
        if self.conn != 0 {
            unsafe {
                IOServiceClose(self.conn);
            }
        }
    }
}

static SMC_CONN: Mutex<Option<SmcConnection>> = Mutex::new(None);

pub fn poll_smc(out: &mut StatsSmcTelemetry) -> bool {
    let mut guard = match SMC_CONN.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };

    if guard.is_none() {
        *guard = SmcConnection::open();
    }

    let conn = match guard.as_ref() {
        Some(c) => c,
        None => return false,
    };

    // Read fans
    if let Some(fnum) = conn.read_key(b"FNum") {
        let fan_count = (fnum as usize).min(STATS_CORE_MAX_FANS);
        out.fan_count = fan_count as u32;

        let cur_keys = [b"F0Ac", b"F1Ac", b"F2Ac", b"F3Ac"];
        let min_keys = [b"F0Mn", b"F1Mn", b"F2Mn", b"F3Mn"];
        let max_keys = [b"F0Mx", b"F1Mx", b"F2Mx", b"F3Mx"];

        for i in 0..fan_count {
            let cur = conn.read_key(&cur_keys[i]).unwrap_or(0.0);
            let min = conn.read_key(&min_keys[i]).unwrap_or(0.0);
            let max = conn.read_key(&max_keys[i]).unwrap_or(0.0);

            out.fans[i] = StatsFanInfo {
                id: i as u32,
                current_rpm: cur,
                min_rpm: min,
                max_rpm: max,
            };
        }
    }

    // Read CPU temperatures (try known Apple Silicon and Intel SMC keys)
    let cpu_keys = [b"Tp09", b"Tp0T", b"Tp1h", b"Tp1p", b"TC0P", b"TC0D", b"TC0E", b"TC0F"];
    let mut cpu_temps = 0.0;
    let mut cpu_temp_count = 0;

    for k in &cpu_keys {
        if let Some(val) = conn.read_key(k) {
            if val > 0.0 && val < 125.0 {
                cpu_temps += val;
                cpu_temp_count += 1;
            }
        }
    }
    if cpu_temp_count > 0 {
        out.cpu_temp = cpu_temps / (cpu_temp_count as f64);
    }

    // Read GPU temperatures
    let gpu_keys = [b"Tg05", b"Tg0D", b"TG0P", b"TG0D"];
    let mut gpu_temps = 0.0;
    let mut gpu_temp_count = 0;

    for k in &gpu_keys {
        if let Some(val) = conn.read_key(k) {
            if val > 0.0 && val < 125.0 {
                gpu_temps += val;
                gpu_temp_count += 1;
            }
        }
    }
    if gpu_temp_count > 0 {
        out.gpu_temp = gpu_temps / (gpu_temp_count as f64);
    }

    true
}

pub fn get_load_average(out: &mut StatsLoadAvg) -> bool {
    let mut loads = [0.0f64; 3];
    let ret = unsafe { getloadavg(loads.as_mut_ptr(), 3) };
    if ret == 3 {
        out.one_min = loads[0];
        out.five_min = loads[1];
        out.fifteen_min = loads[2];
        true
    } else {
        false
    }
}
