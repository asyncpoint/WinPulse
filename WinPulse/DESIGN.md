# WinPulse design notes

WinPulse is a small native Windows resource widget, not a full-screen dashboard. The design keeps the window compact, topmost, draggable, and context-menu driven.

## Display modes

Only **Horizontal bars** is supported; the vertical layout has been removed. CPU and RAM use semantic utilization colors (green below 70%, yellow from 70% to below 90%, red at 90% or above). Network and disk rates use a neutral activity color because throughput is not itself utilization.

## Throughput visualization

CPU and RAM are the only metrics rendered as bars. Network throughput is one compact row showing received (↓) and sent (↑) rates as direct values. Disk read (↓) and write (↑) values on one row each have their own direct-value row. Rates use B/s, KB/s, or MB/s; no throughput bars or adaptive bar scales are used.

## Window placement

The initial location is near the bottom-right of the primary monitor's work area. On display or DPI changes, after layout changes, and when dragging ends, the widget is clamped to the work area of the nearest monitor. The work area excludes the taskbar. Mixed-DPI and hot-plug behavior must still be verified on real Windows hardware.

## Background opacity

The widget uses the Win32 layered-window API `SetLayeredWindowAttributes` with `LWA_ALPHA`. This makes the selected opacity (20–100%) visibly effective and updates immediately. The Win32 constant-alpha path applies to the entire widget, including labels and bars; this is a deliberate reliability trade-off from the previous per-pixel implementation, which did not visibly respond on the user’s system.

## Preferences

The opacity, GPU visibility, and disk-space visibility are persisted in `%APPDATA%\WinPulse\preferences.ini`. Invalid opacity values are clamped to the supported range. The file is best-effort: if the application-data directory cannot be written, the widget still runs with defaults.

## Known limitations

- GPU utilization is deliberately `N/A` until a validated provider is available.
- Disk-space totals are aggregated, not selectable per drive.
- Native rendering and mixed-DPI placement require runtime validation on Windows; the current development environment did not provide the Rust toolchain needed to compile or execute this revision.
