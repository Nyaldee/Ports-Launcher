
use super::dialogs::{open_message_dialog, tr};
use super::events::{lock, AppEvent};
use super::state::AppState;
use crate::ui::gamepad_router::GamepadRouter;
use crate::Tr;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(target_os = "windows")]
const UPDATER_NAME: &str = "ports_launcher_updater.bat";
#[cfg(target_os = "linux")]
const UPDATER_NAME: &str = "ports_launcher_updater.sh";

pub(crate) fn launch_self_update(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let updater = crate::base_dir().join(UPDATER_NAME);
    match crate::core::launch::launch(&updater) {
        Ok(_) => {
            let mut state = app.state.borrow_mut();
            state.mark_launcher_update_check();
            state.set_launcher_update_available(false);
            let _ = slint::quit_event_loop();
        }
        Err(e) => open_message_dialog(app, router, &tr!(app).invoke_dialog_title_update_error(), &e.to_string()),
    }
}

pub(crate) fn start_self_update_check(app: &Rc<AppState>) {
    let github_token = {
        let mut state = app.state.borrow_mut();
        if !state.should_check_launcher_update() {
            return;
        }
        state.mark_launcher_update_check();
        state.github_token.clone()
    };
    let events = app.events.clone();
    std::thread::spawn(move || {
        let result = crate::core::github_api::fetch_latest_tag_and_date(crate::SELF_REPO, github_token.as_deref());
        match result {
            Ok((latest_tag, _)) if crate::core::version::tag_is_newer(&latest_tag, crate::core::version::APP_VERSION) => {
                lock(&events).push(AppEvent::SelfUpdateAvailable)
            }
            Ok(_) => {}
            Err(e) => eprintln!("[self update check] {}", e.message()),
        }
    });
}

pub(crate) fn start_catalog_sync(app: &Rc<AppState>) {
    let known_etag = {
        let state = app.state.borrow();
        if !state.should_check_catalog() {
            return;
        }
        state.last_catalog_etag.clone()
    };
    let ports_json_path = app.paths.config_dir.join("ports.json");
    let ports_local_json_path = app.paths.config_dir.join("ports.local.json");
    let events = app.events.clone();
    std::thread::spawn(move || match crate::core::catalog_sync::fetch_ports_if_changed(&known_etag) {
        Ok(crate::core::catalog_sync::CatalogUpdate::NotModified) => {
            lock(&events).push(AppEvent::PortsCheckDone { etag: known_etag });
        }
        Ok(crate::core::catalog_sync::CatalogUpdate::Updated { text, etag }) => {
            let remote_ports = crate::core::config::parse_catalog(&text).unwrap_or_default();
            let ports = crate::core::config::merge_local_catalog(remote_ports, crate::core::config::load_local_config(&ports_local_json_path));
            let _ = std::fs::write(&ports_json_path, &text);
            let mut events = lock(&events);
            events.push(AppEvent::PortsCheckDone { etag });
            events.push(AppEvent::RemoteCatalogFetched(ports));
        }
        Err(e) => eprintln!("[catalog sync] {e}"),
    });
}

pub(crate) fn start_themes_sync(app: &Rc<AppState>) {
    let known_etag = {
        let state = app.state.borrow();
        if !state.should_check_themes() {
            return;
        }
        state.last_themes_etag.clone()
    };
    let themes_path = app.paths.themes_path.clone();
    let events = app.events.clone();
    std::thread::spawn(move || match crate::core::catalog_sync::fetch_themes_if_changed(&known_etag) {
        Ok(crate::core::catalog_sync::CatalogUpdate::NotModified) => {
            lock(&events).push(AppEvent::ThemesCheckDone { etag: known_etag });
        }
        Ok(crate::core::catalog_sync::CatalogUpdate::Updated { text, etag }) => {
            let _ = std::fs::write(&themes_path, &text);
            let mut events = lock(&events);
            events.push(AppEvent::ThemesCheckDone { etag });
            events.push(AppEvent::RemoteThemesFetched);
        }
        Err(e) => eprintln!("[themes sync] {e}"),
    });
}
