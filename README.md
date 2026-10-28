# WinPulse

WinPulse is a compact native Windows resource monitor written in Rust using Win32/GDI. It stays above other windows, can be dragged, and uses a right-click menu for options.

## Features

- CPU and physical RAM usage shown as horizontal bars. Usage colors are green below 70%, yellow from 70% to below 90%, and red at 90% or higher.
- Optional text-only **NETWORK** row (enabled by default): `↓ received  ↑ sent` rates. Down arrow is inbound/received; up arrow is outbound/sent.
- Optional text-only **DISK** row (enabled by default): `↓ read  ↑ write` rates. Down arrow is read; up arrow is write.
- Throughput units are B/s, KB/s, or MB/s.
- Optional GPU row is disabled by default; it reports `N/A` when no validated provider is available.
- Optional disk-space row is disabled by default and labels the first detected mount point (for example `SPACE C:`). Space usage is aggregate across detected disks.
- Right-click controls to enable/disable network traffic and disk read/write rows, optional GPU/disk-space rows, background opacity (20%, 40%, 60%, 80%, 100%), and exit.
- Background-only per-pixel transparency: panel background opacity changes while foreground text and bars remain opaque.
- Window height is calculated from visible rows at startup and whenever network, disk I/O, GPU, or disk-space rows are toggled. Window placement is clamped to the nearest monitor work area.
- Preferences (including network and disk I/O visibility) are stored in `%APPDATA%\WinPulse\preferences.ini`.

## Default view

```text
CPU       [████████░░░░░░░░░░]              42%
RAM       [████████████░░░░░░]              68%
NETWORK                         ↓ 427 KB/s   ↑ 24 KB/s
DISK                            ↓ 640 KB/s   ↑ 80 KB/s
```

## Interaction

- **Left-click and drag:** move the widget. It is clamped to the nearest monitor's work area when the drag ends.
- **Right-click:** open the menu.
- **Show network traffic:** toggle the NETWORK receive/transmit row.
- **Show disk read/write:** toggle the DISK read/write row.
- **Show GPU:** toggle the optional GPU row; unavailable metrics display `N/A`.
- **Show disk space:** toggle the optional disk capacity row.
- **Background opacity:** change panel opacity without deliberately fading foreground text and bars.
- **Exit WinPulse:** close the application.

## Metric details and limitations

- CPU uses deltas from Windows `GetSystemTimes`; the first sample establishes a baseline.
- RAM uses `GlobalMemoryStatusEx`.
- Network rates use deltas of cumulative interface receive/transmit counters. Virtual adapters can affect totals.
- Disk read/write rates use `sysinfo` disk usage counters refreshed about once per second. These are system-provider interval counters, not guaranteed hardware-level performance counters.
- Disk-space usage aggregates detected disks; the displayed mount point is for orientation and is not a per-drive filter.
- GPU usage remains `N/A` until a validated provider is implemented.
- The layered-window path uses per-pixel alpha. Native rendering, antialiasing, and mixed-DPI behavior should still be verified on a real Windows desktop.

## Build on Windows

Requirements: stable Rust, Windows 10 or later, and the Visual Studio C++ build tools / Windows SDK for the MSVC target.

```powershell
cargo fmt --all -- --check
cargo test
cargo check --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

GitHub Actions runs formatting, tests, release checking, and a Windows x64 release build. It uploads `WinPulse-windows-x64.exe` as an artifact.
