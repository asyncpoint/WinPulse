# WinPulse

WinPulse is a lightweight native Windows floating system-resource widget written in Rust with a Win32/GDI UI. It stays above other windows and can be dragged around the desktop.

## Features

- **One compact layout:** `Horizontal bars`.
- CPU and physical RAM utilization, refreshed about once per second.
- A single **NETWORK** row shows received (↓) and sent (↑) throughput as direct values with units (`B/s`, `KB/s`, `MB/s`), without bars.
- A single **DISK** row shows read (↓) and write (↑) throughput as direct values (`B/s`, `KB/s`, `MB/s`), without bars.
- Optional aggregate disk-space usage (off by default); its label includes the first detected volume mount point (for example, `SPACE C:`).
- Optional GPU row (off by default); displays `N/A` when no validated provider is available rather than inventing a value.
- Working live opacity control: right-click **Widget opacity** and choose 20%, 40%, 60%, 80%, or 100%. Win32 layered-window alpha currently applies to the entire widget, including text and bars.
- Right-click controls for GPU visibility, disk-space visibility, opacity, and exit. Horizontal bars is the only layout; there is no unused layout selector.
- Window repositioning is clamped to the nearest monitor's work area, excluding the taskbar. Display/DPI changes and option visibility changes trigger a bounds check and content-sized resize.
- Layout, opacity, and visibility preferences are saved to `%APPDATA%\WinPulse\preferences.ini`.
- Size-focused release profile (LTO, one codegen unit, stripping, and `opt-level = "z"`).

## Layouts

### Horizontal bars (default)

```text
CPU       [████████░░░░░░░░░░]              42%
RAM       [████████████░░░░░░]              68%
NETWORK                         ↓ 2 MB/s   ↑ 320 KB/s
DISK                            ↓ 12 MB/s   ↑ 4 MB/s
```

## Interaction

- **Left-click and drag:** move the widget. When the drag ends, its position is corrected to remain within the nearest monitor work area.
- **Right-click:** open the menu.
- **Horizontal bars:** the only supported layout.
- **Show GPU:** toggle the GPU row. It reports `N/A` while no validated provider is available.
- **Show disk space:** toggle aggregate used-space percentage across detected disks; the row label identifies the first detected volume mount point.
- **Opacity:** set widget opacity from 20% to 100%. Windows applies alpha to the entire layered window; at lower settings, text and bars also become more transparent.
- **Exit WinPulse:** close the application.

Preferences are saved under the current user's roaming application-data directory. The widget initially opens near the bottom-right of the primary monitor's work area; after moving to another monitor, bounds corrections use that monitor's work area.

## Metric details and limitations

- **CPU:** system-wide utilization from `GetSystemTimes` deltas. The first successful sample establishes a baseline.
- **RAM:** Windows physical-memory load from `GlobalMemoryStatusEx`.
- **Network:** receive/transmit byte deltas across detected interfaces. A fast-attack/slow-decay adaptive scale makes low rates visible and adapts to bursts; virtual interfaces may affect totals.
- **Disk activity:** read/write byte deltas from detected disks, refreshed about once per second.
- **Disk space:** optional aggregate used/total capacity. Read/write activity and used-space totals are aggregated across detected disks; the space label shows the first detected volume mount point for orientation, not a per-drive filter. Per-drive selection is not implemented.
- **GPU:** currently `N/A`; a validated Windows GPU Engine counter provider remains future work.
- **DPI and display handling:** placement is clamped to monitor work areas on layout changes, display changes, DPI changes, and after drag operations. Runtime behavior still needs verification on real Windows systems with multiple monitors and mixed scaling factors.
- **Transparency rendering:** uses Win32 layered-window constant alpha. Both the panel and foreground fade together; 60–100% is recommended when readability is more important than seeing content behind the widget.

## Build from source

Requirements: Windows 10 or later, Rust stable, and the Windows SDK / appropriate Visual Studio C++ build tools for the selected Windows target.

```powershell
cargo test
cargo build --release
```

The application uses `sysinfo` for network and disk counters and `windows-sys` for native Win32 calls. GPU metrics remain explicitly unavailable until a real provider is implemented and validated.
