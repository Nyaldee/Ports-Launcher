
use std::cell::OnceCell;
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, POINT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, GetObjectW, MonitorFromPoint, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::System::Threading::{AttachThreadInput, CreateEventW, GetCurrentThreadId, SetEvent, WaitForSingleObject, INFINITE};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, ReleaseCapture};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, DestroyIcon, GetCursorPos, GetForegroundWindow, GetIconInfo, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, MessageBoxW, PostMessageW, SendMessageW, SetForegroundWindow,
    SetWindowLongPtrW, ShowWindow, ASFW_ANY, GWLP_HWNDPARENT, GWL_EXSTYLE, HICON, HTCAPTION, ICONINFO, ICON_BIG,
    ICON_SMALL, MB_ICONERROR, MB_OK, SW_HIDE, SW_SHOWNA, WM_LBUTTONUP, WM_NCLBUTTONDOWN, WM_SETICON, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn claim_single_instance(on_activate: impl Fn() + Send + 'static) -> bool {
    let name = to_wide("Local\\PortsLauncher.Activate");
    let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }) else { return true };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
        return false;
    }
    let raw_event = event.0 as isize;
    std::thread::spawn(move || {
        let event = HANDLE(raw_event as *mut _);
        while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
            on_activate();
        }
    });
    true
}

fn monitor_under_cursor() -> HMONITOR {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
    }
}

pub fn work_area_under_cursor() -> (i32, i32, i32, i32) {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if unsafe { GetMonitorInfoW(monitor_under_cursor(), &mut info) }.as_bool() {
        let r = info.rcWork;
        (r.left, r.top, (r.right - r.left).max(1), (r.bottom - r.top).max(1))
    } else {
        (0, 0, 1920, 1080)
    }
}

pub fn scale_factor_under_cursor() -> f32 {
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    let ok = unsafe { GetDpiForMonitor(monitor_under_cursor(), MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.is_ok();
    if ok && dpi_x > 0 {
        dpi_x as f32 / 96.0
    } else {
        1.0
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
    match window.window_handle().window_handle().ok()?.as_raw() {
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
        if SetForegroundWindow(hwnd).as_bool() {
            let _ = BringWindowToTop(hwnd);
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

pub fn show_startup_error(message: &str) {
    let title = to_wide("Ports Launcher");
    let text = to_wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
    }
}

fn exe_path_wide() -> Option<Vec<u16>> {
    Some(to_wide(&std::env::current_exe().ok()?.to_string_lossy()))
}

fn cached_window_icons() -> Option<(HICON, HICON)> {
    thread_local! {
        static ICONS: OnceCell<Option<(HICON, HICON)>> = const { OnceCell::new() };
    }
    fn load() -> Option<(HICON, HICON)> {
        let wide = exe_path_wide()?;
        let (mut large_icon, mut small_icon) = (HICON::default(), HICON::default());
        let extracted = unsafe { ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large_icon), Some(&mut small_icon), 1) };
        (extracted != 0).then_some((large_icon, small_icon))
    }
    ICONS.with(|cell| *cell.get_or_init(load))
}

pub fn apply_window_icon(hwnd: HWND) {
    let Some((large_icon, small_icon)) = cached_window_icons() else { return };
    for (kind, icon) in [(ICON_BIG, large_icon), (ICON_SMALL, small_icon)] {
        if !icon.is_invalid() {
            unsafe {
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(icon.0 as isize)));
            }
        }
    }
}

pub fn enable_dark_context_menus() {
    const SET_PREFERRED_APP_MODE_ORDINAL: usize = 135;
    const ALLOW_DARK: i32 = 1;
    unsafe {
        let Ok(module) = LoadLibraryA(PCSTR(c"uxtheme.dll".as_ptr() as *const u8)) else { return };
        let Some(proc) = GetProcAddress(module, PCSTR(SET_PREFERRED_APP_MODE_ORDINAL as *const u8)) else { return };
        let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(proc);
        set_preferred_app_mode(ALLOW_DARK);
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
        let cleanup = || {
            let _ = DestroyIcon(icon);
            let _ = DeleteObject(info.hbmColor.into());
            let _ = DeleteObject(info.hbmMask.into());
        };

        let mut bitmap = BITMAP::default();
        if info.hbmColor.is_invalid()
            || GetObjectW(info.hbmColor.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bitmap as *mut _ as *mut _)) == 0
        {
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

pub fn begin_window_drag(window: &slint::Window) -> bool {
    let Some(hwnd) = native_window(window) else { return false };
    unsafe {
        let _ = ReleaseCapture();
        let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(0)));
        let _ = PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), LPARAM(0));
    }
    false
}

pub fn restore_window(window: &slint::Window) {
    let _ = window.show();
    window.set_minimized(false);
    if let Some(native) = native_window(window) {
        force_foreground_window(native);
    }
}

pub fn set_fullscreen(_window: &slint::Window, _on: bool) {}

pub fn default_font_family() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut metrics = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, metrics.cbSize, Some(&mut metrics as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    }
    .is_ok();
    let face = &metrics.lfMessageFont.lfFaceName;
    let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
    let family = String::from_utf16_lossy(&face[..len]);
    if ok && !family.is_empty() {
        family
    } else {
        "Segoe UI".to_string()
    }
}

pub fn double_click_time_ms() -> u32 {
    unsafe { GetDoubleClickTime() }
}

pub fn register_desktop_entry() {}

#[cfg(test)]
mod tests {
    #[test]
    fn default_font_family_renvoie_une_police() {
        let family = super::default_font_family();
        println!("police système : {family}");
        assert!(!family.is_empty());
    }
}
