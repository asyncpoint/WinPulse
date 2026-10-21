use sysinfo::Disks;

#[derive(Debug, Default, Clone, Copy)]
pub struct DiskMetrics {
    pub read_bytes_per_sec: u64,
    pub written_bytes_per_sec: u64,
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub used_percent: f32,
}

/// Aggregate activity and capacity across detected disks. sysinfo's disk usage
/// counters are interval deltas after refresh, so they are used directly as rates.
pub fn sample(disks: &mut Disks) -> DiskMetrics {
    disks.refresh(true);
    let mut result = DiskMetrics::default();
    let mut available = 0u64;
    for disk in disks.list() {
        let usage = disk.usage();
        result.read_bytes_per_sec = result.read_bytes_per_sec.saturating_add(usage.read_bytes);
        result.written_bytes_per_sec = result
            .written_bytes_per_sec
            .saturating_add(usage.written_bytes);
        result.total_bytes = result.total_bytes.saturating_add(disk.total_space());
        available = available.saturating_add(disk.available_space());
    }
    result.used_bytes = result.total_bytes.saturating_sub(available);
    if result.total_bytes > 0 {
        result.used_percent =
            (result.used_bytes as f64 * 100.0 / result.total_bytes as f64).clamp(0.0, 100.0) as f32;
    }
    result
}
