
use super::dialogs::{apply_theme, close_current_dialog, open_message_dialog, open_picker_dialog, tr, update_progress_dialog, DialogSlot};
use super::install_launch::{launch_flow, start_install};
use super::playtime::{any_process_running, checkpoint_playtime, refresh_live_playtime_display};
use super::state::AppState;
use crate::core::models::Port;
use crate::ui::chrome;
use crate::ui::gamepad_router::GamepadRouter;
use crate::Tr;
use serde_json::Value;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) enum AppEvent {
    InstallProgress { message: String },
    InstallDone { key: String, tag: Option<String>, pin_version: bool },
    InstallAssetAmbiguous { key: String, assets: Vec<Value>, release_override: Option<Value> },
    InstallError { key: String, message: String },
    ExtraInstallDone { key: String, result: Result<(), String> },
    PlayUpdateChecked { port: Box<Port>, available: bool },
    SelfUpdateAvailable,
    VersionsFetched { key: String, releases: Vec<Value> },
    VersionsFetchError { key: String, message: String },
    ImageCached { folder: String },
    BringToForeground,
    RemoteCatalogFetched(Vec<Port>),
    PortsCheckDone { etag: String },
    RemoteThemesFetched,
    ThemesCheckDone { etag: String },
    SaveBackupProgress { name: String },
    SaveBackupDone { copied: usize, skipped: usize, failed: usize },
}

fn json_field_labels(items: &[Value], field: &str) -> Vec<String> {
    items.iter().map(|v| v.get(field).and_then(Value::as_str).unwrap_or("?").to_string()).collect()
}

fn finish_operation(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, key: &str) {
    app.install_runtime.installing.borrow_mut().remove(key);
    close_current_dialog(app, router);
}

fn show_progress(app: &AppState, message: impl FnOnce(&Tr<'_>) -> slint::SharedString) {
    if let DialogSlot::Progress(d) = &*app.dialog_nav.dialogs.borrow() {
        let text = message(&d.global::<Tr>());
        update_progress_dialog(app, d, &text);
    }
}

pub(crate) fn poll_app_events(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    app.refresh_geometry_if_scale_changed();
    checkpoint_playtime(app);
    refresh_live_playtime_display(app);
    let drained: Vec<AppEvent> = std::mem::take(&mut *lock(&app.events));
    let last_install_progress = drained.iter().rposition(|e| matches!(e, AppEvent::InstallProgress { .. }));
    let last_backup_progress = drained.iter().rposition(|e| matches!(e, AppEvent::SaveBackupProgress { .. }));
    for (i, event) in drained.into_iter().enumerate() {
        match event {
            AppEvent::InstallProgress { .. } if Some(i) != last_install_progress => {}
            AppEvent::SaveBackupProgress { .. } if Some(i) != last_backup_progress => {}
            AppEvent::InstallProgress { message } => show_progress(app, |_| message.into()),
            AppEvent::SaveBackupProgress { name } => show_progress(app, |tr| tr.invoke_progress_backing_up_named(name.into())),
            AppEvent::InstallDone { key, tag, pin_version } => {
                finish_operation(app, router, &key);
                let port = app.find_port(&key);
                if let Some(port) = &port {
                    app.grid_nav.card_image_cache.borrow_mut().remove(&port.folder);
                }
                {
                    let mut state = app.state.borrow_mut();
                    state.mark_installed(&key, tag);
                    if pin_version {
                        state.set_port_update(&key, false);
                    }
                }
                app.refresh_current_view();
                if app.install_runtime.pending_launch_after_install.borrow_mut().remove(&key) {
                    if let Some(port) = port {
                        launch_flow(app, router, &port);
                    }
                }
            }
            AppEvent::InstallAssetAmbiguous { key, assets, release_override } => {
                finish_operation(app, router, &key);
                if let Some(port) = app.find_port(&key) {
                    let labels = json_field_labels(&assets, "name");
                    open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_file(), labels, move |app, router, idx| {
                        if let Some(chosen) = assets.get(idx) {
                            start_install(app, router, port.clone(), Some(chosen.clone()), release_override.clone());
                        }
                    });
                }
            }
            AppEvent::VersionsFetched { key, releases } => {
                finish_operation(app, router, &key);
                if let Some(port) = app.find_port(&key) {
                    let labels = json_field_labels(&releases, "tag_name");
                    open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_version(), labels, move |app, router, idx| {
                        if let Some(release) = releases.get(idx) {
                            start_install(app, router, port.clone(), None, Some(release.clone()));
                        }
                    });
                }
            }
            AppEvent::VersionsFetchError { key, message } => {
                finish_operation(app, router, &key);
                open_message_dialog(app, router, &tr!(app).invoke_dialog_title_error(), &message);
            }
            AppEvent::InstallError { key, message } => {
                finish_operation(app, router, &key);
                open_message_dialog(app, router, &tr!(app).invoke_dialog_title_installation_error(), &message);
            }
            AppEvent::ExtraInstallDone { key, result } => {
                finish_operation(app, router, &key);
                let window = app.window();
                let tr = window.global::<Tr>();
                let message = match result {
                    Ok(()) => tr.invoke_message_extras_installed(),
                    Err(e) => tr.invoke_message_extras_failed(e.into()),
                };
                open_message_dialog(app, router, &tr.invoke_dialog_title_extras(), &message);
            }
            AppEvent::PlayUpdateChecked { port, available } => {
                app.install_runtime.installing.borrow_mut().remove(port.key());
                if available {
                    app.install_runtime.pending_launch_after_install.borrow_mut().insert(port.key().to_string());
                    start_install(app, router, *port, None, None);
                } else {
                    launch_flow(app, router, &port);
                }
            }
            AppEvent::SelfUpdateAvailable => {
                app.window().set_self_update_available(true);
                app.state.borrow_mut().set_launcher_update_available(true);
            }
            AppEvent::ImageCached { folder } => {
                app.grid_nav.pending_image_fetches.borrow_mut().remove(&folder);
                let cached = crate::core::image_cache::cached_image_path(&app.paths.cache_dir, &folder).is_ok_and(|p| p.exists());
                if cached {
                    app.grid_nav.card_image_cache.borrow_mut().remove(&folder);
                    app.refresh_current_view();
                }
            }
            AppEvent::RemoteCatalogFetched(ports) => {
                *app.catalog.borrow_mut() = ports;
                app.refresh_current_view();
            }
            AppEvent::PortsCheckDone { etag } => app.state.borrow_mut().mark_catalog_check(etag),
            AppEvent::ThemesCheckDone { etag } => app.state.borrow_mut().mark_themes_check(etag),
            AppEvent::RemoteThemesFetched => {
                let active = app.state.borrow().active_theme.clone();
                crate::ui::theme::load(&app.paths.themes_path, &mut app.theme.theme_config.borrow_mut(), &active);
                apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
            }
            AppEvent::SaveBackupDone { copied, skipped, failed } => {
                close_current_dialog(app, router);
                let window = app.window();
                let tr = window.global::<Tr>();
                let summary = tr.invoke_message_backup_summary(copied as i32, skipped as i32, failed as i32);
                open_message_dialog(app, router, &tr.invoke_dialog_title_saves_backup(), &summary);
            }
            AppEvent::BringToForeground => chrome::restore_window(app.window().window()),
        }
    }

    let still_running = any_process_running(app);
    let runtime = &app.install_runtime;
    if runtime.minimized_for_game.get() && !still_running {
        runtime.minimized_for_game.set(false);
        let window = app.window();
        if window.get_big_mode() {
            chrome::restore_window(window.window());
        }
    }
}
