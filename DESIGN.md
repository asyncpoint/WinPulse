# WinPulse — Design Document

**Status:** Implementation design for the current source revision  
**Application type:** Native Windows floating system-resource widget  
**Language:** Rust 2024  
**UI technology:** Win32 windowing and GDI through `windows-sys`

## 1. Purpose

WinPulse provides an unobtrusive, always-on-top view of system-wide CPU utilization and physical-memory load. It is intentionally small: the application avoids a heavyweight GUI framework, browser runtime, background service, and continuous busy loop.

The widget is a floating window positioned near the primary display's work-area corner. It is not currently a taskbar toolbar or a taskbar extension.

## 2. Design goals

1. Keep idle CPU and memory overhead low.
2. Use native Windows APIs and avoid unnecessary runtime dependencies.
3. Refresh resource readings at a modest, predictable interval.
4. Offer multiple compact layouts without separate windows or application restarts.
5. Keep the interaction discoverable through a context menu.
6. Make metric calculations and resource-management behavior straightforward to audit.
7. Prefer readable values and labeled indicators over color-only communication.

## 3. Non-goals for this revision

- Per-process CPU/RAM monitoring.
- Historical graphs, alerts, logging, or telemetry upload.
- Native taskbar toolbar integration.
- Saving preferences or restoring window position.
- Full multi-monitor/DPI policy, themes, and user-configurable refresh rates.
- Installer, auto-start integration, or background Windows service.

## 4. UI layouts

The application supports three layouts. The context menu selects a specific layout, rather than cycling through modes, so users can choose either design directly.

### 4.1 Horizontal floating bar (default)

- Approximate client size: 270 × 72 logical pixels.
- Two rows: CPU and RAM.
- Each row has a short label and a horizontal bar.
- Bar tracks remain visible at 0% utilization; colored fill shows the current value.
- CPU and RAM both use a shared semantic color policy: green through 60%, yellow above 60% and below 90%, and red from 90% upward. Labels identify the resource; color communicates utilization severity.

### 4.2 Vertical meters

- Approximate client size: 112 × 174 logical pixels.
- Two vertical tracks side by side.
- Fill rises from the bottom of each track.
- CPU and RAM labels appear above the meters; percentages appear below.
- Intended for a narrow desktop edge or corner.

### 4.3 Stacked horizontal bars

- Approximate client size: 260 × 104 logical pixels.
- Two horizontally filled rows with labels and numeric percentages.
- Numeric values sit at the right edge, favoring readability over the smallest footprint.

All layouts share the same metrics and semantic usage-color thresholds. CPU and RAM are colored independently based on their current readings. Layout changes resize the existing window without resetting the metrics or creating another window.

## 4.4 Semantic usage colors

All resource indicators use the same thresholds regardless of resource type:

- Green (`#22C55E`): 0–60%, inclusive.
- Yellow (`#EAB308`): greater than 60% and less than 90%.
- Red (`#EF4444`): 90–100%, inclusive.

The shared `usage_color` function applies this policy consistently to labels, bars/meters, and percentage text. CPU and RAM change independently. The neutral background track remains constant. High utilization is an attention cue, not proof of a fault.

## 5. Interaction model

| Input | Behavior |
|---|---|
| Left-click and drag | Move the borderless widget using the native non-client drag behavior |
| Right-click | Open a context menu |
| Select a layout | Change layout and resize the window |
| Exit WinPulse | Destroy the window and stop the message loop |
| Timer tick | Refresh metrics and invalidate the client area |

The selected layout is checked in the context menu. The implemented menu contains the three layout choices and Exit. The window uses `WS_EX_TOPMOST` to remain above normal windows and `WS_EX_TOOLWINDOW` to avoid a regular taskbar button. There is no menu item for toggling topmost behavior.

## 6. Metrics and sampling

### 6.1 CPU

`GetSystemTimes` supplies cumulative idle, kernel, and user time counters. The first successful read is stored as the baseline. Each later reading computes counter deltas:

`total = Δkernel + Δuser`

`busy = total − Δidle`

`CPU % = 100 × busy / total`

The result is updated only when the total delta is positive and the idle delta does not exceed the total. The displayed result is clamped to 0–100%. Saturating subtraction avoids underflow if counters unexpectedly move backwards.

### 6.2 Memory

`GlobalMemoryStatusEx` supplies `dwMemoryLoad`, the Windows-reported percentage of physical memory in use. The value is clamped to 0–100%. The UI does not currently show exact used/total memory sizes.

### 6.3 Refresh policy

A Win32 timer fires every 1,000 milliseconds. It updates metrics and invalidates the window for repaint. There is no explicit polling thread or busy loop.

## 7. Architecture

The current implementation remains in `src/main.rs` to keep the project easy to build and distribute. Its responsibilities are separated into functions:

- **Application startup:** register the window class, create the initial state and window, and run the message loop.
- **Window procedure:** route timer, mouse, menu, paint, and destroy messages.
- **Application state:** hold the selected layout, latest metric values, and CPU sampling baseline. The state is owned by a `Box` and attached to the window through `GWLP_USERDATA`.
- **Metrics:** `update_metrics` samples system APIs and validates CPU deltas.
- **Rendering:** each layout has its own renderer. Shared helpers draw tracks, fills, labels, and percentages.
- **Context menu:** users can select any of the three layouts or exit.

A future change can split these responsibilities into modules if configuration or more metrics make the single source file difficult to maintain. That split is not necessary solely for this revision.

## 8. Rendering and resource management

- GDI drawing uses a compatible memory DC and bitmap for buffered painting.
- The background is painted by the application; `WM_ERASEBKGND` reports handled to reduce redundant erasure.
- Brushes, the bitmap, and the memory DC are released after painting.
- GDI creation failures are checked before the relevant handles are used; painting can fall back to direct background fill if the memory DC cannot be created.
- `COLORREF` values are composed with red in the least-significant byte, green in the next byte, and blue in the third byte.

## 9. Failure handling

Startup stops if the module handle, window class registration, or window creation fails. The timer is checked; if it cannot be created, the window is destroyed and the application exits. Failed metric API calls leave the last valid reading in place rather than replacing it with fabricated values.

Further hardening opportunities include diagnostics for startup failures, a small testable CPU-delta helper, and explicit fallback handling for all individual GDI object creation failures.

## 10. Build and release

The crate uses Rust 2024 and `windows-sys` 0.59 with Win32 Foundation, System Information, Threading, Library Loader, Windows and Messaging, and GDI feature flags. The release profile favors a small binary through size optimization (`opt-level = "z"`), link-time optimization (`lto = true`), LTO, a single codegen unit, panic abort, and symbol stripping.

The GitHub Actions workflow checks formatting, performs a release-mode compile check, builds the optimized x64 executable, uploads a workflow artifact, and attaches the executable to GitHub Releases for version tags (`v*`). Before publishing a release, build and test on Windows with the intended Rust/MSVC toolchain and commit the generated `Cargo.lock` so dependency resolution is reproducible. Do not promise a specific executable size or memory footprint without measuring the produced artifact and running it on representative systems.

## 11. Validation checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo check --release` on Windows MSVC
- [ ] `cargo build --release` on Windows MSVC
- [ ] Verify all three menu options and the checked state
- [ ] Verify left-click dragging and Exit behavior
- [ ] Verify CPU baseline and changing CPU load over multiple samples
- [ ] Verify RAM reading against Windows Task Manager
- [ ] Leave the widget running for several hours and inspect CPU, memory, and GDI object counts
- [ ] Test different resolutions and scaling settings; record remaining DPI/multi-monitor limitations
- [ ] Measure release executable size instead of relying on an estimated figure
