#![windows_subsystem = "windows"]

use std::fs;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::ptr;
use sysinfo::{Disks, Networks, System};
mod monitoring;
use monitoring::{disk, gpu, network};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 1000;
const ID_MODE_HORIZONTAL: u16 = 1001;
const ID_MODE_VERTICAL_METERS: u16 = 1002;
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
const MUTED: u32 = rgb(155, 163, 174);
const NORMAL_COLOR: u32 = rgb(34, 197, 94); // Green: 0–60%
const ELEVATED_COLOR: u32 = rgb(234, 179, 8); // Yellow: >60–<90%
const HIGH_COLOR: u32 = rgb(239, 68, 68); // Red: 90–100%

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
enum UiMode {
    Horizontal,
    VerticalMeters,
}

impl UiMode {
    fn size(self) -> (i32, i32) {
        match self {
            Self::Horizontal => (420, 220),
            Self::VerticalMeters => (360, 330),
        }
    }
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
    net_in_peak: f64,
    net_out_peak: f64,
    disk_read_peak: f64,
    disk_write_peak: f64,
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
            show_gpu: true,
            show_disk_space: false,
            cpu: CpuCounters::default(),
            system: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            previous_net: None,
            net_in_peak: 1024.0,
            net_out_peak: 1024.0,
            disk_read_peak: 1024.0,
            disk_write_peak: 1024.0,
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
        let (width, height) = state.mode.size();
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
                ID_MODE_HORIZONTAL => set_mode(hwnd, UiMode::Horizontal),
                ID_MODE_VERTICAL_METERS => set_mode(hwnd, UiMode::VerticalMeters),
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
        let x = (work.right - width - 16)
            .max(work.left)
            .min(work.right - width.max(1));
        let y = (work.bottom - height - 16)
            .max(work.top)
            .min(work.bottom - height.max(1));
        (x, y)
    } else {
        (100, 100)
    }
}

unsafe fn work_area_for(hwnd: HWND) -> RECT {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
        info.rcWork
    } else {
        let mut r: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut r as *mut RECT).cast(), 0);
        r
    }
}

unsafe fn keep_in_work_area(hwnd: HWND) {
    let mut r: RECT = zeroed();
    if GetWindowRect(hwnd, &mut r) == 0 {
        return;
    }
    let work = work_area_for(hwnd);
    let work_w = (work.right - work.left).max(1);
    let work_h = (work.bottom - work.top).max(1);
    let w = (r.right - r.left).clamp(1, work_w);
    let h = (r.bottom - r.top).clamp(1, work_h);
    let max_x = (work.right - w).max(work.left);
    let max_y = (work.bottom - h).max(work.top);
    let x = r.left.clamp(work.left, max_x);
    let y = r.top.clamp(work.top, max_y);
    if x != r.left || y != r.top || w != r.right - r.left || h != r.bottom - r.top {
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
    }
}

unsafe fn show_context_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }

    let state = state_ptr(hwnd);
    let current = if state.is_null() {
        None
    } else {
        Some((*state).mode)
    };
    append_menu_item(
        menu,
        ID_MODE_HORIZONTAL,
        "Horizontal bars",
        current == Some(UiMode::Horizontal),
    );
    append_menu_item(
        menu,
        ID_MODE_VERTICAL_METERS,
        "Vertical Bars",
        current == Some(UiMode::VerticalMeters),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    let (show_gpu, show_space) = if state.is_null() {
        (true, false)
    } else {
        ((*state).show_gpu, (*state).show_disk_space)
    };
    append_menu_item(
        menu,
        ID_TOGGLE_GPU,
        "Show GPU (N/A if unavailable)",
        show_gpu,
    );
    append_menu_item(menu, ID_TOGGLE_DISK_SPACE, "Show disk space", show_space);
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    let opacity_menu = CreatePopupMenu();
    if !opacity_menu.is_null() {
        let op = if state.is_null() {
            80
        } else {
            (*state).opacity
        };
        append_menu_item(
            opacity_menu,
            ID_OPACITY_20,
            "20% - most transparent",
            op == 20,
        );
        append_menu_item(opacity_menu, ID_OPACITY_40, "40%", op == 40);
        append_menu_item(opacity_menu, ID_OPACITY_60, "60%", op == 60);
        append_menu_item(opacity_menu, ID_OPACITY_80, "80%", op == 80 || op == 75);
        append_menu_item(opacity_menu, ID_OPACITY_100, "100% - opaque", op == 100);
        AppendMenuW(
            menu,
            MF_POPUP,
            opacity_menu as usize,
            windows_sys::w!("Background opacity"),
        );
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    AppendMenuW(
        menu,
        MF_STRING,
        ID_EXIT as usize,
        windows_sys::w!("Exit WinPulse"),
    );

    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    let selected = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        0,
        hwnd,
        ptr::null(),
    );
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
    if state.is_null() {
        return;
    }
    if gpu {
        (*state).show_gpu = !(*state).show_gpu;
    } else {
        (*state).show_disk_space = !(*state).show_disk_space;
    }
    let (width, height) = (*state).mode.size();
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        width,
        height,
        SWP_NOMOVE | SWP_NOACTIVATE,
    );
    keep_in_work_area(hwnd);
    save_preferences(&*state);
    InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn set_mode(hwnd: HWND, mode: UiMode) {
    let state = state_ptr(hwnd);
    if !state.is_null() {
        (*state).mode = mode;
    }
    let (width, height) = mode.size();
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        width,
        height,
        SWP_NOMOVE | SWP_NOACTIVATE,
    );
    keep_in_work_area(hwnd);
    if !state.is_null() {
        save_preferences(&*state);
    }
    InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn set_opacity(hwnd: HWND, opacity: u8) {
    let state = state_ptr(hwnd);
    if state.is_null() {
        return;
    }
    (*state).opacity = opacity;
    apply_window_opacity(hwnd, opacity);
    save_preferences(&*state);
    InvalidateRect(hwnd, ptr::null(), 0);
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
    state.net_in_peak = adaptive_peak(state.net_in_peak, state.net_in_bps);
    state.net_out_peak = adaptive_peak(state.net_out_peak, state.net_out_bps);

    let disk_metrics = disk::sample(&mut state.disks);
    state.disk_read_bps = disk_metrics.read_bytes_per_sec;
    state.disk_write_bps = disk_metrics.written_bytes_per_sec;
    state.disk_read_peak = adaptive_peak(state.disk_read_peak, state.disk_read_bps);
    state.disk_write_peak = adaptive_peak(state.disk_write_peak, state.disk_write_bps);
    state.disk_total_bytes = disk_metrics.total_bytes;
    state.disk_label = state
        .disks
        .list()
        .first()
        .map(|d| {
            let m = d.mount_point().to_string_lossy();
            let label = m.trim_end_matches('\\').trim_end_matches('/');
            if label.is_empty() {
                "DISK".to_string()
            } else {
                format!("SPACE {}", label)
            }
        })
        .unwrap_or_else(|| "SPACE".to_string());
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
    ) == 0
    {
        return;
    }

    // The first successful read is a baseline, not a valid utilization sample.
    if state.cpu.initialized {
        let diff_idle = idle.saturating_sub(state.cpu.idle);
        let diff_kernel = kernel.saturating_sub(state.cpu.kernel);
        let diff_user = user.saturating_sub(state.cpu.user);
        let total = diff_kernel.saturating_add(diff_user);
        if total > 0 && diff_idle <= total {
            state.cpu_usage =
                (((total - diff_idle) as f64 * 100.0) / total as f64).clamp(0.0, 100.0) as f32;
        }
    }

    state.cpu.idle = idle;
    state.cpu.kernel = kernel;
    state.cpu.user = user;
    state.cpu.initialized = true;
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        EndPaint(hwnd, &ps);
        return;
    }

    let mut rect: RECT = zeroed();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        EndPaint(hwnd, &ps);
        return;
    }

    let mem_dc = CreateCompatibleDC(hdc);
    if mem_dc.is_null() {
        let fallback_brush = CreateSolidBrush(BG);
        if !fallback_brush.is_null() {
            FillRect(hdc, &rect, fallback_brush);
            DeleteObject(fallback_brush);
        }
        EndPaint(hwnd, &ps);
        return;
    }
    let bitmap = CreateCompatibleBitmap(hdc, width, height);
    if bitmap.is_null() {
        DeleteDC(mem_dc);
        let fallback_brush = CreateSolidBrush(BG);
        if !fallback_brush.is_null() {
            FillRect(hdc, &rect, fallback_brush);
            DeleteObject(fallback_brush);
        }
        EndPaint(hwnd, &ps);
        return;
    }
    let old_bitmap = SelectObject(mem_dc, bitmap);
    let bg_brush = CreateSolidBrush(BG);
    FillRect(mem_dc, &rect, bg_brush);
    DeleteObject(bg_brush);

    let state = state_ptr(hwnd);
    if !state.is_null() {
        match (*state).mode {
            UiMode::Horizontal => render_horizontal(mem_dc, width, height, &*state),
            UiMode::VerticalMeters => render_vertical_meters(mem_dc, width, height, &*state),
        }
    }

    // Convert the rendered frame to a 32-bit premultiplied-alpha DIB. Exact background
    // pixels receive the selected alpha; foreground pixels stay opaque so text/bars remain crisp.
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut core::ffi::c_void = ptr::null_mut();
    let dib = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);
    if !dib.is_null() && !bits.is_null() {
        let dib_dc = CreateCompatibleDC(hdc);
        if !dib_dc.is_null() {
            let old_dib = SelectObject(dib_dc, dib);
            BitBlt(dib_dc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
            let px = bits as *mut u32;
            let count = (width * height) as usize;
            let opacity = {
                let st = state_ptr(hwnd);
                if st.is_null() { 80u8 } else { (*st).opacity }
            };
            let alpha = opacity as u32 * 255 / 100;
            let bg = BG;
            for i in 0..count {
                let c = *px.add(i);
                let b = c & 0xff;
                let g = (c >> 8) & 0xff;
                let r = (c >> 16) & 0xff;
                if r == ((bg >> 16) & 0xff) && g == ((bg >> 8) & 0xff) && b == (bg & 0xff) {
                    *px.add(i) = ((alpha << 24)
                        | (((r * alpha / 255) & 0xff) << 16)
                        | (((g * alpha / 255) & 0xff) << 8)
                        | ((b * alpha / 255) & 0xff));
                } else {
                    *px.add(i) = 0xff000000 | (c & 0x00ffffff);
                }
            }
            let mut dst = POINT { x: 0, y: 0 };
            let mut src = POINT { x: 0, y: 0 };
            let size = SIZE {
                cx: width,
                cy: height,
            };
            let mut blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let mut wndpt: POINT = zeroed();
            GetWindowRect(hwnd, &mut rect);
            wndpt.x = rect.left;
            wndpt.y = rect.top;
            UpdateLayeredWindow(
                hwnd,
                ptr::null_mut(),
                &mut wndpt,
                &size,
                dib_dc,
                &mut src,
                0,
                &mut blend,
                ULW_ALPHA,
            );
            SelectObject(dib_dc, old_dib);
            DeleteDC(dib_dc);
            DeleteObject(dib);
        } else {
            BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
        }
    } else {
        BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
    }
    SelectObject(mem_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(mem_dc);
    EndPaint(hwnd, &ps);
}

unsafe fn render_horizontal(hdc: HDC, width: i32, _height: i32, state: &AppState) {
    let mut y = 10;
    draw_metric_row(
        hdc,
        width,
        y,
        "CPU",
        format!("{:.0}%", state.cpu_usage),
        Some(state.cpu_usage),
        state.cpu_usage,
    );
    y += 25;
    draw_metric_row(
        hdc,
        width,
        y,
        "RAM",
        format!("{:.0}%", state.ram_usage),
        Some(state.ram_usage),
        state.ram_usage,
    );
    y += 25;
    draw_rate_row(hdc, width, y, "NET IN", state.net_in_bps, state.net_in_peak);
    y += 25;
    draw_rate_row(
        hdc,
        width,
        y,
        "NET OUT",
        state.net_out_bps,
        state.net_out_peak,
    );
    y += 25;
    draw_rate_row(
        hdc,
        width,
        y,
        "DISK R",
        state.disk_read_bps,
        state.disk_read_peak,
    );
    y += 25;
    draw_rate_row(
        hdc,
        width,
        y,
        "DISK W",
        state.disk_write_bps,
        state.disk_write_peak,
    );
    y += 25;
    if state.show_disk_space {
        draw_metric_row(
            hdc,
            width,
            y,
            &state.disk_label,
            format!("{:.0}% used", state.disk_used_percent),
            Some(state.disk_used_percent),
            state.disk_used_percent,
        );
        y += 25;
    }
    if state.show_gpu {
        let (v, p) = match state.gpu_usage {
            Some(v) => (format!("{v:.0}%"), Some(v)),
            None => ("N/A".to_string(), None),
        };
        draw_metric_row(hdc, width, y, "GPU", v, p, state.gpu_usage.unwrap_or(0.0));
    }
}

fn adaptive_peak(current: f64, rate: u64) -> f64 {
    let floor = 1024.0_f64;
    let observed = rate as f64;
    // Fast attack, gradual decay: rates stay legible without a fixed arbitrary maximum.
    if observed > current {
        current * 0.35 + observed * 0.65
    } else {
        (current * 0.92 + observed * 0.08).max(floor)
    }
}

unsafe fn draw_rate_row(hdc: HDC, width: i32, y: i32, label: &str, rate: u64, peak: f64) {
    let left = 12;
    let label_width = 78;
    let value_width = 88;
    let bar_x = left + label_width;
    let bar_width = (width - bar_x - value_width - 14).max(24);
    draw_label(hdc, left, y + 1, label, TEXT);
    let pct = if rate == 0 {
        0.0
    } else {
        ((rate as f64 / peak.max(1024.0) * 100.0) as f32).max(100.0 / bar_width as f32)
    };
    draw_bar(
        hdc,
        bar_x,
        y + 3,
        bar_width,
        12,
        pct,
        if rate > 0 { NORMAL_COLOR } else { TRACK },
    );
    draw_right_text(hdc, width - 10, y + 1, &format_rate(rate), TEXT);
}

unsafe fn render_vertical_meters(hdc: HDC, width: i32, height: i32, state: &AppState) {
    let metrics: Vec<(&str, f32)> = vec![("CPU", state.cpu_usage), ("RAM", state.ram_usage)];
    let top = 32;
    let bottom = 150;
    let meter_height = (bottom - top).max(20);
    let meter_width = 58;
    let gap = 42;
    let total_width = meter_width * 2 + gap;
    let start_x = ((width - total_width) / 2).max(8);
    for (i, (label, value)) in metrics.iter().enumerate() {
        let x = start_x + i as i32 * (meter_width + gap);
        let color = usage_color(*value);
        draw_centered_text(hdc, x + meter_width / 2, 10, label, TEXT);
        draw_vertical_bar(hdc, x, top, meter_width, meter_height, *value, color);
        draw_centered_text(
            hdc,
            x + meter_width / 2,
            bottom + 5,
            &format!("{value:.0}%"),
            color,
        );
    }
    // Compute row spacing from the client height so every row stays inside the work area,
    // including on small displays and after per-monitor DPI changes.
    let mut rows: Vec<(&str, String, Option<f32>, f64)> = vec![
        (
            "NET IN",
            format_rate(state.net_in_bps),
            None,
            state.net_in_peak,
        ),
        (
            "NET OUT",
            format_rate(state.net_out_bps),
            None,
            state.net_out_peak,
        ),
        (
            "DISK R",
            format_rate(state.disk_read_bps),
            None,
            state.disk_read_peak,
        ),
        (
            "DISK W",
            format_rate(state.disk_write_bps),
            None,
            state.disk_write_peak,
        ),
    ];
    if state.show_disk_space {
        rows.push((
            &state.disk_label,
            format!("{:.0}% used", state.disk_used_percent),
            Some(state.disk_used_percent),
            0.0,
        ));
    }
    if state.show_gpu {
        let (v, p) = match state.gpu_usage {
            Some(v) => (format!("{v:.0}%"), Some(v)),
            None => ("N/A".to_string(), None),
        };
        rows.push(("GPU", v, p, 0.0));
    }
    let start_y = (bottom + 31).min(height.saturating_sub((rows.len() as i32) * 18 + 4));
    let available = (height - start_y - 4).max(1);
    let row_step = (available / rows.len().max(1) as i32).clamp(16, 25);
    for (i, (label, value, utilization, peak)) in rows.iter().enumerate() {
        let y = start_y + i as i32 * row_step;
        if y + 15 > height {
            break;
        }
        if *utilization == None {
            let rate = match *label {
                "NET IN" => state.net_in_bps,
                "NET OUT" => state.net_out_bps,
                "DISK R" => state.disk_read_bps,
                _ => state.disk_write_bps,
            };
            draw_rate_row(hdc, width, y, label, rate, *peak);
        } else {
            draw_metric_row(
                hdc,
                width,
                y,
                label,
                value.clone(),
                *utilization,
                utilization.unwrap_or(0.0),
            );
        }
    }
}

unsafe fn draw_metric_row(
    hdc: HDC,
    width: i32,
    y: i32,
    label: &str,
    value: String,
    utilization: Option<f32>,
    _unused: f32,
) {
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
    let track_rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    FillRect(hdc, &track_rect, track);
    DeleteObject(track);

    let fill_width =
        ((width as f32 * value.clamp(0.0, 100.0) / 100.0).round() as i32).clamp(0, width);
    if fill_width > 0 {
        let fill = CreateSolidBrush(color);
        let fill_rect = RECT {
            left: x,
            top: y,
            right: x + fill_width,
            bottom: y + height,
        };
        FillRect(hdc, &fill_rect, fill);
        DeleteObject(fill);
    }
}

unsafe fn draw_vertical_bar(
    hdc: HDC,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    value: f32,
    color: u32,
) {
    let track = CreateSolidBrush(TRACK);
    let track_rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    FillRect(hdc, &track_rect, track);
    DeleteObject(track);

    let fill_height =
        ((height as f32 * value.clamp(0.0, 100.0) / 100.0).round() as i32).clamp(0, height);
    if fill_height > 0 {
        let fill = CreateSolidBrush(color);
        let fill_rect = RECT {
            left: x,
            top: y + height - fill_height,
            right: x + width,
            bottom: y + height,
        };
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

unsafe fn draw_centered_text(hdc: HDC, center_x: i32, y: i32, text: &str, color: u32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut extent: SIZE = zeroed();
    GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut extent);
    draw_label(hdc, center_x - extent.cx / 2, y, text, color);
}

unsafe fn draw_right_text(hdc: HDC, right_x: i32, y: i32, text: &str, color: u32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut extent: SIZE = zeroed();
    GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut extent);
    draw_label(hdc, right_x - extent.cx, y, text, color);
}

fn preferences_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WinPulse")
        .join("preferences.ini")
}
fn load_preferences(state: &mut AppState) {
    if let Ok(text) = fs::read_to_string(preferences_path()) {
        for line in text.lines() {
            let mut p = line.splitn(2, '=');
            let k = p.next().unwrap_or("");
            let v = p.next().unwrap_or("");
            match k {
                "mode" => {
                    state.mode = if v == "vertical" {
                        UiMode::VerticalMeters
                    } else {
                        UiMode::Horizontal
                    }
                }
                "opacity" => {
                    if let Ok(n) = v.parse::<u8>() {
                        state.opacity = n.clamp(20, 100);
                    }
                }
                "show_gpu" => state.show_gpu = v == "true",
                "show_disk_space" => state.show_disk_space = v == "true",
                _ => {}
            }
        }
    }
}
fn save_preferences(state: &AppState) {
    let path = preferences_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mode = if state.mode == UiMode::VerticalMeters {
        "vertical"
    } else {
        "horizontal"
    };
    let content = format!(
        "mode={mode}\nopacity={}\nshow_gpu={}\nshow_disk_space={}\n",
        state.opacity, state.show_gpu, state.show_disk_space
    );
    let _ = fs::write(path, content);
}
unsafe fn apply_window_opacity(hwnd: HWND, _opacity: u8) {
    // The selected alpha is applied to background pixels by the per-pixel layered-window renderer.
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
}

#[cfg(test)]
mod tests {
    use super::{adaptive_peak, format_rate};
    #[test]
    fn rates_use_human_units() {
        assert_eq!(format_rate(0), "0 B/s");
        assert_eq!(format_rate(1024), "1 KB/s");
        assert_eq!(format_rate(1024 * 1024), "1.0 MB/s");
    }
    #[test]
    fn adaptive_peak_tracks_bursts_and_decays_slowly() {
        let p = adaptive_peak(1024.0, 1024 * 1024);
        assert!(p > 1024.0);
        assert!(adaptive_peak(p, 0) > 1024.0);
    }
}
