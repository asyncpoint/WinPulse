#![windows_subsystem = "windows"]

use std::mem::{size_of, zeroed};
use std::ptr;
use sysinfo::{Disks, Networks, System};
mod monitoring;
use monitoring::{disk, gpu, network};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 1000;
const ID_MODE_HORIZONTAL: u16 = 1001;
const ID_MODE_VERTICAL_METERS: u16 = 1002;
const ID_MODE_STACKED: u16 = 1003;
const ID_TOGGLE_GPU: u16 = 1010;
const ID_TOGGLE_DISK_SPACE: u16 = 1011;
const ID_EXIT: u16 = 1099;
const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::w!("WinPulseNativeClass");

const BG: u32 = rgb(25, 27, 31);
const TRACK: u32 = rgb(55, 59, 66);
const TEXT: u32 = rgb(235, 238, 242);
const MUTED: u32 = rgb(155, 163, 174);
const NORMAL_COLOR: u32 = rgb(34, 197, 94); // Green: <70%
const ELEVATED_COLOR: u32 = rgb(234, 179, 8); // Yellow: 70–<90%
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
    StackedBars,
}

impl UiMode {
    fn size(self) -> (i32, i32) {
        match self {
            Self::Horizontal => (360, 190),
            Self::VerticalMeters => (240, 310),
            Self::StackedBars => (340, 210),
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
    gpu_usage: Option<f32>,
    show_gpu: bool,
    show_disk_space: bool,
    cpu: CpuCounters,
    system: System,
    networks: Networks,
    disks: Disks,
    previous_net: Option<(u64, u64)>,
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
            gpu_usage: None,
            show_gpu: true,
            show_disk_space: false,
            cpu: CpuCounters::default(),
            system: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            previous_net: None,
        }
    }
}

fn main() {
    unsafe {
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
        let state_ptr = (&mut *state) as *mut AppState;
        let (width, height) = state.mode.size();
        let (x, y) = initial_position(width, height);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
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
        WM_RBUTTONUP => {
            show_context_menu(hwnd);
            0
        }
        WM_COMMAND => {
            match (wparam & 0xffff) as u16 {
                ID_MODE_HORIZONTAL => set_mode(hwnd, UiMode::Horizontal),
                ID_MODE_VERTICAL_METERS => set_mode(hwnd, UiMode::VerticalMeters),
                ID_MODE_STACKED => set_mode(hwnd, UiMode::StackedBars),
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
    let mut work: RECT = zeroed();
    if SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0) != 0 {
        (
            (work.right - width - 16).max(work.left),
            (work.bottom - height - 16).max(work.top),
        )
    } else {
        (100, 100)
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
        "Horizontal floating bar",
        current == Some(UiMode::Horizontal),
    );
    append_menu_item(
        menu,
        ID_MODE_VERTICAL_METERS,
        "Vertical meters",
        current == Some(UiMode::VerticalMeters),
    );
    append_menu_item(
        menu,
        ID_MODE_STACKED,
        "Stacked horizontal bars",
        current == Some(UiMode::StackedBars),
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
    InvalidateRect(hwnd, ptr::null(), 1);
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
            UiMode::StackedBars => render_stacked(mem_dc, width, height, &*state),
        }
    }

    BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
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
    y += 24;
    draw_metric_row(
        hdc,
        width,
        y,
        "RAM",
        format!("{:.0}%", state.ram_usage),
        Some(state.ram_usage),
        state.ram_usage,
    );
    y += 24;
    draw_metric_row(
        hdc,
        width,
        y,
        "NET IN",
        format_rate(state.net_in_bps),
        None,
        0.0,
    );
    y += 24;
    draw_metric_row(
        hdc,
        width,
        y,
        "NET OUT",
        format_rate(state.net_out_bps),
        None,
        0.0,
    );
    y += 24;
    draw_metric_row(
        hdc,
        width,
        y,
        "DISK R/W",
        format!(
            "{}/{}",
            format_rate(state.disk_read_bps),
            format_rate(state.disk_write_bps)
        ),
        None,
        0.0,
    );
    y += 24;
    if state.show_disk_space {
        draw_metric_row(
            hdc,
            width,
            y,
            "DISK",
            format!("{:.0}% used", state.disk_used_percent),
            Some(state.disk_used_percent),
            state.disk_used_percent,
        );
        y += 24;
    }
    if state.show_gpu {
        let (value, pct) = match state.gpu_usage {
            Some(v) => (format!("{v:.0}%"), Some(v)),
            None => ("N/A".to_string(), None),
        };
        draw_metric_row(
            hdc,
            width,
            y,
            "GPU",
            value,
            pct,
            state.gpu_usage.unwrap_or(0.0),
        );
    }
}

unsafe fn render_stacked(hdc: HDC, width: i32, _height: i32, state: &AppState) {
    render_horizontal(hdc, width, 0, state);
}

unsafe fn render_vertical_meters(hdc: HDC, width: i32, height: i32, state: &AppState) {
    // Labels occupy a dedicated row; bars start below them, avoiding the previous overlap.
    let metrics: Vec<(&str, f32)> = vec![("CPU", state.cpu_usage), ("RAM", state.ram_usage)];
    let top = 30;
    let bottom = 145;
    let meter_height = (bottom - top).max(20);
    let meter_width = 42;
    let gap = 26;
    let total_width = meter_width * 2 + gap;
    let start_x = ((width - total_width) / 2).max(8);
    for (i, (label, value)) in metrics.iter().enumerate() {
        let x = start_x + i as i32 * (meter_width + gap);
        let color = usage_color(*value);
        draw_centered_text(hdc, x + meter_width / 2, 8, label, TEXT);
        draw_vertical_bar(hdc, x, top, meter_width, meter_height, *value, color);
        draw_centered_text(
            hdc,
            x + meter_width / 2,
            bottom + 4,
            &format!("{value:.0}%"),
            color,
        );
    }
    let mut y = 176;
    draw_label(
        hdc,
        12,
        y,
        &format!("IN  {}", format_rate(state.net_in_bps)),
        TEXT,
    );
    y += 18;
    draw_label(
        hdc,
        12,
        y,
        &format!("OUT {}", format_rate(state.net_out_bps)),
        TEXT,
    );
    y += 18;
    draw_label(
        hdc,
        12,
        y,
        &format!(
            "DISK R/W {}/{}",
            format_rate(state.disk_read_bps),
            format_rate(state.disk_write_bps)
        ),
        TEXT,
    );
    y += 18;
    if state.show_disk_space {
        draw_label(
            hdc,
            12,
            y,
            &format!("SPACE {:.0}% used", state.disk_used_percent),
            usage_color(state.disk_used_percent),
        );
        y += 18;
    }
    if state.show_gpu {
        let gpu_text = state
            .gpu_usage
            .map(|v| format!("GPU {v:.0}%"))
            .unwrap_or_else(|| "GPU N/A".to_string());
        draw_label(
            hdc,
            12,
            y,
            &gpu_text,
            state.gpu_usage.map(usage_color).unwrap_or(MUTED),
        );
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
