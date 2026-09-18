
use super::dialogs::{
    dialog_is_open, open_error_dialog, open_message_dialog, open_progress_dialog, open_update_toggle_dialog, tr,
};
use super::events::{lock, AppEvent};
use super::playtime::is_port_running;
use super::state::AppState;
use crate::core::jobs::InstallOutcome;
use crate::core::models::{Port, SourceType};
use crate::core::executable_detect::ExecutableSelectionError;
use crate::ui::gamepad_router::GamepadRouter;
use crate::Tr;
use serde_json::Value;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

pub(crate) fn open_path_if_exists(path: &Path) {
    if path.exists() {
        crate::core::launch::open_path(path);
    }
}

fn try_claim_op_slot(app: &Rc<AppState>, port: &Port) -> Option<String> {
    let key = port.key().to_string();
    if app.install_runtime.installing.borrow().contains(&key) || is_port_running(app, &key) {
        return None;
    }
    app.install_runtime.installing.borrow_mut().insert(key.clone());
    Some(key)
}

pub(crate) fn start_install(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    port: Port,
    asset_override: Option<Value>,
    release_override: Option<Value>,
) {
    let Some(key) = try_claim_op_slot(app, &port) else { return };

    let window = app.window();
    let tr = window.global::<crate::Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_installing(), &tr.invoke_progress_installing(port.name.clone().into()));

    if app.stress_test {
        let Ok(dest_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else { return };
        let _ = std::fs::create_dir_all(&dest_dir);
        let _ = std::fs::write(dest_dir.join("game.exe"), b"not a real executable -- visual stress test");
        lock(&app.events).push(AppEvent::InstallDone { key, tag: Some("v1.0.0".to_string()), pin_version: false });
        return;
    }

    let pin_version = release_override.is_some();
    let (github_token, gitlab_token) = app.api_tokens();
    let library_dir = app.paths.library_dir.clone();
    let cache_dir = app.paths.cache_dir.clone();
    let saves_backup_dir = app.paths.saves_backup_dir.clone();
    let events = app.events.clone();
    let progress_key = key.clone();

    std::thread::spawn(move || {
        let events_progress = events.clone();
        let mut on_progress = move |message: &str| {
            lock(&events_progress).push(AppEvent::InstallProgress { message: message.to_string() });
        };
        let overrides = crate::core::installer::InstallOverrides { asset: asset_override.as_ref(), release: release_override.as_ref() };
        let paths = crate::core::installer::InstallPaths { library_dir: &library_dir, cache_dir: &cache_dir, saves_backup_dir: &saves_backup_dir };
        let outcome = crate::core::jobs::run_install(&port, paths, github_token.as_deref(), gitlab_token.as_deref(), overrides, &mut on_progress);
        let event = match outcome {
            InstallOutcome::Done { tag } => AppEvent::InstallDone { key: progress_key, tag, pin_version },
            InstallOutcome::AssetAmbiguous { assets } => AppEvent::InstallAssetAmbiguous { key: progress_key, assets, release_override },
            InstallOutcome::Error(message) => AppEvent::InstallError { key: progress_key, message },
        };
        lock(&events).push(event);
    });
}

pub(crate) fn open_version_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let Some(key) = try_claim_op_slot(app, &port) else { return };
    let window = app.window();
    let tr = window.global::<crate::Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_loading(), &tr.invoke_progress_fetching_versions(port.name.clone().into()));

    let (github_token, gitlab_token) = app.api_tokens();
    let events = app.events.clone();
    let repo = port.repo.clone().unwrap_or_default();
    let source_type = port.source_type;

    std::thread::spawn(move || {
        let result = match source_type {
            SourceType::Github => crate::core::github_api::list_releases(&repo, github_token.as_deref(), 3).map_err(|e| e.message().to_string()),
            SourceType::Gitlab => crate::core::gitlab_api::list_releases(&repo, gitlab_token.as_deref(), 3).map_err(|e| e.message().to_string()),
            SourceType::DirectUrl | SourceType::Local => Err("This source has no version history.".to_string()),
        };
        let event = match result {
            Ok(releases) => AppEvent::VersionsFetched { key, releases },
            Err(message) => AppEvent::VersionsFetchError { key, message },
        };
        lock(&events).push(event);
    });
}

pub(crate) fn start_extra_install(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let Some(key) = try_claim_op_slot(app, &port) else { return };

    let window = app.window();
    let tr = window.global::<crate::Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_extras(), &tr.invoke_progress_installing_extras(port.name.clone().into()));

    let library_dir = app.paths.library_dir.clone();
    let events = app.events.clone();

    std::thread::spawn(move || {
        let events_progress = events.clone();
        let mut on_progress = move |message: &str| {
            lock(&events_progress).push(AppEvent::InstallProgress { message: message.to_string() });
        };
        let result = crate::core::jobs::run_extra_install(&port, &library_dir, &mut on_progress);
        lock(&events).push(AppEvent::ExtraInstallDone { key, result });
    });
}

pub(crate) fn open_favorite_exe_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let Ok(game_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else { return };
    if !game_dir.exists() {
        return;
    }
    let candidates: Vec<PathBuf> = match crate::core::executable_detect::autodetect_executable(&game_dir) {
        Ok(single) => vec![single],
        Err(ExecutableSelectionError::Ambiguous(_, candidates)) => candidates,
        Err(ExecutableSelectionError::Message(message)) => {
            open_message_dialog(app, router, &tr!(app).invoke_dialog_title_no_executable_found(), &message);
            return;
        }
    };
    let mut labels = vec![tr!(app).invoke_picker_ask_every_time().to_string()];
    labels.extend(candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()));
    let key = port.key().to_string();
    let game_dir_owned = game_dir.clone();
    super::dialogs::open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_favorite_executable(), labels, move |app, _router, idx| {
        let exe = if idx == 0 {
            None
        } else {
            candidates.get(idx - 1).and_then(|p| p.strip_prefix(&game_dir_owned).ok()).map(|p| p.to_string_lossy().to_string())
        };
        app.state.borrow_mut().set_favorite_exe(&key, exe);
    });
}

pub(crate) fn repair_missing_cached_image(app: &Rc<AppState>, port: &Port) {
    let Some(url) = port.image.clone() else { return };
    let Ok(dest) = crate::core::image_cache::cached_image_path(&app.paths.cache_dir, &port.folder) else { return };
    if dest.exists() {
        return;
    }
    let cache_dir = app.paths.cache_dir.clone();
    let folder = port.folder.clone();
    let events = app.events.clone();
    std::thread::spawn(move || {
        crate::core::image_cache::cache_image(&url, &cache_dir, &folder);
        if crate::core::image_cache::cached_image_path(&cache_dir, &folder).map(|p| p.exists()).unwrap_or(false) {
            lock(&events).push(AppEvent::ImageCached { folder });
        }
    });
}

pub(crate) fn launch_executable(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port, exe: &Path) {
    if is_port_running(app, port.key()) {
        return;
    }
    if !exe.exists() {
        open_error_dialog(app, router, port.clone());
        return;
    }
    if app.stress_test {
        return;
    }
    if let Ok(child) = crate::core::launch::launch(exe) {
        app.install_runtime.running_processes.borrow_mut().insert(port.key().to_string(), child);
        app.install_runtime.launch_started_at.borrow_mut().insert(port.key().to_string(), Instant::now());
        if app.state.borrow().discord_rpc_enabled {
            let large_image = port.icon.clone().or_else(|| port.image.clone());
            let handle = crate::core::discord_presence::start(port.display_name().to_string(), port.folder.clone(), large_image);
            app.install_runtime.discord_presence.borrow_mut().insert(port.key().to_string(), handle);
        }
        repair_missing_cached_image(app, port);
        if app.window().get_big_mode() {
            app.window().window().set_minimized(true);
            app.install_runtime.minimized_for_game.set(true);
        }
    }
}

pub(crate) fn launch_flow(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    let Ok(game_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else {
        let window = app.window();
        let tr = window.global::<crate::Tr>();
        open_message_dialog(app, router, &tr.invoke_dialog_title_invalid_port(), &tr.invoke_message_invalid_folder_name());
        return;
    };
    if !game_dir.exists() {
        open_error_dialog(app, router, port.clone());
        return;
    }
    if let Some(favorite) = app.state.borrow().get(port.key()).and_then(|i| i.favorite_exe.clone()) {
        if let Ok(path) = crate::core::path_safety::safe_join(&game_dir, &favorite) {
            if path.exists() {
                launch_executable(app, router, port, &path);
                return;
            }
        }
    }
    match crate::core::executable_detect::resolve_executable(port.executable.as_ref(), &game_dir) {
        Ok(exe) => launch_executable(app, router, port, &exe),
        Err(ExecutableSelectionError::Ambiguous(_, candidates)) => {
            let labels: Vec<String> =
                candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()).collect();
            let port2 = port.clone();
            super::dialogs::open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_executable(), labels, move |app, router, idx| {
                if let Some(exe) = candidates.get(idx) {
                    launch_executable(app, router, &port2, exe);
                }
            });
        }
        Err(ExecutableSelectionError::Message(message)) => {
            open_message_dialog(app, router, &tr!(app).invoke_dialog_title_executable_not_found(), &message)
        }
    }
}

pub(crate) fn with_indexed_port(app: &Rc<AppState>, index: i32, action: impl FnOnce(Port)) {
    let port = app.windowed_nav.displayed_windowed.borrow().get(index as usize).cloned();
    if let Some(port) = port {
        action(port);
    }
}

pub(crate) fn install_row(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, index: i32) {
    if dialog_is_open(app) {
        return;
    }
    with_indexed_port(app, index, |port| start_install(app, router, port, None, None));
}

pub(crate) fn open_update_toggle_dialog_row(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, index: i32) {
    if dialog_is_open(app) {
        return;
    }
    with_indexed_port(app, index, |port| open_update_toggle_dialog(app, router, port));
}

pub(crate) fn activate_port(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    if dialog_is_open(app) || app.install_runtime.installing.borrow().contains(port.key()) {
        return;
    }
    if crate::core::installer::is_installed(port, &app.paths.library_dir) {
        launch_with_update_check(app, router, port);
    } else {
        start_install(app, router, port.clone(), None, None);
    }
}

pub(crate) fn launch_with_update_check(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    let (tag, installed_at) = {
        let state = app.state.borrow();
        let should_check = state.release_sync
            && matches!(port.source_type, SourceType::Github | SourceType::Gitlab)
            && state.get(port.key()).map(|i| i.update).unwrap_or(true);
        if !should_check {
            drop(state);
            launch_flow(app, router, port);
            return;
        }
        let info = state.get(port.key());
        let installed_at = info.map(|i| i.installed_at.clone()).unwrap_or_default();
        if !crate::core::state::is_stale_for_update_check(&installed_at) {
            drop(state);
            launch_flow(app, router, port);
            return;
        }
        (info.and_then(|i| i.installed_tag.clone()), installed_at)
    };

    if try_claim_op_slot(app, port).is_none() {
        return;
    }
    let (github_token, gitlab_token) = app.api_tokens();
    let port_owned = port.clone();
    let events = app.events.clone();
    std::thread::spawn(move || {
        let available = crate::core::jobs::run_update_check(&port_owned, tag.as_deref(), &installed_at, github_token.as_deref(), gitlab_token.as_deref())
            .unwrap_or(false);
        lock(&events).push(AppEvent::PlayUpdateChecked { port: Box::new(port_owned), available });
    });
}

pub(crate) fn activate_selection(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if let Some(port) = app.current_selected_port() {
        activate_port(app, router, &port);
    }
}

pub(crate) fn show_info_for_current_selection(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if let Some(port) = app.current_selected_port() {
        super::dialogs::open_info_dialog(app, router, &port);
    }
}

pub(crate) fn reveal_selected_folder(app: &Rc<AppState>) {
    let Some(port) = app.current_selected_port() else { return };
    let Ok(game_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else { return };
    open_path_if_exists(&game_dir);
}

pub(crate) fn delete_port(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    if dialog_is_open(app) || app.install_runtime.installing.borrow().contains(port.key()) || is_port_running(app, port.key()) || port.user_managed {
        return;
    }
    match crate::core::installer::uninstall_port(port, &app.paths.library_dir, &app.paths.saves_backup_dir) {
        Ok(()) => {
            app.state.borrow_mut().mark_removed(port.key());
            app.refresh_current_view();
            app.refresh_tray_recent_games();
        }
        Err(message) => open_message_dialog(app, router, &tr!(app).invoke_dialog_title_uninstall_error(), &message),
    }
}
