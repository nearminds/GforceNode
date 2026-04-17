//! System information collection.

use serde_json::json;
use sysinfo::System;

/// Collect system information for the initial auth message.
pub fn collect_system_info() -> serde_json::Value {
    let hostname = System::host_name().unwrap_or_else(|| "unknown".into());
    let os = std::env::consts::OS; // linux, macos, windows
    let arch = std::env::consts::ARCH; // x86_64, aarch64

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_cores = sys.cpus().len();
    let memory_total_gb = sys.total_memory() as f64 / 1_073_741_824.0;
    let disk_total_gb: f64 = sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .map(|d| d.total_space() as f64 / 1_073_741_824.0)
        .sum();

    json!({
        "hostname": hostname,
        "os": os,
        "arch": arch,
        "cpu_cores": cpu_cores,
        "memory_total_gb": (memory_total_gb * 10.0).round() / 10.0,
        "disk_total_gb": (disk_total_gb * 10.0).round() / 10.0,
    })
}

/// Collect current system metrics for heartbeat.
pub fn collect_metrics() -> (f32, f32, f32) {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_percent = sys.global_cpu_usage();
    let memory_percent = if sys.total_memory() > 0 {
        (sys.used_memory() as f64 / sys.total_memory() as f64 * 100.0) as f32
    } else {
        0.0
    };

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let disk_free_gb: f64 = disks
        .iter()
        .map(|d| d.available_space() as f64 / 1_073_741_824.0)
        .sum();

    (cpu_percent, memory_percent, disk_free_gb as f32)
}
