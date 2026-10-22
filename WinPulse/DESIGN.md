# WinPulse design notes

WinPulse is a small native Windows resource widget, not a full-screen dashboard. The design keeps the window compact, topmost, draggable, and context-menu driven.

## Display modes

Only two modes are supported: **Horizontal bars** and **Vertical Bars**. CPU and RAM use semantic utilization colors (green below 70%, yellow from 70% to below 90%, red at 90% or above). Network and disk rates use a neutral activity color because throughput is not itself utilization.

## Throughput visualization

Network receive/transmit and disk read/write are independent rows. Each row prints its rate in B/s, KB/s, or MB/s. Its scale follows a fast-attack / slow-decay peak estimate with a nonzero baseline; any nonzero rate is guaranteed a visible minimum fill. This avoids rates disappearing against a fixed high scale while reducing rapid bar-size flicker.

## Window placement

The initial location is near the bottom-right of the primary monitor's work area. On display or DPI changes, after layout changes, and when dragging ends, the widget is clamped to the work area of the nearest monitor. The work area excludes the taskbar. Mixed-DPI and hot-plug behavior must still be verified on real Windows hardware.

## Background opacity

The widget uses a layered window with a 32-bit alpha surface. Pixels matching the panel background receive the selected alpha (20–100%); foreground pixels remain opaque. This avoids fading the labels and bars along with the panel. GDI text edge pixels can be color-blended with the panel background, so very low opacity may subtly affect antialiased edges.

## Preferences

The selected layout, opacity, GPU visibility, and disk-space visibility are persisted in `%APPDATA%\WinPulse\preferences.ini`. Invalid opacity values are clamped to the supported range. The file is best-effort: if the application-data directory cannot be written, the widget still runs with defaults.

## Known limitations

- GPU utilization is deliberately `N/A` until a validated provider is available.
- Disk-space totals are aggregated, not selectable per drive.
- Native rendering and mixed-DPI placement require runtime validation on Windows; the current development environment did not provide the Rust toolchain needed to compile or execute this revision.
