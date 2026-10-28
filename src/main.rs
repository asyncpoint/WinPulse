#![windows_subsystem = "windows"]

use std::mem::{size_of, zeroed};
use std::ptr;
use std::path::PathBuf;
use std::fs;
use sysinfo::{Disks, Networks, System};
mod monitoring;
use monitoring::{disk, gpu, network};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 1000;
const ID_OPACITY_20: u16 = 1020;
const ID_OPACITY_40: u16 = 1021;
const ID_OPACITY_60: u16 = 1022;
const ID_OPACITY_80: u16 = 1023;
const ID_OPACITY_100: u16 = 1024;
const ID_TOGGLE_GPU: u16 = 1010;
const ID_TOGGLE_DISK_SPACE: u16 = 1011;
const ID_EXIT: u16 = 1099;
const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::w!("WinPulseNativeClass");

const BG: u32 = rgb(25, 27, 31);
const TRACK: u32 = rgb(55, 59, 66);
const TEXT: u32 = rgb(235, 238, 242);
const NORMAL_COLOR: u32 = rgb(34, 197, 94);   // Green: 0–60%
const ELEVATED_COLOR: u32 = rgb(234, 179, 8); // Yellow: >60–<90%
const HIGH_COLOR: u32 = rgb(239, 68, 68);     // Red: 90–100%

// Shared semantic color policy: color communicates utilization, not resource type.
fn usage_color(value: f32) -> u32 {
    if value >= 90.0 {
        HIGH_COLOR
    } else if value >= 70.0 {
        ELEVATED_COLOR
    } else {
        NORMAL_COLOR
    }
}

// COLORREF stores red in the low byte, then green, then blue.
const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UiMode { Horizontal }

const ROW_TOP: i32 = 8;
const ROW_STEP: i32 = 24;
const ROW_CONTENT_HEIGHT: i32 = 18;
const BOTTOM_PADDING: i32 = 7;
const WIDGET_WIDTH: i32 = 420;

impl UiMode {
    fn size(self, state: &AppState) -> (i32, i32) {
        // Four permanent rows (CPU, RAM, Network, Disk I/O). Optional rows
        // consume space only while enabled. Match render_horizontal's row grid
        // exactly instead of reserving room for hidden metrics.
        let rows = visible_row_count(state) as i32;
        let height = ROW_TOP + (rows - 1).max(0) * ROW_STEP
            + ROW_CONTENT_HEIGHT + BOTTOM_PADDING;
        (WIDGET_WIDTH, height)
    }
}

fn visible_row_count(state: &AppState) -> usize {
    4 + usize::from(state.show_disk_space) + usize::from(state.show_gpu)
}

#[derive(Default)]
struct CpuCounters {
    idle: u64,
    kernel: u64,
    user: u64,
    initialized: bool,
}

struct AppState {
    mode: UiMode,
    cpu_usage: f32,
    ram_usage: f32,
    net_in_bps: u64,
    net_out_bps: u64,
    disk_read_bps: u64,
    disk_write_bps: u64,
    disk_used_percent: f32,
    disk_used_bytes: u64,
    disk_total_bytes: u64,
    disk_label: String,
    gpu_usage: Option<f32>,
    show_gpu: bool,
    show_disk_space: bool,
    cpu: CpuCounters,
    system: System,
    networks: Networks,
    disks: Disks,
    previous_net: Option<(u64, u64)>,
    opacity: u8,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: UiMode::Horizontal,
            cpu_usage: 0.0,
            ram_usage: 0.0,
            net_in_bps: 0,
            net_out_bps: 0,
            disk_read_bps: 0,
            disk_write_bps: 0,
            disk_used_percent: 0.0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            disk_label: "DISK".to_string(),
            gpu_usage: None,
            show_gpu: false,
            show_disk_space: false,
            cpu: CpuCounters::default(),
            system: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            previous_net: None,
            opacity: 80,
        }
    }
}

fn main() {
    unsafe {
        // Opt into per-monitor-v2 scaling before creating any HWNDs.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(ptr::null());
        if instance.is_null() {
            return;
        }

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: ptr::null_mut(), // We paint the entire client area ourselves.
            lpszClassName: CLASS_NAME,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hIcon: ptr::null_mut(),
            lpszMenuName: ptr::null(),
        };

        if RegisterClassW(&wc) == 0 {
            return;
        }

        let mut state = Box::new(AppState::default());
        load_preferences(&mut state);
        let state_ptr = (&mut *state) as *mut AppState;
        let (width, height) = state.mode.size(&state);
        let (x, y) = initial_position(width, height);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            CLASS_NAME,
            windows_sys::w!("WinPulse - Resource Monitor"),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            width,
            height,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            state_ptr.cast(),
        );

        if hwnd.is_null() {
            return;
        }

        // The window owns the state for the lifetime of its message loop.
        let _ = Box::into_raw(state);
        apply_window_opacity(hwnd, (*state_ptr).opacity);
        keep_in_work_area(hwnd);
        update_metrics(&mut *state_ptr);
        if SetTimer(hwnd, TIMER_ID, REFRESH_MS, None) == 0 {
            DestroyWindow(hwnd);
            return;
        }

        let mut msg: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            if result <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// The window owns this pointer from WM_NCCREATE until WM_DESTROY. The window
// procedure runs on one UI thread, so callers must not retain references across
// callbacks or create overlapping mutable references.
unsafe fn state_ptr(hwnd: HWND) -> *mut AppState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            1
        }
        WM_TIMER if wparam == TIMER_ID => {
            let state = state_ptr(hwnd);
            if !state.is_null() {
                update_metrics(&mut *state);
            }
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            // Keep the widget easy to reposition without adding a title bar.
            ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
            0
        }
        WM_EXITSIZEMOVE => {
            keep_in_work_area(hwnd);
            0
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            let state = state_ptr(hwnd);
            if !state.is_null() { resize_to_content(hwnd, &*state); }
            keep_in_work_area(hwnd);
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_RBUTTONUP => {
            show_context_menu(hwnd);
            0
        }
        WM_COMMAND => {
            match (wparam & 0xffff) as u16 {
                ID_OPACITY_20 => set_opacity(hwnd, 20),
                ID_OPACITY_40 => set_opacity(hwnd, 40),
                ID_OPACITY_60 => set_opacity(hwnd, 60),
                ID_OPACITY_80 => set_opacity(hwnd, 80),
                ID_OPACITY_100 => set_opacity(hwnd, 100),
                ID_TOGGLE_GPU => toggle_option(hwnd, true),
                ID_TOGGLE_DISK_SPACE => toggle_option(hwnd, false),
                ID_EXIT => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_ID);
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !raw.is_null() {
                drop(Box::from_raw(raw));
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn initial_position(width: i32, height: i32) -> (i32, i32) {
    // Use the primary monitor's work area initially; subsequent moves and display changes
    // are clamped against the monitor nearest the widget.
    let mut work: RECT = zeroed();
    if SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0) != 0 {
        let x = (work.right - width - 16).max(work.left).min(work.right - width.max(1));
        let y = (work.bottom - height - 16).max(work.top).min(work.bottom - height.max(1));
        (x, y)
    } else { (100, 100) }
}

unsafe fn work_area_for(hwnd: HWND) -> RECT {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 { info.rcWork }
    else { let mut r: RECT = zeroed(); SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut r as *mut RECT).cast(), 0); r }
}

unsafe fn keep_in_work_area(hwnd: HWND) {
    let mut r: RECT = zeroed();
    if GetWindowRect(hwnd, &mut r) == 0 { return; }
    let work = work_area_for(hwnd);
    let work_w = (work.right - work.left).max(1); let work_h = (work.bottom - work.top).max(1);
    let w = (r.right - r.left).clamp(1, work_w); let h = (r.bottom - r.top).clamp(1, work_h);
    let max_x = (work.right - w).max(work.left); let max_y = (work.bottom - h).max(work.top);
    let x = r.left.clamp(work.left, max_x); let y = r.top.clamp(work.top, max_y);
    if x != r.left || y != r.top || w != r.right-r.left || h != r.bottom-r.top { SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE); }
}

unsafe fn show_context_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }

    let state = state_ptr(hwnd);
    let (show_gpu, show_space) = if state.is_null() { (false, false) } else { ((*state).show_gpu, (*state).show_disk_space) };
    append_menu_item(menu, ID_TOGGLE_GPU, "Show GPU (N/A if unavailable)", show_gpu);
    append_menu_item(menu, ID_TOGGLE_DISK_SPACE, "Show disk space", show_space);
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    let opacity_menu = CreatePopupMenu();
    if !opacity_menu.is_null() {
        let op = if state.is_null() { 80 } else { (*state).opacity };
        append_menu_item(opacity_menu, ID_OPACITY_20, "20% - most transparent", op == 20);
        append_menu_item(opacity_menu, ID_OPACITY_40, "40%", op == 40);
        append_menu_item(opacity_menu, ID_OPACITY_60, "60%", op == 60);
        append_menu_item(opacity_menu, ID_OPACITY_80, "80%", op == 80 || op == 75);
        append_menu_item(opacity_menu, ID_OPACITY_100, "100% - opaque", op == 100);
        AppendMenuW(menu, MF_POPUP, opacity_menu as usize, windows_sys::w!("Widget opacity"));
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    AppendMenuW(menu, MF_STRING, ID_EXIT as usize, windows_sys::w!("Exit WinPulse"));

    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    let selected = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, ptr::null());
    DestroyMenu(menu);
    if selected != 0 {
        SendMessageW(hwnd, WM_COMMAND, selected as usize, 0);
    }
}

unsafe fn append_menu_item(menu: HMENU, id: u16, label: &'static str, checked: bool) {
    let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    AppendMenuW(menu, flags, id as usize, wide.as_ptr());
}

unsafe fn toggle_option(hwnd: HWND, gpu: bool) {
    let state = state_ptr(hwnd);
    if state.is_null() { return; }
    if gpu { (*state).show_gpu = !(*state).show_gpu; }
    else { (*state).show_disk_space = !(*state).show_disk_space; }
    resize_to_content(hwnd, &*state);
    save_preferences(&*state);
    InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn resize_to_content(hwnd: HWND, state: &AppState) {
    let (width, height) = state.mode.size(state);
    let work = work_area_for(hwnd);
    let work_w = (work.right - work.left).max(1);
    let work_h = (work.bottom - work.top).max(1);
    let width = width.min(work_w);
    let height = height.min(work_h);
    let mut r: RECT = zeroed();
    let _ = GetWindowRect(hwnd, &mut r);
    let x = r.left.clamp(work.left, (work.right - width).max(work.left));
    let y = r.top.clamp(work.top, (work.bottom - height).max(work.top));
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
    InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn set_opacity(hwnd: HWND, opacity: u8) {
    let state = state_ptr(hwnd); if state.is_null() { return; }
    (*state).opacity = opacity; apply_window_opacity(hwnd, opacity); save_preferences(&*state); InvalidateRect(hwnd, ptr::null(), 0);
}

unsafe fn update_metrics(state: &mut AppState) {
    let mut mem_info: MEMORYSTATUSEX = zeroed();
    mem_info.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if GlobalMemoryStatusEx(&mut mem_info) != 0 {
        state.ram_usage = (mem_info.dwMemoryLoad as f32).clamp(0.0, 100.0);
    }

    // Resource providers are isolated by metric type for easier maintenance.
    state.system.refresh_memory();
    let net = network::sample(&mut state.networks, &mut state.previous_net);
    state.net_in_bps = net.received_bytes_per_sec;
    state.net_out_bps = net.transmitted_bytes_per_sec;

    let disk_metrics = disk::sample(&mut state.disks);
    state.disk_read_bps = disk_metrics.read_bytes_per_sec;
    state.disk_write_bps = disk_metrics.written_bytes_per_sec;
    state.disk_total_bytes = disk_metrics.total_bytes;
    state.disk_label = state.disks.list().first().map(|d| { let m = d.mount_point().to_string_lossy(); let label = m.trim_end_matches('\\').trim_end_matches('/'); if label.is_empty() { "DISK".to_string() } else { format!("SPACE {}", label) } }).unwrap_or_else(|| "SPACE".to_string());
    state.disk_used_bytes = disk_metrics.used_bytes;
    state.disk_used_percent = disk_metrics.used_percent;
    state.gpu_usage = gpu::sample();

    let mut idle: u64 = 0;
    let mut kernel: u64 = 0;
    let mut user: u64 = 0;
    if GetSystemTimes(
        (&mut idle as *mut u64).cast(),
        (&mut kernel as *mut u64).cast(),
        (&mut user as *mut u64).cast(),
    ) == 0 {
        return;
    }

    // The first successful read is a baseline, not a valid utilization sample.
    if state.cpu.initialized {
        let diff_idle = idle.saturating_sub(state.cpu.idle);
        let diff_kernel = kernel.saturating_sub(state.cpu.kernel);
        let diff_user = user.saturating_sub(state.cpu.user);
        let total = diff_kernel.saturating_add(diff_user);
        if total > 0 && diff_idle <= total {
            state.cpu_usage = (((total - diff_idle) as f64 * 100.0) / total as f64)
                .clamp(0.0, 100.0) as f32;
        }
    }

    state.cpu.idle = idle;
    state.cpu.kernel = kernel;
    state.cpu.user = user;
    state.cpu.initialized = true;
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let paint_dc = BeginPaint(hwnd, &mut ps);
    if paint_dc.is_null() {
        EndPaint(hwnd, &ps);
        return;
    }

    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width <= 0 || height <= 0 {
        EndPaint(hwnd, &ps);
        return;
    }

    // Render with ordinary GDI first, then copy into a 32-bit DIB for alpha.
    let memory_dc = CreateCompatibleDC(paint_dc);
    if memory_dc.is_null() {
        EndPaint(hwnd, &ps);
        return;
    }
    let memory_bitmap = CreateCompatibleBitmap(paint_dc, width, height);
    if memory_bitmap.is_null() {
        DeleteDC(memory_dc);
        EndPaint(hwnd, &ps);
        return;
    }
    let old_memory_bitmap = SelectObject(memory_dc, memory_bitmap);
    let background_brush = CreateSolidBrush(BG);
    FillRect(memory_dc, &client, background_brush);
    DeleteObject(background_brush);

    let state = state_ptr(hwnd);
    if !state.is_null() {
        render_horizontal(memory_dc, width, height, &*state);
    }

    let mut bitmap_info: BITMAPINFO = zeroed();
    bitmap_info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bitmap_info.bmiHeader.biWidth = width;
    bitmap_info.bmiHeader.biHeight = -height; // top-down DIB
    bitmap_info.bmiHeader.biPlanes = 1;
    bitmap_info.bmiHeader.biBitCount = 32;
    bitmap_info.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut core::ffi::c_void = ptr::null_mut();
    let dib = CreateDIBSection(paint_dc, &bitmap_info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);

    if !dib.is_null() && !bits.is_null() {
        let dib_dc = CreateCompatibleDC(paint_dc);
        if !dib_dc.is_null() {
            let old_dib = SelectObject(dib_dc, dib);
            BitBlt(dib_dc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY);

            let current_state = state_ptr(hwnd);
            let opacity = if current_state.is_null() { 80 } else { (*current_state).opacity };
            let alpha = (u32::from(opacity.clamp(20, 100)) * 255) / 100;
            let pixels = bits.cast::<u32>();
            let count = (width as usize).saturating_mul(height as usize);
            for index in 0..count {
                let pixel = *pixels.add(index);
                // A DIB pixel is 0x00RRGGBB. BG is a COLORREF (0x00BBGGRR).
                let blue = pixel & 0xff;
                let green = (pixel >> 8) & 0xff;
                let red = (pixel >> 16) & 0xff;
                let is_background = red == (BG & 0xff)
                    && green == ((BG >> 8) & 0xff)
                    && blue == ((BG >> 16) & 0xff);
                if is_background {
                    // Premultiply RGB for AC_SRC_ALPHA. Foreground remains opaque.
                    *pixels.add(index) = (alpha << 24)
                        | ((((red * alpha) / 255) & 0xff) << 16)
                        | ((((green * alpha) / 255) & 0xff) << 8)
                        | (((blue * alpha) / 255) & 0xff);
                } else {
                    *pixels.add(index) = 0xff00_0000 | (pixel & 0x00ff_ffff);
                }
            }

            let mut destination = POINT { x: 0, y: 0 };
            let mut source = POINT { x: 0, y: 0 };
            let size = SIZE { cx: width, cy: height };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let mut window_rect: RECT = zeroed();
            if GetWindowRect(hwnd, &mut window_rect) != 0 {
                destination.x = window_rect.left;
                destination.y = window_rect.top;
                let _ = UpdateLayeredWindow(
                    hwnd,
                    ptr::null_mut(),
                    &mut destination,
                    &size,
                    dib_dc,
                    &mut source,
                    0,
                    &blend,
                    ULW_ALPHA,
                );
            }
            SelectObject(dib_dc, old_dib);
            DeleteDC(dib_dc);
            DeleteObject(dib);
        }
    }

    SelectObject(memory_dc, old_memory_bitmap);
    DeleteObject(memory_bitmap);
    DeleteDC(memory_dc);
    EndPaint(hwnd, &ps);
}

unsafe fn render_horizontal(hdc: HDC, width: i32, _height: i32, state: &AppState) {
    let mut y = ROW_TOP;
    draw_metric_row(hdc, width, y, "CPU", format!("{:.0}%", state.cpu_usage), Some(state.cpu_usage), state.cpu_usage); y += ROW_STEP;
    draw_metric_row(hdc, width, y, "RAM", format!("{:.0}%", state.ram_usage), Some(state.ram_usage), state.ram_usage); y += ROW_STEP;
    // Keep traffic compact and readable: direct values, not bars. sysinfo's
    // received bytes are inbound; transmitted bytes are outbound.
    draw_value_row(hdc, width, y, "NETWORK", &format!("↓ {}   ↑ {}", format_rate(state.net_in_bps), format_rate(state.net_out_bps))); y += ROW_STEP;
    draw_value_row(hdc, width, y, "DISK", &format!("↓ {}   ↑ {}", format_rate(state.disk_read_bps), format_rate(state.disk_write_bps))); y += ROW_STEP;
    if state.show_disk_space { draw_metric_row(hdc, width, y, &state.disk_label, format!("{:.0}% used", state.disk_used_percent), Some(state.disk_used_percent), state.disk_used_percent); y += ROW_STEP; }
    if state.show_gpu { let (v, p) = match state.gpu_usage { Some(v) => (format!("{v:.0}%"), Some(v)), None => ("N/A".to_string(), None) }; draw_metric_row(hdc, width, y, "GPU", v, p, state.gpu_usage.unwrap_or(0.0)); }
}

unsafe fn draw_value_row(hdc: HDC, width: i32, y: i32, label: &str, value: &str) {
    draw_label(hdc, 12, y + 1, label, TEXT);
    draw_right_text(hdc, width - 10, y + 1, value, TEXT);
}

unsafe fn draw_metric_row(hdc: HDC, width: i32, y: i32, label: &str, value: String, utilization: Option<f32>, _unused: f32) {
    let left = 12;
    let label_width = 66;
    let value_width = 82;
    let bar_x = left + label_width;
    let bar_width = (width - bar_x - value_width - 14).max(24);
    let color = utilization.map(usage_color).unwrap_or(TEXT);
    draw_label(hdc, left, y + 1, label, TEXT);
    if let Some(v) = utilization {
        draw_bar(hdc, bar_x, y + 3, bar_width, 12, v, usage_color(v));
    } else {
        draw_bar(hdc, bar_x, y + 3, bar_width, 12, 0.0, TRACK);
    }
    draw_right_text(hdc, width - 10, y + 1, &value, color);
}

fn format_rate(bytes_per_second: u64) -> String {
    if bytes_per_second >= 1024 * 1024 {
        format!("{:.1} MB/s", bytes_per_second as f64 / (1024.0 * 1024.0))
    } else if bytes_per_second >= 1024 {
        format!("{:.0} KB/s", bytes_per_second as f64 / 1024.0)
    } else {
        format!("{} B/s", bytes_per_second)
    }
}

unsafe fn draw_bar(hdc: HDC, x: i32, y: i32, width: i32, height: i32, value: f32, color: u32) {
    let track = CreateSolidBrush(TRACK);
    let track_rect = RECT { left: x, top: y, right: x + width, bottom: y + height };
    FillRect(hdc, &track_rect, track);
    DeleteObject(track);

    let fill_width = ((width as f32 * value.clamp(0.0, 100.0) / 100.0).round() as i32).clamp(0, width);
    if fill_width > 0 {
        let fill = CreateSolidBrush(color);
        let fill_rect = RECT { left: x, top: y, right: x + fill_width, bottom: y + height };
        FillRect(hdc, &fill_rect, fill);
        DeleteObject(fill);
    }
}


unsafe fn draw_label(hdc: HDC, x: i32, y: i32, text: &str, color: u32) {
    SetBkMode(hdc, TRANSPARENT as i32);
    SetTextColor(hdc, color);
    let wide: Vec<u16> = text.encode_utf16().collect();
    TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32);
}

unsafe fn draw_right_text(hdc: HDC, right_x: i32, y: i32, text: &str, color: u32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut extent: SIZE = zeroed();
    GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut extent);
    draw_label(hdc, right_x - extent.cx, y, text, color);
}

fn preferences_path() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("WinPulse").join("preferences.ini")
}
fn load_preferences(state: &mut AppState) {
    if let Ok(text) = fs::read_to_string(preferences_path()) {
        for line in text.lines() { let mut p = line.splitn(2, '='); let k = p.next().unwrap_or(""); let v = p.next().unwrap_or(""); match k {
            "opacity" => if let Ok(n) = v.parse::<u8>() { state.opacity = n.clamp(20, 100); },
            "show_gpu" => state.show_gpu = v == "true", "show_disk_space" => state.show_disk_space = v == "true", _ => {}
        }}
    }
}
fn save_preferences(state: &AppState) {
    let path = preferences_path(); if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
    let content = format!("opacity={}\nshow_gpu={}\nshow_disk_space={}\n", state.opacity, state.show_gpu, state.show_disk_space);
    let _ = fs::write(path, content);
}
unsafe fn apply_window_opacity(hwnd: HWND, _opacity: u8) {
    // Per-pixel alpha is applied in paint(); never apply LWA_ALPHA here because
    // that fades text and bars along with the panel background.
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_LAYERED == 0 {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex_style | WS_EX_LAYERED) as isize);
    }
    let _ = RedrawWindow(
        hwnd,
        ptr::null(),
        ptr::null_mut(),
        RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
    );
}

#[cfg(test)]
mod tests {
    use super::{format_rate, visible_row_count, AppState, UiMode};
    #[test] fn rates_use_human_units() {
        assert_eq!(format_rate(0), "0 B/s");
        assert_eq!(format_rate(1024), "1 KB/s");
        assert_eq!(format_rate(1024 * 1024), "1.0 MB/s");
    }
    #[test] fn hidden_optional_rows_do_not_reserve_window_height() {
        let mut state = AppState::default();
        assert!(!state.show_gpu && !state.show_disk_space);
        assert_eq!(visible_row_count(&state), 4);
        let base = UiMode::Horizontal.size(&state).1;
        state.show_gpu = true;
        assert_eq!(visible_row_count(&state), 5);
        assert_eq!(UiMode::Horizontal.size(&state).1, base + 24);
        state.show_disk_space = true;
        assert_eq!(visible_row_count(&state), 6);
        assert_eq!(UiMode::Horizontal.size(&state).1, base + 48);
    }
}
