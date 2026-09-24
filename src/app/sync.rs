
use super::dialogs::{open_message_dialog, tr};
use super::events::{lock, AppEvent};
use super::state::AppState;
use crate::core::catalog_sync::{self, CatalogUpdate};
use crate::core::version::{is_newer_date, APP_VERSION, SELF_REPO};
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
    match crate::core::launch::launch(&app.paths.config_dir.join(UPDATER_NAME)) {
        Ok(_) => {
            let mut state = app.state.borrow_mut();
            state.mark_launcher_update_check();
            state.set_launcher_update_available(false);
            let _ = slint::quit_event_loop();
        }
        Err(e) => open_message_dialog(app, router, &tr!(app).invoke_dialog_title_update_error(), &e.to_string()),
    }
}

pub(crate) fn start_self_update_check(app: &AppState) {
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
        if let Ok((_, Some(latest_date))) = crate::core::github_api::fetch_latest_tag_and_date(SELF_REPO, github_token.as_deref()) {
            if is_newer_date(&latest_date, APP_VERSION) {
                lock(&events).push(AppEvent::SelfUpdateAvailable);
            }
        }
    });
}

pub(crate) fn start_catalog_sync(app: &AppState) {
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
    std::thread::spawn(move || match catalog_sync::fetch_ports_if_changed(&known_etag) {
        Ok(CatalogUpdate::NotModified) => lock(&events).push(AppEvent::PortsCheckDone { etag: known_etag }),
        Ok(CatalogUpdate::Updated { text, etag }) => {
            let Ok(remote_ports) = crate::core::config::parse_catalog(&text) else { return };
            let ports = crate::core::config::merge_local_catalog(remote_ports, crate::core::config::load_local_config(&ports_local_json_path));
            let _ = crate::core::files::write_atomic(&ports_json_path, text.as_bytes());
            let mut events = lock(&events);
            events.push(AppEvent::PortsCheckDone { etag });
            events.push(AppEvent::RemoteCatalogFetched(ports));
        }
        Err(_) => {}
    });
}

pub(crate) fn start_themes_sync(app: &AppState) {
    let known_etag = {
        let state = app.state.borrow();
        if !state.should_check_themes() {
            return;
        }
        state.last_themes_etag.clone()
    };
    let themes_path = app.paths.themes_path.clone();
    let events = app.events.clone();
    std::thread::spawn(move || match catalog_sync::fetch_themes_if_changed(&known_etag) {
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
