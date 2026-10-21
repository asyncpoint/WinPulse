# WinPulse — Design Document

**Status:** Expanded implementation candidate; Windows compilation and runtime validation remain required.  
**Platform:** Windows 10+ 64-bit  
**Language:** Rust 2024  
**UI:** Win32 windowing and GDI through `windows-sys`

## 1. Purpose

WinPulse is a compact, native, always-on-top resource widget. It avoids a browser runtime and heavyweight UI framework. It remains a floating window rather than a Windows taskbar toolbar.

## 2. Requirements

- Display CPU and RAM utilization.
- Display incoming/outgoing network rates and disk read/write activity by default.
- Make aggregate disk-space usage optional and disabled by default.
- Show GPU utilization when a supported provider can supply it; show `N/A` otherwise.
- Allow GPU visibility to be toggled independently from monitoring capability.
- Use consistent semantic utilization colors: green below 70%, yellow from 70% to below 90%, red from 90% upward.
- Support horizontal, vertical-meter, and stacked layouts.
- Keep size-focused release optimization enabled.

## 3. UI and interaction

Right-click opens a menu with three layouts, `Show GPU`, `Show disk space`, and `Exit WinPulse`. GPU is visible by default and disk space is hidden by default. The GPU label remains visible as `N/A` when shown but no provider is available. Left-click dragging repositions the window.

The horizontal and stacked layouts show CPU, RAM, network receive/transmit, disk read/write, and optional disk-space rows. The vertical layout reserves a dedicated label row above CPU/RAM meters and places network/disk/GPU details below them to avoid the prior label overlap. Layout sizing is fixed per mode; dynamic content may require further DPI and small-window validation.

## 4. Semantic color policy

A single `usage_color` function is used for percentage-based metrics:

- Green `#22C55E`: below 70%.
- Yellow `#EAB308`: 70% to below 90%.
- Red `#EF4444`: 90% and above.

The policy applies to CPU, RAM, and optional disk-space percentage. Network throughput and disk I/O rates use neutral text because a rate alone is not a utilization percentage.

## 5. Monitoring modules

### `monitoring/network.rs`

Refreshes `sysinfo::Networks`, aggregates cumulative received/transmitted byte counters over discovered interfaces, and calculates deltas against the previous sample. The first sample establishes a baseline. Interface aggregation may include virtual adapters, so totals can differ from a single physical adapter's counters.

### `monitoring/disk.rs`

Refreshes discovered disks and aggregates interval read/write byte deltas, total capacity, and available capacity. Disk activity is on by default. Disk-space display is optional and currently aggregates all discovered disks; per-drive selection and exclusion rules are future work.

### `monitoring/gpu.rs`

Defines the provider boundary and returns `None` in this revision. The UI renders `N/A`, never a fabricated `0%`. A production GPU provider still needs to enumerate Windows PDH `GPU Engine` counters, handle wildcard instances and multiple adapters, aggregate usage without double-counting engines, and validate behavior across supported Windows/driver combinations. GPU percentage monitoring must not be described as implemented until this provider exists and is tested.

## 6. CPU and RAM

CPU utilization uses deltas from `GetSystemTimes`; the first valid sample is a baseline. RAM uses `GlobalMemoryStatusEx::dwMemoryLoad`. Failed reads preserve the last valid CPU/RAM sample.

## 7. Refresh policy and overhead

A Win32 timer updates readings approximately once per second. Network and disk rates are derived from counter deltas at that interval. No dedicated busy loop is used. `sysinfo` is added for network and disk enumeration/counters; this adds a dependency and the actual executable size should be measured rather than assumed.

## 8. Architecture

- `main.rs`: window lifecycle, state, CPU/RAM sampling, context menu, drawing, and shared presentation helpers.
- `monitoring/network.rs`: network rates.
- `monitoring/disk.rs`: disk activity and capacity.
- `monitoring/gpu.rs`: optional GPU provider interface and unavailable fallback.

Further separation of Win32 rendering and CPU/memory sampling is possible if the code grows. Each monitor module is independent to support provider replacement and focused testing.

## 9. Known limitations

- GPU counter integration is not implemented yet; the indicator is therefore `N/A` until a provider is added.
- Disk-space monitoring is aggregate only; per-drive choice and custom thresholds are not implemented.
- Preferences are not persisted between runs.
- Network totals may include virtual interfaces.
- GPU multi-engine aggregation, per-drive selection, DPI awareness, multi-monitor placement, and runtime behavior require Windows testing.
- A Cargo lockfile should be generated and committed before release.

## 10. Build and validation

The release profile uses `opt-level = "z"`, LTO, one codegen unit, panic abort, and symbol stripping. Build on Windows with Rust MSVC and run:

```powershell
cargo fmt --all -- --check
cargo check --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

Smoke-test all layouts, context menu toggles, dragging, exit, CPU/RAM changes, network traffic, disk activity, disk-space visibility, and the `GPU N/A` fallback. Measure binary size and resource use on the resulting executable. Do not publish a release until the Windows build succeeds.
