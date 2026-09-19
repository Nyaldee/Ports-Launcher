
use std::cell::OnceCell;
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, GetObjectW, MonitorFromPoint, ReleaseDC, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, ReleaseCapture};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, DestroyIcon, GetCursorPos, GetForegroundWindow, GetIconInfo, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, MessageBoxW, PostMessageW, SendMessageW, SetForegroundWindow,
    SetProcessDPIAware, SetWindowLongPtrW, ShowWindow, GWLP_HWNDPARENT, GWL_EXSTYLE, HICON, HTCAPTION, ICONINFO, ICON_BIG,
    ICON_SMALL, MB_ICONERROR, MB_OK, MESSAGEBOX_STYLE, SW_HIDE, SW_SHOWNA, WM_LBUTTONUP, WM_NCLBUTTONDOWN, WM_SETICON,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

pub fn enable_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwareness, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        PROCESS_PER_MONITOR_DPI_AWARE,
    };
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err()
            && SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE).is_err()
        {
            let _ = SetProcessDPIAware();
        }
    }
}

fn monitor_under_cursor() -> HMONITOR {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
    }
}

pub fn work_area_under_cursor() -> (i32, i32, i32, i32) {
    let monitor = monitor_under_cursor();
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let r = info.rcWork;
        (r.left, r.top, (r.right - r.left).max(1), (r.bottom - r.top).max(1))
    } else {
        (0, 0, 1920, 1080)
    }
}

pub fn scale_factor_under_cursor() -> f32 {
    let monitor = monitor_under_cursor();
    unsafe {
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
            dpi_x as f32 / 96.0
        } else {
            1.0
        }
    }
}

pub fn is_hidden_or_minimized(hwnd: HWND) -> bool {
    unsafe { IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() }
}

pub fn own_window(hwnd: HWND, owner: HWND) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner.0 as isize);
    }
}

pub fn native_window(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

pub fn force_foreground_window(hwnd: HWND) {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground == hwnd {
            return;
        }
        let current_thread_id = GetCurrentThreadId();
        let foreground_thread_id = if foreground.0.is_null() { 0 } else { GetWindowThreadProcessId(foreground, None) };
        let attached = foreground_thread_id != 0
            && foreground_thread_id != current_thread_id
            && AttachThreadInput(foreground_thread_id, current_thread_id, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(foreground_thread_id, current_thread_id, false);
        }
    }
}

pub fn foreground_window_belongs_to_us() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        pid == std::process::id()
    }
}

fn message_box(title: &str, message: &str, flags: MESSAGEBOX_STYLE) {
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags);
    }
}

pub fn show_startup_error(message: &str) {
    message_box("Ports Launcher", message, MB_OK | MB_ICONERROR);
}

#[cfg(debug_assertions)]
pub fn show_info(title: &str, message: &str) {
    message_box(title, message, MB_OK);
}

fn exe_path_wide() -> Option<Vec<u16>> {
    let exe_path = std::env::current_exe().ok()?;
    Some(exe_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect())
}

fn cached_window_icons() -> Option<(HICON, HICON)> {
    thread_local! {
        static ICONS: OnceCell<Option<(HICON, HICON)>> = const { OnceCell::new() };
    }
    fn load() -> Option<(HICON, HICON)> {
        let wide = exe_path_wide()?;
        unsafe {
            let mut large_icon = HICON::default();
            let mut small_icon = HICON::default();
            let extracted = ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large_icon), Some(&mut small_icon), 1);
            if extracted == 0 {
                return None;
            }
            Some((large_icon, small_icon))
        }
    }
    ICONS.with(|cell| *cell.get_or_init(load))
}

pub fn apply_window_icon(hwnd: HWND) {
    let Some((large_icon, small_icon)) = cached_window_icons() else { return };
    unsafe {
        if !large_icon.is_invalid() {
            let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(large_icon.0 as isize)));
        }
        if !small_icon.is_invalid() {
            let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(small_icon.0 as isize)));
        }
    }
}

pub fn enable_dark_context_menus() {
    unsafe {
        let Ok(module) = LoadLibraryA(PCSTR(c"uxtheme.dll".as_ptr() as *const u8)) else { return };
        let Some(proc) = GetProcAddress(module, PCSTR(135usize as *const u8)) else { return };
        let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(proc);
        set_preferred_app_mode(1);
    }
}

pub fn extract_app_icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let wide = exe_path_wide()?;
    unsafe {
        let mut icon = HICON::default();
        if ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut icon), None, 1) == 0 || icon.is_invalid() {
            return None;
        }

        let mut info = ICONINFO::default();
        if GetIconInfo(icon, &mut info).is_err() {
            let _ = DestroyIcon(icon);
            return None;
        }
        let cleanup = move || {
            let _ = DestroyIcon(icon);
            let _ = DeleteObject(info.hbmColor.into());
            let _ = DeleteObject(info.hbmMask.into());
        };
        if info.hbmColor.is_invalid() {
            cleanup();
            return None;
        }

        let mut bitmap = BITMAP::default();
        if GetObjectW(info.hbmColor.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bitmap as *mut _ as *mut _)) == 0 {
            cleanup();
            return None;
        }
        let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);

        let mut buffer = vec![0u8; (width * height * 4).max(0) as usize];
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width;
        bmi.bmiHeader.biHeight = -height;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let hdc = GetDC(None);
        let copied = GetDIBits(hdc, info.hbmColor, 0, height as u32, Some(buffer.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS) != 0;
        ReleaseDC(None, hdc);
        cleanup();
        if !copied {
            return None;
        }

        for px in buffer.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        Some((buffer, width as u32, height as u32))
    }
}

pub fn force_normal_window_visibility(hwnd: HWND) {
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = (ex_style | WS_EX_APPWINDOW.0 as isize) & !(WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
}

pub fn begin_window_drag(hwnd: HWND) {
    unsafe {
        let _ = ReleaseCapture();
        let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(0)));
        let _ = PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), LPARAM(0));
    }
}

pub fn double_click_time_ms() -> u32 {
    unsafe { GetDoubleClickTime() }
}

pub fn register_desktop_entry() {}
