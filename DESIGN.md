# WinPulse design notes

WinPulse is a small topmost Windows resource widget, not a full-screen dashboard. The only supported layout is horizontal. CPU and RAM are utilization bars; network and disk I/O are compact text rows.

## Rows and defaults

The default view has four rows: CPU, RAM, NETWORK, and DISK. GPU and disk-space rows are opt-in and disabled by default. NETWORK shows receive (↓) and transmit (↑) rates; DISK shows read (↓) and write (↑) rates. Throughput is never rendered as bars.

## Sizing and placement

The window height is computed from the visible row count at startup and after each optional-row toggle. The widget is clamped to the work area of the nearest monitor, excluding the taskbar. DPI/display change messages trigger a bounds check. Mixed-DPI and monitor hot-plug behavior still needs runtime verification on Windows.

## Transparency

The renderer paints to a 32-bit DIB and calls `UpdateLayeredWindow` with per-pixel alpha. Exact panel-background pixels receive the selected opacity, with RGB premultiplied for `AC_SRC_ALPHA`; foreground pixels remain opaque. The DIB and `COLORREF` channel order must be handled correctly (red/blue are ordered differently). GDI text antialiasing can blend edge pixels with the background.

## Preferences

Opacity, GPU visibility, and disk-space visibility are saved to `%APPDATA%\WinPulse\preferences.ini`. Fresh defaults disable both optional rows. Legacy `mode=vertical` preferences are ignored because only horizontal layout exists.

## Limitations

- GPU utilization is `N/A` until a validated provider is implemented.
- Disk space is aggregated; per-drive selection is not implemented.
- `sysinfo` disk counters are provider interval counters and may not correspond to exact one-second hardware rates.
- Windows runtime validation is required for transparency, drawing, and mixed-DPI placement.
