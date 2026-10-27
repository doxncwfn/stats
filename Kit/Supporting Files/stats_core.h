#ifndef STATS_CORE_H
#define STATS_CORE_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#define STATS_CORE_MAX_CORES 64
#define STATS_CORE_MAX_TOP_PROCESSES 48
#define STATS_CORE_MAX_PROCESS_NAME 64
#define STATS_CORE_MAX_DISKS 16
#define STATS_CORE_MAX_NET_INTERFACES 16
#define STATS_CORE_MAX_FANS 4
#define STATS_CORE_MAX_SENSORS 32

// CPU Metrics
typedef struct {
    double total_usage;        // 0.0 to 1.0 (e.g. 0.15 = 15%)
    double system_load;        // 0.0 to 1.0
    double user_load;          // 0.0 to 1.0
    double idle_load;          // 0.0 to 1.0
    uint32_t core_count;
    double usage_per_core[STATS_CORE_MAX_CORES];
} StatsCpuLoad;

// RAM Metrics
typedef struct {
    uint64_t total;            // total physical RAM in bytes
    uint64_t used;             // active + inactive + wired + compressed - purgeable - external
    uint64_t free;             // total - used
    uint64_t active;
    uint64_t inactive;
    uint64_t wired;
    uint64_t compressed;
    uint64_t app;              // used - wired - compressed
    uint64_t cache;            // purgeable + external
    uint64_t swap_total;
    uint64_t swap_used;
    uint64_t swap_free;
    int64_t  swapins;
    int64_t  swapouts;
    uint32_t pressure_level;   // 1 = normal, 2 = warning, 4 = critical
} StatsRamUsage;

// Process Information
typedef struct {
    int32_t pid;
    double cpu_usage;          // 0.0 to 100.0+ (percent)
    uint64_t ram_bytes;        // resident memory in bytes
    char name[STATS_CORE_MAX_PROCESS_NAME]; // null-terminated C string
} StatsProcessInfo;

typedef struct {
    uint32_t count;
    StatsProcessInfo processes[STATS_CORE_MAX_TOP_PROCESSES];
} StatsTopProcesses;

// Disk Metrics
typedef struct {
    char bsd_name[32];         // e.g. "disk3s1"
    char mount_point[64];      // e.g. "/" or "/Volumes/External"
    char fs_type[16];          // e.g. "apfs"
    uint64_t total_bytes;
    uint64_t free_bytes;
    uint64_t read_bytes_per_sec;
    uint64_t write_bytes_per_sec;
} StatsDiskInfo;

typedef struct {
    uint32_t count;
    uint64_t total_read_rate;   // aggregate bytes/sec
    uint64_t total_write_rate;  // aggregate bytes/sec
    StatsDiskInfo disks[STATS_CORE_MAX_DISKS];
} StatsDiskTelemetry;

// Network Metrics
typedef struct {
    char name[16];             // e.g. "en0", "bridge0", "utun2"
    uint64_t upload_bytes_per_sec;
    uint64_t download_bytes_per_sec;
    uint64_t total_upload_bytes;
    uint64_t total_download_bytes;
    bool is_up;
} StatsInterfaceInfo;

typedef struct {
    uint32_t count;
    uint64_t total_upload_rate;   // aggregate bytes/sec
    uint64_t total_download_rate; // aggregate bytes/sec
    StatsInterfaceInfo interfaces[STATS_CORE_MAX_NET_INTERFACES];
} StatsNetworkTelemetry;

// SMC & Sensor Metrics
typedef struct {
    uint32_t id;
    double current_rpm;
    double min_rpm;
    double max_rpm;
} StatsFanInfo;

typedef struct {
    char key[8];               // 4-char SMC key, null-terminated
    double value;
} StatsSensorInfo;

typedef struct {
    double cpu_temp;           // in Celsius
    double gpu_temp;           // in Celsius
    uint32_t fan_count;
    StatsFanInfo fans[STATS_CORE_MAX_FANS];
    uint32_t sensor_count;
    StatsSensorInfo sensors[STATS_CORE_MAX_SENSORS];
} StatsSmcTelemetry;

// Load Average Metrics
typedef struct {
    double one_min;
    double five_min;
    double fifteen_min;
} StatsLoadAvg;

// Lifecycle & Telemetry APIs
bool stats_core_init(void);

// CPU polling
bool stats_core_poll_cpu(StatsCpuLoad *out_cpu);

// RAM polling
bool stats_core_poll_ram(StatsRamUsage *out_ram);

// Process polling (zero allocations)
bool stats_core_top_cpu_processes(uint32_t limit, StatsTopProcesses *out_procs);
bool stats_core_top_ram_processes(uint32_t limit, StatsTopProcesses *out_procs);
bool stats_core_top_energy_processes(uint32_t limit, StatsTopProcesses *out_procs);

// Disk polling (getfsstat & IOKit storage driver counters)
bool stats_core_poll_disks(StatsDiskTelemetry *out_disks);

// Network polling (getifaddrs if_data64 counters)
bool stats_core_poll_network(StatsNetworkTelemetry *out_network);

// SMC & Sensor polling
bool stats_core_poll_smc(StatsSmcTelemetry *out_smc);

// Direct kernel load average (replaces uptime subprocess)
bool stats_core_get_load_average(StatsLoadAvg *out_load);

#ifdef __cplusplus
}
#endif

#endif /* STATS_CORE_H */
