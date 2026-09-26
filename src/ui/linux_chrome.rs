
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, MapState, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeWindow {
    X11(u32),
    Wayland,
}

pub fn native_window(window: &slint::Window) -> Option<NativeWindow> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Xcb(h) => Some(NativeWindow::X11(h.window.get())),
        RawWindowHandle::Xlib(h) => Some(NativeWindow::X11(h.window as u32)),
        RawWindowHandle::Wayland(_) => Some(NativeWindow::Wayland),
        _ => None,
    }
}

fn instance_socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|dir| dir.is_dir()) {
        Some(dir) => dir.join("ports_launcher.sock"),
        None => std::env::temp_dir().join(format!("ports_launcher-{}.sock", std::env::var("USER").unwrap_or_default())),
    }
}

pub fn claim_single_instance(on_activate: impl Fn() + Send + 'static) -> bool {
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = instance_socket_path();
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(&[0]);
        return false;
    }
    let _ = fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return true };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
            on_activate();
        }
    });
    true
}

struct Session {
    conn: RustConnection,
    root: u32,
    screen_width: i32,
    screen_height: i32,
}

thread_local! {
    static SESSION: OnceCell<Option<Session>> = const { OnceCell::new() };
    static ATOMS: RefCell<HashMap<&'static str, u32>> = RefCell::new(HashMap::new());
    static ICON: OnceCell<Option<(Vec<u8>, u32, u32)>> = const { OnceCell::new() };
}

fn with_conn<T>(f: impl FnOnce(&Session) -> T) -> Option<T> {
    SESSION.with(|cell| {
        cell.get_or_init(|| {
            let (conn, screen_num) = x11rb::connect(None).ok()?;
            let screen = &conn.setup().roots[screen_num];
            let (root, screen_width, screen_height) = (screen.root, screen.width_in_pixels as i32, screen.height_in_pixels as i32);
            Some(Session { conn, root, screen_width, screen_height })
        })
        .as_ref()
        .map(f)
    })
}

fn cached_atom(conn: &RustConnection, name: &'static str) -> Option<u32> {
    if let Some(cached) = ATOMS.with(|c| c.borrow().get(name).copied()) {
        return Some(cached);
    }
    let value = conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom;
    ATOMS.with(|c| c.borrow_mut().insert(name, value));
    Some(value)
}

fn send_root_message(conn: &RustConnection, root: u32, window: u32, message_type: u32, data: [u32; 5]) -> Option<()> {
    let event = ClientMessageEvent::new(32, window, message_type, data);
    let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT;
    conn.send_event(false, root, mask, event).ok()?;
    conn.flush().ok()
}

fn set_property32(window: u32, property: &'static str, kind: AtomEnum, data: &[u32]) {
    with_conn(|s| -> Option<()> {
        let atom = cached_atom(&s.conn, property)?;
        s.conn.change_property32(PropMode::REPLACE, window, atom, kind, data).ok()?;
        s.conn.flush().ok()
    });
}

pub fn force_foreground_window(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    with_conn(|s| -> Option<()> {
        let atom = cached_atom(&s.conn, "_NET_ACTIVE_WINDOW")?;
        send_root_message(&s.conn, s.root, win, atom, [1, 0, 0, 0, 0])
    });
}

pub fn foreground_window_belongs_to_us() -> bool {
    with_conn(|s| -> Option<bool> {
        let active_atom = cached_atom(&s.conn, "_NET_ACTIVE_WINDOW")?;
        let reply = s.conn.get_property(false, s.root, active_atom, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        let active = reply.value32()?.next()?;
        let pid_atom = cached_atom(&s.conn, "_NET_WM_PID")?;
        let reply = s.conn.get_property(false, active, pid_atom, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
        let pid = reply.value32()?.next();
        Some(pid == Some(std::process::id()))
    })
    .flatten()
    .unwrap_or(false)
}

pub fn show_startup_error(message: &str) {
    use std::process::Command;
    let title = "Ports Launcher";
    let shown = |mut cmd: Command| cmd.status().is_ok_and(|s| s.success());
    if shown({
        let mut c = Command::new("zenity");
        c.args(["--error", "--no-markup", "--title", title, "--text", message]);
        c
    }) || shown({
        let mut c = Command::new("kdialog");
        c.args(["--title", title, "--error", message]);
        c
    }) {
        return;
    }
    eprintln!("{title}: {message}");
}

pub fn apply_window_icon(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    let Some((rgba, width, height)) = extract_app_icon_rgba() else { return };
    let mut data = Vec::with_capacity(2 + (width * height) as usize);
    data.extend([width, height]);
    data.extend(rgba.as_chunks::<4>().0.iter().map(|&[r, g, b, a]| u32::from_be_bytes([a, r, g, b])));
    set_property32(win, "_NET_WM_ICON", AtomEnum::CARDINAL, &data);
}

pub fn enable_dark_context_menus() {}

pub fn extract_app_icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    ICON.with(|cell| cell.get_or_init(decode_app_icon_rgba).clone())
}

fn decode_app_icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let ico = include_bytes!("../../Icon.ico");
    let count = u16::from_le_bytes([*ico.get(4)?, *ico.get(5)?]) as usize;
    let dimension = |b: u8| if b == 0 { 256u32 } else { b as u32 };
    let mut best: Option<(u32, u32, u32)> = None;
    for i in 0..count {
        let entry = ico.get(6 + i * 16..6 + i * 16 + 16)?;
        let area = dimension(entry[0]) * dimension(entry[1]);
        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?);
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?);
        if best.is_none_or(|(best_area, ..)| area > best_area) {
            best = Some((area, size, offset));
        }
    }
    let (_, size, offset) = best?;
    let png_bytes = ico.get(offset as usize..(offset + size) as usize)?;
    let image = image::load_from_memory(png_bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some((image.into_raw(), width, height))
}

pub fn force_normal_window_visibility(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    let Some(normal) = with_conn(|s| cached_atom(&s.conn, "_NET_WM_WINDOW_TYPE_NORMAL")).flatten() else { return };
    set_property32(win, "_NET_WM_WINDOW_TYPE", AtomEnum::ATOM, &[normal]);
}

pub fn begin_window_drag(window: &slint::Window) -> bool {
    const MOVERESIZE_MOVE: u32 = 8;
    match native_window(window) {
        Some(NativeWindow::X11(win)) => {
            with_conn(|s| -> Option<()> {
                let pointer = s.conn.query_pointer(s.root).ok()?.reply().ok()?;
                let moveresize = cached_atom(&s.conn, "_NET_WM_MOVERESIZE")?;
                send_root_message(&s.conn, s.root, win, moveresize, [pointer.root_x as u32, pointer.root_y as u32, MOVERESIZE_MOVE, 1, 1])
            });
            false
        }
        Some(NativeWindow::Wayland) => {
            use slint::winit_030::WinitWindowAccessor;
            window.with_winit_window(|winit_window| winit_window.drag_window().is_ok()).unwrap_or(false)
        }
        None => false,
    }
}

pub fn cursor_position() -> Option<(i32, i32)> {
    with_conn(|s| s.conn.query_pointer(s.root).ok()?.reply().ok().map(|r| (r.root_x as i32, r.root_y as i32))).flatten()
}

pub fn is_hidden_or_minimized(window: NativeWindow) -> bool {
    let NativeWindow::X11(win) = window else { return false };
    with_conn(|s| s.conn.get_window_attributes(win).ok()?.reply().ok().map(|a| a.map_state != MapState::VIEWABLE)).flatten().unwrap_or(false)
}

pub fn own_window(window: NativeWindow, owner: NativeWindow) {
    let (NativeWindow::X11(win), NativeWindow::X11(owner_win)) = (window, owner) else { return };
    set_property32(win, "WM_TRANSIENT_FOR", AtomEnum::WINDOW, &[owner_win]);
}

pub fn double_click_time_ms() -> u32 {
    500
}

pub fn restore_window(window: &slint::Window) {
    match native_window(window) {
        Some(NativeWindow::X11(win)) => {
            let _ = window.show();
            window.set_minimized(false);
            force_foreground_window(NativeWindow::X11(win));
        }
        _ => {
            let fullscreen = window.is_fullscreen();
            let _ = window.hide();
            let _ = window.show();
            window.set_fullscreen(fullscreen);
        }
    }
}

pub fn set_fullscreen(window: &slint::Window, on: bool) {
    window.set_fullscreen(on);
}

pub fn default_font_family() -> String {
    String::new()
}

pub fn work_area_under_cursor() -> (i32, i32, i32, i32) {
    with_conn(|s| {
        let workarea = || -> Option<(i32, i32, i32, i32)> {
            let atom = cached_atom(&s.conn, "_NET_WORKAREA")?;
            let reply = s.conn.get_property(false, s.root, atom, AtomEnum::CARDINAL, 0, 4).ok()?.reply().ok()?;
            let mut values = reply.value32()?;
            let (x, y, w, h) = (values.next()?, values.next()?, values.next()?, values.next()?);
            (w > 0 && h > 0).then_some((x as i32, y as i32, w as i32, h as i32))
        };
        workarea().unwrap_or((0, 0, s.screen_width, s.screen_height))
    })
    .unwrap_or((0, 0, 1920, 1080))
}

pub fn scale_factor_under_cursor() -> f32 {
    1.0
}

fn xdg_data_home() -> Option<PathBuf> {
    match std::env::var("XDG_DATA_HOME") {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => std::env::var("HOME").ok().map(|home| PathBuf::from(home).join(".local").join("share")),
    }
}

fn write_if_different(path: &Path, content: &[u8]) {
    if fs::read(path).ok().as_deref() == Some(content) {
        return;
    }
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_ok() {
            let _ = fs::write(path, content);
        }
    }
}

fn desktop_exec_arg(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len() + 2);
    escaped.push('"');
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                escaped.push('\\');
                escaped.push(c);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

pub fn register_desktop_entry() {
    let Some(data_home) = xdg_data_home() else { return };
    let Ok(exe) = std::env::current_exe() else { return };

    let desktop_entry = format!(
        "[Desktop Entry]\nType=Application\nName=Ports Launcher\nExec={}\nIcon=ports_launcher\nCategories=Game;\nTerminal=false\nStartupWMClass=ports_launcher\n",
        desktop_exec_arg(&exe.to_string_lossy())
    );
    write_if_different(&data_home.join("applications").join("ports_launcher.desktop"), desktop_entry.as_bytes());

    let Some((rgba, width, height)) = extract_app_icon_rgba() else { return };
    let Some(img) = image::RgbaImage::from_raw(width, height, rgba) else { return };
    let mut png_bytes = Vec::new();
    if img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png).is_err() {
        return;
    }
    let hicolor_dir = data_home.join("icons").join("hicolor");
    let current_size = format!("{width}x{height}");
    write_if_different(&hicolor_dir.join(&current_size).join("apps").join("ports_launcher.png"), &png_bytes);
    remove_other_icon_sizes(&hicolor_dir, &current_size);
}

fn remove_other_icon_sizes(hicolor_dir: &Path, current_size: &str) {
    let Ok(entries) = fs::read_dir(hicolor_dir) else { return };
    for entry in entries.flatten().filter(|e| e.file_name().to_str() != Some(current_size)) {
        let _ = fs::remove_file(entry.path().join("apps").join("ports_launcher.png"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_exec_arg_echappe_les_caracteres_reserves() {
        assert_eq!(desktop_exec_arg("/opt/Ports Launcher/ports_launcher"), "\"/opt/Ports Launcher/ports_launcher\"");
        assert_eq!(desktop_exec_arg("/a$b/100%/c\"d"), "\"/a\\$b/100%%/c\\\"d\"");
    }
}
