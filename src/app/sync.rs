use super::dialogs::{open_message_dialog, tr};
use super::events::{lock, AppEvent};
use super::state::AppState;
use crate::core::catalog_sync::{self, CatalogUpdate};
use crate::core::version::{is_newer_date, APP_VERSION};
use crate::ui::gamepad_router::GamepadRouter;
use crate::Tr;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(target_os = "windows")]
const UPDATER_NAME: &str = "ports_launcher_updater.bat";
#[cfg(target_os = "linux")]
const UPDATER_NAME: &str = "ports_launcher_updater.sh";

#[cfg(target_os = "windows")]
const UPDATE_PLATFORM: &str = "windows";
#[cfg(target_os = "linux")]
const UPDATE_PLATFORM: &str = "linux";

pub(crate) fn launch_self_update(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    match crate::core::launch::launch(&app.paths.config_dir.join(UPDATER_NAME)) {
        Ok(_) => {
            let mut state = app.state.borrow_mut();
            state.mark_launcher_update_check();
            state.set_launcher_update_available(false);
            state.reset_catalog_check();
            state.reset_themes_check();
            let _ = slint::quit_event_loop();
        }
        Err(e) => open_message_dialog(app, router, &tr!(app).invoke_dialog_title_update_error(), &e.to_string()),
    }
}

pub(crate) fn start_self_update_check(app: &AppState) {
    {
        let mut state = app.state.borrow_mut();
        if !state.should_check_launcher_update() {
            return;
        }
        state.mark_launcher_update_check();
    }
    let events = app.events.clone();
    std::thread::spawn(move || {
        if let Ok(latest_date) = catalog_sync::fetch_latest_build_date(UPDATE_PLATFORM) {
            lock(&events).push(AppEvent::SelfUpdateChecked { available: is_newer_date(&latest_date, APP_VERSION) });
        }
    });
}

pub(crate) fn start_catalog_sync(app: &AppState, force: bool) {
    let (url, known_etag) = {
        let mut state = app.state.borrow_mut();
        let url = state.catalog_url.clone().unwrap_or_else(|| catalog_sync::DEFAULT_CATALOG_URL.to_string());
        state.note_catalog_url(&url);
        if !force && !state.should_check_catalog() {
            return;
        }
        (url, state.last_catalog_etag.clone())
    };
    let catalog_path = app.paths.config_dir.join(crate::core::config::CATALOG_FILE);
    let local_catalog_path = app.paths.config_dir.join(crate::core::config::LOCAL_CATALOG_FILE);
    let events = app.events.clone();
    std::thread::spawn(move || match catalog_sync::fetch_catalog_if_changed(&url, &known_etag) {
        Ok(CatalogUpdate::NotModified) => lock(&events).push(AppEvent::PortsCheckDone { etag: known_etag }),
        Ok(CatalogUpdate::Updated { text, etag }) => {
            let Ok(remote_ports) = crate::core::config::parse_catalog(&text) else { return };
            let ports = crate::core::config::merge_local_catalog(remote_ports, crate::core::config::load_local_config(&local_catalog_path));
            let _ = crate::core::files::write_atomic(&catalog_path, text.as_bytes());
            let mut events = lock(&events);
            events.push(AppEvent::PortsCheckDone { etag });
            events.push(AppEvent::RemoteCatalogFetched(ports));
        }
        Err(_) => {}
    });
}

pub(crate) fn start_announcement_refresh(app: &AppState) {
    let url = {
        let state = app.state.borrow();
        if !state.announcements_enabled {
            return;
        }
        state.announcement_url.clone().unwrap_or_else(|| catalog_sync::DEFAULT_ANNOUNCEMENT_URL.to_string())
    };
    let events = app.events.clone();
    std::thread::spawn(move || {
        if let Ok(announcement) = catalog_sync::fetch_announcement(&url) {
            lock(&events).push(AppEvent::AnnouncementFetched(announcement));
        }
    });
}

pub(crate) fn start_themes_sync(app: &AppState, force: bool) {
    let (url, known_etag) = {
        let mut state = app.state.borrow_mut();
        let url = state.themes_url.clone().unwrap_or_else(|| catalog_sync::DEFAULT_THEMES_URL.to_string());
        state.note_themes_url(&url);
        if !force && !state.should_check_themes() {
            return;
        }
        (url, state.last_themes_etag.clone())
    };
    let themes_path = app.paths.themes_path.clone();
    let events = app.events.clone();
    std::thread::spawn(move || match catalog_sync::fetch_themes_if_changed(&url, &known_etag) {
        Ok(CatalogUpdate::NotModified) => lock(&events).push(AppEvent::ThemesCheckDone { etag: known_etag }),
        Ok(CatalogUpdate::Updated { text, etag }) => {
            let _ = crate::core::files::write_atomic(&themes_path, text.as_bytes());
            let mut events = lock(&events);
            events.push(AppEvent::ThemesCheckDone { etag });
            events.push(AppEvent::RemoteThemesFetched);
        }
        Err(_) => {}
    });
}
