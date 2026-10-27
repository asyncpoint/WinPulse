# WinPulse

WinPulse is a lightweight native Windows floating system-resource widget written in Rust with a Win32/GDI UI. It stays above other windows and can be dragged around the desktop.

## Features

- **Exactly two layouts:** `Horizontal bars` and `Vertical Bars`.
- CPU and physical RAM utilization, refreshed about once per second.
- Network receive and transmit rates with adaptive, smoothed bar scales and always-visible numeric units (`B/s`, `KB/s`, `MB/s`). A nonzero rate gets a visible minimum fill.
- Independent **Disk Read** and **Disk Write** throughput rows.
- Optional aggregate disk-space usage (off by default); its label includes the first detected volume mount point (for example, `SPACE C:`).
- GPU row shown by default; displays `N/A` when no validated provider is available rather than inventing a value.
- Background-only transparency: right-click **Background opacity** and choose 20%, 40%, 60%, 80%, or 100%. Text and bars remain opaque for readability.
- Right-click controls for layout, GPU visibility, disk-space visibility, background opacity, and exit.
- Window repositioning is clamped to the nearest monitor's work area, excluding the taskbar. Display/DPI changes and layout switches trigger a bounds check.
- Layout, opacity, and visibility preferences are saved to `%APPDATA%\WinPulse\preferences.ini`.
- Size-focused release profile (LTO, one codegen unit, stripping, and `opt-level = "z"`).

## Layouts

### Horizontal bars (default)

```text
CPU       [████████░░░░░░░░░░]              42%
RAM       [████████████░░░░░░]              68%
NET IN    [████░░░░░░░░░░░░░░]          2 MB/s
NET OUT   [█░░░░░░░░░░░░░░░░░]        320 KB/s
DISK R    [██░░░░░░░░░░░░░░░░]         12 MB/s
DISK W    [█░░░░░░░░░░░░░░░░░]          4 MB/s
```

### Vertical Bars

CPU and RAM are shown as vertical meters, with network and disk throughput in separate adaptive horizontal rows underneath.

## Interaction

- **Left-click and drag:** move the widget. When the drag ends, its position is corrected to remain within the nearest monitor work area.
- **Right-click:** open the menu.
- **Horizontal bars / Vertical Bars:** change layout.
- **Show GPU:** toggle the GPU row. It reports `N/A` while no validated provider is available.
- **Show disk space:** toggle aggregate used-space percentage across detected disks; the row label identifies the first detected volume mount point.
- **Background opacity:** set background opacity from 20% to 100%. Only exact background pixels are made translucent; foreground text and bars are rendered opaque. This uses a layered-window alpha surface rather than whole-window alpha.
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
- **Transparency rendering:** the background uses per-pixel alpha while foreground pixels remain opaque. GDI text antialiasing can blend edge pixels with the background, so very low opacity may slightly affect edge pixels; use 60–100% for maximum readability.

## Build from source

Requirements: Windows 10 or later, Rust stable, and the Windows SDK / appropriate Visual Studio C++ build tools for the selected Windows target.

```powershell
cargo test
cargo build --release
```

The application uses `sysinfo` for network and disk counters and `windows-sys` for native Win32 calls. GPU metrics remain explicitly unavailable until a real provider is implemented and validated.
