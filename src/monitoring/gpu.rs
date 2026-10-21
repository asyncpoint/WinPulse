/// GPU utilization is optional because Windows GPU performance counters depend
/// on the OS build, driver, and GPU engine availability. `None` means unavailable
/// and must be rendered as `N/A`; it must never be interpreted as 0% utilization.
///
/// TODO: add a PDH `GPU Engine` provider with wildcard instance enumeration and
/// per-adapter aggregation. Keep this interface stable for that provider.
pub fn sample() -> Option<f32> {
    None
}
