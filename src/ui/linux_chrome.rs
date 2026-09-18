
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::fs;
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

struct Session {
    conn: RustConnection,
    root: u32,
    screen_width: i32,
    screen_height: i32,
}

thread_local! {
    static SESSION: OnceCell<Option<Session>> = OnceCell::new();
    static ATOMS: RefCell<HashMap<&'static str, u32>> = RefCell::new(HashMap::new());
    static ICON: OnceCell<Option<(Vec<u8>, u32, u32)>> = OnceCell::new();
}

fn with_conn<T>(f: impl FnOnce(&Session) -> T) -> Option<T> {
    SESSION.with(|cell| {
        let session = cell.get_or_init(|| {
            let (conn, screen_num) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots[screen_num].root;
            let screen_width = conn.setup().roots[screen_num].width_in_pixels as i32;
            let screen_height = conn.setup().roots[screen_num].height_in_pixels as i32;
            Some(Session { conn, root, screen_width, screen_height })
        });
        session.as_ref().map(f)
    })
}

fn intern(conn: &RustConnection, name: &str) -> Option<u32> {
    conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom)
}

fn cached_atom(conn: &RustConnection, name: &'static str) -> Option<u32> {
    if let Some(cached) = ATOMS.with(|c| c.borrow().get(name).copied()) {
        return Some(cached);
    }
    let value = intern(conn, name)?;
    ATOMS.with(|c| c.borrow_mut().insert(name, value));
    Some(value)
}

fn send_root_message(conn: &RustConnection, root: u32, window: u32, message_type: u32, data: [u32; 5]) -> Option<()> {
    let event = ClientMessageEvent::new(32, window, message_type, data);
    let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT;
    conn.send_event(false, root, mask, &event).ok()?;
    conn.flush().ok()?;
    Some(())
}

fn active_window(conn: &RustConnection, root: u32) -> Option<u32> {
    let atom = cached_atom(conn, "_NET_ACTIVE_WINDOW")?;
    let reply = conn.get_property(false, root, atom, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
    reply.value32().and_then(|mut it| it.next())
}

pub fn force_foreground_window(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    with_conn(|s| {
        (|| -> Option<()> {
            let atom = cached_atom(&s.conn, "_NET_ACTIVE_WINDOW")?;
            send_root_message(&s.conn, s.root, win, atom, [1, 0, 0, 0, 0])
        })();
    });
}

pub fn foreground_window_belongs_to_us() -> bool {
    with_conn(|s| -> Option<bool> {
        let active = active_window(&s.conn, s.root)?;
        let pid_atom = cached_atom(&s.conn, "_NET_WM_PID")?;
        let reply = s.conn.get_property(false, active, pid_atom, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
        Some(reply.value32().and_then(|mut it| it.next()) == Some(std::process::id()))
    })
    .flatten()
    .unwrap_or(false)
}

fn message_box(title: &str, message: &str) {
    use std::process::Command;
    if matches!(Command::new("zenity").args(["--error", "--title", title, "--text", message]).status(), Ok(s) if s.success())
    {
        return;
    }
    if matches!(Command::new("kdialog").args(["--title", title, "--error", message]).status(), Ok(s) if s.success()) {
        return;
    }
    eprintln!("{title}: {message}");
}

pub fn show_startup_error(message: &str) {
    message_box("Ports Launcher", message);
}

#[cfg(debug_assertions)]
pub fn show_info(title: &str, message: &str) {
    message_box(title, message);
}

pub fn apply_window_icon(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    let Some((rgba, width, height)) = extract_app_icon_rgba() else { return };
    let mut data = Vec::with_capacity(2 + (width * height) as usize);
    data.push(width);
    data.push(height);
    data.extend(rgba.chunks_exact(4).map(|px| {
        let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
        (a << 24) | (r << 16) | (g << 8) | b
    }));
    with_conn(|s| {
        (|| -> Option<()> {
            let atom = cached_atom(&s.conn, "_NET_WM_ICON")?;
            s.conn.change_property32(PropMode::REPLACE, win, atom, AtomEnum::CARDINAL, &data).ok()?;
            s.conn.flush().ok()?;
            Some(())
        })();
    });
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
        let (w, h) = (dimension(entry[0]), dimension(entry[1]));
        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?);
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?);
        if best.map(|(area, ..)| w * h > area).unwrap_or(true) {
            best = Some((w * h, size, offset));
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
    with_conn(|s| {
        (|| -> Option<()> {
            let wtype = cached_atom(&s.conn, "_NET_WM_WINDOW_TYPE")?;
            let normal = cached_atom(&s.conn, "_NET_WM_WINDOW_TYPE_NORMAL")?;
            s.conn.change_property32(PropMode::REPLACE, win, wtype, AtomEnum::ATOM, &[normal]).ok()?;
            s.conn.flush().ok()?;
            Some(())
        })();
    });
}

pub fn begin_window_drag(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    with_conn(|s| {
        let Some(pointer) = s.conn.query_pointer(s.root).ok().and_then(|c| c.reply().ok()) else { return };
        let Some(moveresize) = cached_atom(&s.conn, "_NET_WM_MOVERESIZE") else { return };
        const MOVERESIZE_MOVE: u32 = 8;
        let data = [pointer.root_x as u32, pointer.root_y as u32, MOVERESIZE_MOVE, 1, 1];
        let _ = send_root_message(&s.conn, s.root, win, moveresize, data);
    });
}

pub fn cursor_position() -> Option<(i32, i32)> {
    with_conn(|s| s.conn.query_pointer(s.root).ok()?.reply().ok().map(|r| (r.root_x as i32, r.root_y as i32))).flatten()
}

pub fn is_hidden_or_minimized(window: NativeWindow) -> bool {
    let NativeWindow::X11(win) = window else { return false };
    with_conn(|s| s.conn.get_window_attributes(win).ok()?.reply().ok().map(|a| a.map_state != MapState::VIEWABLE))
        .flatten()
        .unwrap_or(false)
}

pub fn own_window(window: NativeWindow, owner: NativeWindow) {
    let (NativeWindow::X11(win), NativeWindow::X11(owner_win)) = (window, owner) else { return };
    with_conn(|s| {
        let Some(atom) = cached_atom(&s.conn, "WM_TRANSIENT_FOR") else { return };
        let _ = s.conn.change_property32(PropMode::REPLACE, win, atom, AtomEnum::WINDOW, &[owner_win]);
        let _ = s.conn.flush();
    });
}

pub fn double_click_time_ms() -> u32 {
    500
}

pub fn enable_dpi_awareness() {}

pub fn work_area_under_cursor() -> (i32, i32, i32, i32) {
    with_conn(|s| {
        let resolved: Option<(i32, i32, i32, i32)> = (|| {
            let atom = cached_atom(&s.conn, "_NET_WORKAREA")?;
            let reply = s.conn.get_property(false, s.root, atom, AtomEnum::CARDINAL, 0, 4).ok()?.reply().ok()?;
            let mut values = reply.value32()?;
            let (x, y, w, h) = (values.next(), values.next(), values.next(), values.next());
            match (x, y, w, h) {
                (Some(x), Some(y), Some(w), Some(h)) if w > 0 && h > 0 => Some((x as i32, y as i32, w as i32, h as i32)),
                _ => None,
            }
        })();
        resolved.unwrap_or((0, 0, s.screen_width, s.screen_height))
    })
    .unwrap_or((0, 0, 1920, 1080))
}

pub fn scale_factor_under_cursor() -> f32 {
    1.0
}

fn xdg_data_home() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    std::env::var("HOME").ok().map(|home| PathBuf::from(home).join(".local").join("share"))
}

fn write_if_different(path: &Path, content: &[u8]) {
    if fs::read(path).ok().as_deref() == Some(content) {
        return;
    }
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_ok() {
        let _ = fs::write(path, content);
    }
}

pub fn register_desktop_entry() {
    let Some(data_home) = xdg_data_home() else { return };
    let Ok(exe) = std::env::current_exe() else { return };

    let exe_str = exe.display().to_string();
    let exec = if exe_str.contains(' ') { format!("\"{exe_str}\"") } else { exe_str };
    let desktop_entry = format!(
        "[Desktop Entry]\nType=Application\nName=Ports Launcher\nExec={exec}\nIcon=ports_launcher\nCategories=Game;\nTerminal=false\nStartupWMClass=ports_launcher\n"
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
    let icon_path = hicolor_dir.join(&current_size).join("apps").join("ports_launcher.png");
    write_if_different(&icon_path, &png_bytes);
    remove_stale_icon_sizes(&hicolor_dir, &current_size);
}

fn remove_stale_icon_sizes(hicolor_dir: &Path, current_size: &str) {
    let Ok(entries) = fs::read_dir(hicolor_dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_str() == Some(current_size) {
            continue;
        }
        let stale = entry.path().join("apps").join("ports_launcher.png");
        let _ = fs::remove_file(stale);
    }
}
