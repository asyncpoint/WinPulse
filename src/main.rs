#![windows_subsystem = "windows"]

use std::mem::{size_of, zeroed};
use std::ptr;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 1000;
const ID_MODE_HORIZONTAL: u16 = 1001;
const ID_MODE_VERTICAL_METERS: u16 = 1002;
const ID_MODE_STACKED: u16 = 1003;
const ID_EXIT: u16 = 1099;
const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::w!("WinPulseNativeClass");

const BG: u32 = rgb(25, 27, 31);
const TRACK: u32 = rgb(55, 59, 66);
const TEXT: u32 = rgb(235, 238, 242);
const MUTED: u32 = rgb(155, 163, 174);
const NORMAL_COLOR: u32 = rgb(34, 197, 94);   // Green: 0–60%
const ELEVATED_COLOR: u32 = rgb(234, 179, 8); // Yellow: >60–<90%
const HIGH_COLOR: u32 = rgb(239, 68, 68);     // Red: 90–100%

// Shared semantic color policy: color communicates utilization, not resource type.
fn usage_color(value: f32) -> u32 {
    if value >= 90.0 {
        HIGH_COLOR
    } else if value > 60.0 {
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
            Self::Horizontal => (270, 72),
            Self::VerticalMeters => (112, 174),
            Self::StackedBars => (260, 104),
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
    cpu: CpuCounters,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: UiMode::Horizontal,
            cpu_usage: 0.0,
            ram_usage: 0.0,
            cpu: CpuCounters::default(),
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
        update_metrics(state_ptr);
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
                ID_EXIT => DestroyWindow(hwnd),
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
        ((work.right - width - 16).max(work.left), (work.bottom - height - 16).max(work.top))
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
    let current = if state.is_null() { None } else { Some((*state).mode) };
    append_menu_item(menu, ID_MODE_HORIZONTAL, "Horizontal floating bar", current == Some(UiMode::Horizontal));
    append_menu_item(menu, ID_MODE_VERTICAL_METERS, "Vertical meters", current == Some(UiMode::VerticalMeters));
    append_menu_item(menu, ID_MODE_STACKED, "Stacked horizontal bars", current == Some(UiMode::StackedBars));
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

unsafe fn set_mode(hwnd: HWND, mode: UiMode) {
    let state = state_ptr(hwnd);
    if !state.is_null() {
        (*state).mode = mode;
    }
    let (width, height) = mode.size();
    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, width, height, SWP_NOMOVE | SWP_NOACTIVATE);
    InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn update_metrics(state: &mut AppState) {
    let mut mem_info: MEMORYSTATUSEX = zeroed();
    mem_info.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if GlobalMemoryStatusEx(&mut mem_info) != 0 {
        state.ram_usage = (mem_info.dwMemoryLoad as f32).clamp(0.0, 100.0);
    }

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

unsafe fn render_horizontal(hdc: HDC, width: i32, height: i32, state: &AppState) {
    let left = 12;
    let label_w = 42;
    let right = width - 12;
    let bar_left = left + label_w;
    let bar_width = (right - bar_left).max(20);
    let row_height = 22;
    let top = ((height - row_height * 2) / 2).max(5);
    let cpu_color = usage_color(state.cpu_usage);
    let ram_color = usage_color(state.ram_usage);
    draw_label(hdc, left, top + 1, "CPU", cpu_color);
    draw_label(hdc, left, top + row_height + 1, "RAM", ram_color);
    draw_bar(hdc, bar_left, top + 4, bar_width, 12, state.cpu_usage, cpu_color);
    draw_bar(hdc, bar_left, top + row_height + 4, bar_width, 12, state.ram_usage, ram_color);
}

unsafe fn render_vertical_meters(hdc: HDC, width: i32, height: i32, state: &AppState) {
    let top = 12;
    let bottom = height - 30;
    let meter_height = (bottom - top).max(20);
    let meter_width = 28;
    let gap = 16;
    let total_width = meter_width * 2 + gap;
    let start_x = ((width - total_width) / 2).max(8);
    let x_cpu = start_x;
    let x_ram = start_x + meter_width + gap;

    let cpu_color = usage_color(state.cpu_usage);
    let ram_color = usage_color(state.ram_usage);
    draw_centered_text(hdc, x_cpu + meter_width / 2, 4, "CPU", cpu_color);
    draw_centered_text(hdc, x_ram + meter_width / 2, 4, "RAM", ram_color);
    draw_vertical_bar(hdc, x_cpu, top, meter_width, meter_height, state.cpu_usage, cpu_color);
    draw_vertical_bar(hdc, x_ram, top, meter_width, meter_height, state.ram_usage, ram_color);
    draw_centered_text(hdc, x_cpu + meter_width / 2, bottom + 4, &format!("{:.0}%", state.cpu_usage), cpu_color);
    draw_centered_text(hdc, x_ram + meter_width / 2, bottom + 4, &format!("{:.0}%", state.ram_usage), ram_color);
}

unsafe fn render_stacked(hdc: HDC, width: i32, _height: i32, state: &AppState) {
    let left = 14;
    let label_width = 48;
    let bar_left = left + label_width;
    let bar_width = (width - bar_left - 48).max(24);
    let cpu_color = usage_color(state.cpu_usage);
    let ram_color = usage_color(state.ram_usage);
    draw_label(hdc, left, 22, "CPU", cpu_color);
    draw_label(hdc, left, 62, "RAM", ram_color);
    draw_bar(hdc, bar_left, 24, bar_width, 14, state.cpu_usage, cpu_color);
    draw_bar(hdc, bar_left, 64, bar_width, 14, state.ram_usage, ram_color);
    draw_right_text(hdc, width - 10, 22, &format!("{:.0}%", state.cpu_usage), cpu_color);
    draw_right_text(hdc, width - 10, 62, &format!("{:.0}%", state.ram_usage), ram_color);
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

unsafe fn draw_vertical_bar(hdc: HDC, x: i32, y: i32, width: i32, height: i32, value: f32, color: u32) {
    let track = CreateSolidBrush(TRACK);
    let track_rect = RECT { left: x, top: y, right: x + width, bottom: y + height };
    FillRect(hdc, &track_rect, track);
    DeleteObject(track);

    let fill_height = ((height as f32 * value.clamp(0.0, 100.0) / 100.0).round() as i32).clamp(0, height);
    if fill_height > 0 {
        let fill = CreateSolidBrush(color);
        let fill_rect = RECT { left: x, top: y + height - fill_height, right: x + width, bottom: y + height };
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
