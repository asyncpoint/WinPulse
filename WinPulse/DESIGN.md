# WinPulse design notes

WinPulse is a small native Windows resource widget, not a full-screen dashboard. The design keeps the window compact, topmost, draggable, and context-menu driven.

## Display modes

Only **Horizontal bars** is supported; the vertical layout has been removed. CPU and RAM use semantic utilization colors (green below 70%, yellow from 70% to below 90%, red at 90% or above). Network and disk rates use a neutral activity color because throughput is not itself utilization.

## Throughput visualization

CPU and RAM are the only metrics rendered as bars. Network throughput is one compact row showing received (↓) and sent (↑) rates as direct values. Disk read (↓) and write (↑) values share a single direct-value row. Rates use B/s, KB/s, or MB/s; no throughput bars or adaptive bar scales are used.

## Window placement

The initial location is near the bottom-right of the primary monitor's work area. On display or DPI changes, after layout changes, and when dragging ends, the widget is clamped to the work area of the nearest monitor. The work area excludes the taskbar. Mixed-DPI and hot-plug behavior must still be verified on real Windows hardware.

## Widget opacity

The widget uses the Win32 layered-window API `SetLayeredWindowAttributes` with `LWA_ALPHA`, reasserting `WS_EX_LAYERED` before applying alpha and forcing an immediate redraw. This applies selected opacity (20–100%) to the entire widget. The Win32 constant-alpha path applies to the entire widget, including labels and bars; this is a deliberate reliability trade-off from the previous per-pixel implementation, which did not visibly respond on the user’s system.

## Preferences

The widget height is derived from four permanent rows plus only the optional GPU and disk-space rows currently enabled; toggling either option immediately resizes the window. GPU and disk-space are both disabled by default. The opacity, GPU visibility, and disk-space visibility are persisted in `%APPDATA%\WinPulse\preferences.ini`. Invalid opacity values are clamped to the supported range. The file is best-effort: if the application-data directory cannot be written, the widget still runs with defaults.

## Known limitations

- GPU utilization is deliberately `N/A` until a validated provider is available.
- Disk-space totals are aggregated, not selectable per drive.
- Native rendering and mixed-DPI placement require runtime validation on Windows; the current development environment did not provide the Rust toolchain needed to compile or execute this revision.
