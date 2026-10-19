# WinPulse

WinPulse is a lightweight, native Windows desktop widget that displays system-wide CPU utilization and physical-memory load. It is written in Rust and uses Win32 APIs and GDI directly, without a browser runtime or heavyweight UI framework.

> **Current scope:** WinPulse is a floating, always-on-top window, not a native Windows taskbar toolbar or extension.

## Features

- Three selectable layouts: horizontal floating bar, vertical meters, and stacked horizontal bars.
- CPU and RAM readings refreshed once per second.
- Shared semantic usage colors across resources and layouts.
- Borderless, always-on-top window that can be repositioned by dragging.
- Right-click menu to choose a layout or exit.
- Size-optimized release profile with LTO and stripped symbols.

## UI preview

The image below is an illustrative design mockup, not a screenshot of a running build. Some promotional details in the artwork may not reflect the current implementation: WinPulse currently displays **CPU and RAM only**, and its actual context menu contains the three layout choices and **Exit WinPulse**.

![WinPulse UI showcase: horizontal, vertical-meter, and stacked-bar layouts with semantic usage colors](assets/winpulse-ui-showcase.png)

## Layouts

### 1. Horizontal floating bar (default)

A compact two-row widget. Each row has a resource label and a horizontal usage bar.

```text
+----------------------------------+
| CPU  [██████████░░░░░░]           |
| RAM  [████████████░░░░]           |
+----------------------------------+
```

### 2. Vertical meters

Two vertical meters sit side by side. Fill rises from the bottom; percentages appear underneath.

```text
+------------------+
|   CPU      RAM   |
|   ░░██      ██   |
|   ░░██      ██   |
|   ████      ██   |
|   ████     ████  |
|   42%       68%  |
+------------------+
```

### 3. Stacked horizontal bars

Two longer horizontal indicators include numeric percentages at the right.

```text
+----------------------------------+
| CPU  [████████░░░░░]  42%        |
| RAM  [████████████░]  68%        |
+----------------------------------+
```

These ASCII diagrams are wireframes; fill lengths and colors depend on live readings.

## Usage color system

Color communicates utilization severity; labels identify the resource.

| Utilization | Color | Meaning |
|---|---|---|
| 0–60% inclusive | Green `#22C55E` | Normal |
| Above 60% and below 90% | Yellow `#EAB308` | Elevated |
| 90–100% inclusive | Red `#EF4444` | High |

CPU and RAM change color independently. High usage is an attention cue, not automatically a fault.

## Interaction

- **Left-click and drag:** move the widget.
- **Right-click:** open the context menu.
- **Choose a layout:** switch to the selected layout; the active layout is checked.
- **Exit WinPulse:** close the application.

The window opens near the bottom-right of the primary display's work area. Position and layout are not persisted between launches.

## Metrics and limitations

- **CPU:** system-wide utilization calculated from deltas between successful `GetSystemTimes` samples. The first successful sample establishes the baseline.
- **RAM:** Windows-reported physical-memory load from `GlobalMemoryStatusEx` (`dwMemoryLoad`), shown as a percentage.
- **Refresh interval:** one second.
- The app does not show per-process metrics, historical graphs, exact used/total RAM, alerts, or telemetry.
- High-DPI behavior and multi-monitor placement have not yet been fully implemented or validated.

## Requirements

- Windows 10 or later (recommended).
- Rust stable with the `x86_64-pc-windows-msvc` target.
- Visual Studio C++ build tools and the Windows SDK required by the Rust MSVC toolchain.

## Build from source

From the repository root in a PowerShell terminal configured with the Windows MSVC build tools:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
rustup target add x86_64-pc-windows-msvc
cargo fmt --all -- --check
cargo check --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

The executable is created at:

```text
target/x86_64-pc-windows-msvc/release/winpulse.exe
```

The release profile enables size optimization (`opt-level = "z"`), link-time optimization (`lto = true`), one code-generation unit, panic-abort behavior, and symbol stripping. Actual executable size and runtime resource use depend on the toolchain and must be measured. Cargo will resolve dependencies during the first build; for reproducible repository builds, generate and commit `Cargo.lock` from the Windows build environment before cutting a release.

## GitHub Actions and releases

The workflow in `.github/workflows/windows-build.yml` checks formatting, runs a release-mode compile check, and builds the optimized Windows x64 executable. After creating the GitHub repository, add a workflow badge using that repository’s actual URL if desired.

- On pushes and pull requests, inspect the **Actions** run for validation results.
- On a manual workflow run or normal branch push, download the `WinPulse-windows-x64` artifact from the workflow run.
- To publish a GitHub Release, push a version tag such as `v1.0.0`. The workflow attaches `WinPulse-windows-x64.exe` to that release and generates release notes.

## Project structure

```text
WinPulse/
├── .github/
│   └── workflows/
│       └── windows-build.yml
├── assets/
│   └── winpulse-ui-showcase.png
├── src/
│   └── main.rs
├── .gitignore
├── Cargo.toml
├── DESIGN.md
├── LICENSE
└── README.md
```

## Contributing and reporting issues

Please open a GitHub issue with your Windows version, Rust toolchain version, reproduction steps, and relevant error output. Avoid including private system information in screenshots or logs. Pull requests should pass `cargo fmt --all -- --check` and the Windows x64 workflow.

## License

WinPulse is distributed under the MIT License. See [LICENSE](LICENSE).
