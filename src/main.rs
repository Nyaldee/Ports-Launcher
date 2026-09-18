#![windows_subsystem = "windows"]

mod app;
mod core;
#[cfg(debug_assertions)]
mod stress_test;
mod ui;

use app::dialogs::{apply_theme, dialog_is_open, open_info_dialog, open_settings_dialog, open_uninstall_confirm_dialog, DialogSlot};
use app::events::{lock, poll_app_events, AppEvent};
use app::gamepad_target::AppGamepadTarget;
use app::install_launch::{
    activate_selection, install_row, launch_flow, open_update_toggle_dialog_row, reveal_selected_folder, with_indexed_port,
};
use app::state::{AppPaths, AppState, DialogNav, GridNav, InstallRuntime, ThemeState, WindowGeometry, WindowedNav};
use app::sync::{launch_self_update, start_catalog_sync, start_self_update_check, start_themes_sync};
use core::models::Port;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use ui::chrome;
use ui::font_sizing::{apply_mode_geometry, compute_mode_geometry};
use ui::gamepad_router::GamepadRouter;

slint::include_modules!();

const SINGLE_INSTANCE_PORT: u16 = 57391;

const PAGE_ROWS: i32 = 10;

const GITHUB_URL: &str = "https://github.com/Nyaldee/Ports-Launcher";
const SELF_REPO: &str = "Nyaldee/Ports-Launcher";
const DISCORD_URL: &str = "https://discord.com/invite/5GYmst9twA";

fn acquire_single_instance_lock() -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)).ok()
}

fn base_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

fn to_port_items(ports: &[&Port], library_dir: &Path, state: &core::state::StateManager) -> Vec<PortItem> {
    ports
        .iter()
        .map(|p| PortItem {
            name: p.name.clone().into(),
            auto_update_off: state.get(p.key()).map(|i| !i.update).unwrap_or(false),
            installed: core::installer::is_installed(p, library_dir),
            is_local: p.user_managed,
        })
        .collect()
}
fn main() {
    chrome::enable_dark_context_menus();

    let events: Arc<Mutex<Vec<AppEvent>>> = Arc::new(Mutex::new(Vec::new()));

    let Some(single_instance_listener) = acquire_single_instance_lock() else {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
            let _ = stream.write_all(&[0u8]);
        }
        return;
    };
    {
        let events = events.clone();
        std::thread::spawn(move || {
            for stream in single_instance_listener.incoming().flatten() {
                drop(stream);
                lock(&events).push(AppEvent::BringToForeground);
            }
        });
    }

    chrome::enable_dpi_awareness();

    chrome::register_desktop_entry();

    #[cfg(debug_assertions)]
    let stress_test_iterations = stress_test::parse_stress_test_iterations();
    #[cfg(not(debug_assertions))]
    let stress_test_iterations: Option<u32> = None;

    #[cfg(debug_assertions)]
    let bdir = if stress_test_iterations.is_some() { stress_test::build_stress_sandbox() } else { base_dir() };
    #[cfg(not(debug_assertions))]
    let bdir = base_dir();

    #[cfg(debug_assertions)]
    let _sandbox_cleanup = stress_test_iterations.is_some().then(|| stress_test::CleanupSandboxOnDrop(bdir.clone()));

    let ports_json_path = bdir.join("ports.json");
    let catalog = if ports_json_path.exists() {
        match core::config::load_config(&ports_json_path) {
            Ok(v) => v,
            Err(e) => {
                chrome::show_startup_error(&format!("Couldn't load ports.json: {e}"));
                return;
            }
        }
    } else {
        Vec::new()
    };
    let catalog = core::config::merge_local_catalog(catalog, core::config::load_local_config(&bdir.join("ports.local.json")));

    let is_first_run = !bdir.join("state.json").exists();
    let state = RefCell::new(core::state::StateManager::load(&bdir.join("state.json"), &bdir.join("themes.json")));

    if state.borrow().last_launcher_update_check.is_empty() {
        state.borrow_mut().mark_launcher_update_check();
    }

    let library_dir = bdir.join("Library");
    let cache_dir = bdir.join("cache");
    let _ = std::fs::create_dir_all(&library_dir);
    let _ = std::fs::create_dir_all(&cache_dir);

    {
        let mut s = state.borrow_mut();
        for port in &catalog {
            if s.get(port.key()).is_none() && core::installer::is_installed(port, &library_dir) {
                s.mark_installed(port.key(), None);
            }
        }
    }

    let mut theme_cfg = ui::theme::ThemeConfig::default();
    ui::theme::load(&bdir.join("themes.json"), &mut theme_cfg, &state.borrow().active_theme);

    let window = match AppWindow::new() {
        Ok(w) => w,
        Err(e) => {
            chrome::show_startup_error(&format!("Failed to create the window: {e}"));
            return;
        }
    };

    let _ = slint::set_xdg_app_id("ports_launcher");

    if is_first_run {
        let seeded = window.global::<Tr>().invoke_placeholder_default_search().to_string();
        state.borrow_mut().set_placeholder_text(seeded);
    }

    apply_theme(&window, &theme_cfg, state.borrow().border_width);
    window.set_placeholder_text(state.borrow().placeholder_text.clone().into());
    window.set_show_clock(state.borrow().show_clock);

    let _clock_timer = if state.borrow().show_clock {
        window.set_clock_text(core::clock::format_now().into());
        let timer = slint::Timer::default();
        let weak = window.as_weak();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(1000), move || {
            if let Some(w) = weak.upgrade() {
                w.set_clock_text(core::clock::format_now().into());
            }
        });
        Some(timer)
    } else {
        None
    };

    let (area_x, area_y, screen_w, screen_h) = chrome::work_area_under_cursor();

    let pre_show_scale = chrome::scale_factor_under_cursor();

    let grid_available_width = screen_w as f32 / pre_show_scale;
    let grid_columns = core::grid::compute_grid_columns(grid_available_width);

    window.set_card_width(core::grid::CARD_WIDTH);
    window.set_card_height(core::grid::CARD_HEIGHT);
    window.set_card_spacing(core::grid::CARD_SPACING);
    window.set_grid_columns(grid_columns as i32);

    let font_family = state.borrow().font_family.clone().unwrap_or_else(|| "Segoe UI".to_string());
    window.set_font_family(font_family.clone().into());

    let big_mode = state.borrow().fullscreen;
    window.set_big_mode(big_mode);

    let area = (area_x, area_y, screen_w, screen_h);
    let (window_width_fraction, border_width) = { let s = state.borrow(); (s.window_width_fraction, s.border_width) };
    let normal_mode = compute_mode_geometry(&font_family, area, pre_show_scale, false, window_width_fraction, border_width);
    let fullscreen_mode = compute_mode_geometry(&font_family, area, pre_show_scale, true, window_width_fraction, border_width);
    apply_mode_geometry(&window, if big_mode { &fullscreen_mode } else { &normal_mode });

    let saved_language = state.borrow().language.clone();
    if !saved_language.is_empty() {
        let _ = slint::select_bundled_translation(&saved_language);
    }

    if state.borrow().launcher_update_available {
        window.set_self_update_available(true);
    }

    window.show().expect("échec de l'affichage de la fenêtre");
    {
        let weak = window.as_weak();
        let applied = std::cell::Cell::new(false);
        let _ = window.window().set_rendering_notifier(move |state, _| {
            if applied.get() || !matches!(state, slint::RenderingState::RenderingSetup) {
                return;
            }
            applied.set(true);
            let Some(window) = weak.upgrade() else { return };
            if let Some(native) = chrome::native_window(window.window()) {
                chrome::apply_window_icon(native);
                chrome::force_normal_window_visibility(native);
            }
        });
    }

    let scale = chrome::scale_factor_under_cursor();
    window.global::<Theme>().set_scale_factor(scale);

    let app = Rc::new(AppState {
        window: window.as_weak(),
        tray: RefCell::new(None),
        state,
        catalog: RefCell::new(catalog),
        paths: AppPaths {
            library_dir: library_dir.clone(),
            cache_dir: cache_dir.clone(),
            config_dir: bdir.clone(),
            saves_backup_dir: bdir.join("Saves Backup"),
            themes_path: bdir.join("themes.json"),
        },
        theme: ThemeState {
            semantic: theme_cfg.semantic,
            font_family: font_family.clone(),
            theme_config: RefCell::new(theme_cfg),
        },
        window_geometry: WindowGeometry {
            border_width: Cell::new(border_width),
            window_width_fraction: Cell::new(window_width_fraction),
            scale: Cell::new(scale),
            normal_mode: RefCell::new(normal_mode),
            fullscreen_mode: RefCell::new(fullscreen_mode),
        },
        grid_nav: GridNav {
            grid_columns: Cell::new(grid_columns),
            displayed_installed: RefCell::new(Vec::new()),
            grid_selected: Cell::new((0, 0)),
            grid_mouse_active: Cell::new(true),
            last_card_click: Cell::new(None),
            double_click_ms: chrome::double_click_time_ms(),
            card_image_cache: RefCell::new(HashMap::new()),
        },
        windowed_nav: WindowedNav {
            displayed_windowed: RefCell::new(Vec::new()),
            windowed_selected: Cell::new(0),
            search_query: RefCell::new(String::new()),
        },
        install_runtime: InstallRuntime {
            installing: RefCell::new(HashSet::new()),
            running_processes: RefCell::new(HashMap::new()),
            launch_started_at: RefCell::new(HashMap::new()),
            discord_presence: RefCell::new(HashMap::new()),
            pending_launch_after_install: RefCell::new(HashSet::new()),
            minimized_for_game: Cell::new(false),
        },
        dialog_nav: DialogNav {
            dialogs: RefCell::new(DialogSlot::None),
            picker_index: Cell::new(0),
            info_nav_index: Cell::new(0),
            confirm_nav_index: Cell::new(0),
            error_nav_index: Cell::new(0),
            info_dialog_port_key: RefCell::new(None),
        },
        events: events.clone(),
        stress_test: stress_test_iterations.is_some(),
    });

    let router = Rc::new(RefCell::new(GamepadRouter::new()));

    app.rebuild_windowed("");
    if big_mode {
        app.enter_fullscreen();
    }

    {
        let app = app.clone();
        window.on_search_changed(move |query| {
            *app.windowed_nav.search_query.borrow_mut() = query.to_string();
            if app.window().get_big_mode() {
                app.rebuild_grid(true);
            } else {
                app.rebuild_windowed(&query);
            }
        });
    }

    {
        let app = app.clone();
        window.on_fullscreen_toggle_requested(move || app.toggle_fullscreen());
    }

    {
        let weak = window.as_weak();
        window.on_minimize_requested(move || {
            if let Some(w) = weak.upgrade() {
                w.window().set_minimized(true);
            }
        });
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_settings_requested(move || open_settings_dialog(&app, &router));
    }

    {
        let app = app.clone();
        window.on_window_size_requested(move |percent| app.set_window_size_percent(percent));
    }
    {
        let app = app.clone();
        window.on_border_adjust_requested(move |delta| app.adjust_border(delta));
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_card_activated(move |row, col| {
            let previous = app.grid_nav.grid_mouse_active.get().then(|| app.grid_nav.grid_selected.get());
            app.grid_nav.grid_selected.set((row as usize, col as usize));
            app.refresh_grid_selection(previous);
            let key = (row as usize, col as usize);
            let now = Instant::now();
            if let Some((last_key, last_time)) = app.grid_nav.last_card_click.get() {
                let elapsed_ms = now.duration_since(last_time).as_millis();
                if elapsed_ms < 50 {
                    return;
                }
                if last_key == key && elapsed_ms <= app.grid_nav.double_click_ms as u128 {
                    app.grid_nav.last_card_click.set(None);
                    activate_selection(&app, &router);
                    return;
                }
            }
            app.grid_nav.last_card_click.set(Some((key, now)));
        });
    }

    {
        let app = app.clone();
        window.on_list_row_hovered(move |index| {
            app.windowed_nav.windowed_selected.set(index as usize);
            app.push_selected_index();
        });
    }
    {
        let app = app.clone();
        window.on_card_hovered(move |row, col| {
            let previous = app.grid_nav.grid_mouse_active.get().then(|| app.grid_nav.grid_selected.get());
            app.grid_nav.grid_selected.set((row as usize, col as usize));
            app.grid_nav.grid_mouse_active.set(true);
            app.refresh_grid_selection(previous);
        });
    }
    {
        let app = app.clone();
        window.on_card_unhovered(move || {
            let previous = app.grid_nav.grid_mouse_active.get().then(|| app.grid_nav.grid_selected.get());
            app.grid_nav.grid_mouse_active.set(false);
            app.refresh_grid_selection(previous);
        });
    }

    {
        let app = app.clone();
        window.on_move_requested(move |dx, dy| {
            if app.window().get_big_mode() {
                app.move_grid_selection(dx, dy);
            } else if dx != 0 {
                app.move_windowed_selection(dx * PAGE_ROWS);
            } else {
                app.move_windowed_selection(dy);
            }
        });
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_activate_requested(move || activate_selection(&app, &router));
    }

    {
        let app = app.clone();
        window.on_reveal_folder_requested(move || reveal_selected_folder(&app));
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_github_requested(move || {
            if app.window().get_self_update_available() {
                launch_self_update(&app, &router);
            } else {
                core::launch::open_url(GITHUB_URL);
            }
        });
    }
    window.on_discord_requested(|| core::launch::open_url(DISCORD_URL));

    {
        let app = app.clone();
        window.on_list_row_activated(move |index| {
            app.windowed_nav.windowed_selected.set(index as usize);
            app.push_selected_index();
        });
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_play_requested(move |index| {
            if dialog_is_open(&app) {
                return;
            }
            with_indexed_port(&app, index, |port| launch_flow(&app, &router, &port));
        });
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_install_requested(move |index| install_row(&app, &router, index));
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_update_requested(move |index| open_update_toggle_dialog_row(&app, &router, index));
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_uninstall_requested(move |index| {
            if dialog_is_open(&app) {
                return;
            }
            with_indexed_port(&app, index, |port| open_uninstall_confirm_dialog(&app, &router, port));
        });
    }
    {
        let app = app.clone();
        window.on_list_row_open_folder_requested(move |index| {
            with_indexed_port(&app, index, |port| {
                if let Ok(dir) = core::path_safety::safe_join(&app.paths.library_dir, &port.folder) {
                    core::launch::open_path(&dir);
                }
            });
        });
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_info_requested(move |index| {
            if dialog_is_open(&app) {
                return;
            }
            with_indexed_port(&app, index, |port| open_info_dialog(&app, &router, &port));
        });
    }

    {
        let weak = window.as_weak();
        window.on_close_requested(move || {
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
        });
    }

    let drag_origin: Rc<Cell<Option<slint::PhysicalPosition>>> = Rc::new(Cell::new(None));
    #[cfg(target_os = "linux")]
    let drag_press_cursor: Rc<Cell<Option<(i32, i32)>>> = Rc::new(Cell::new(None));
    {
        let app = app.clone();
        let drag_origin = drag_origin.clone();
        #[cfg(target_os = "linux")]
        let drag_press_cursor = drag_press_cursor.clone();
        window.on_window_drag_requested(move || {
            drag_origin.set(Some(app.window().window().position()));
            #[cfg(target_os = "linux")]
            drag_press_cursor.set(chrome::cursor_position());
            let Some(native) = chrome::native_window(app.window().window()) else { return };
            chrome::begin_window_drag(native);
        });
    }
    {
        #[cfg(target_os = "linux")]
        let app = app.clone();
        #[cfg(target_os = "linux")]
        let drag_origin = drag_origin.clone();
        #[cfg(target_os = "linux")]
        let drag_press_cursor = drag_press_cursor.clone();
        window.on_window_drag_moved(move || {
            #[cfg(target_os = "linux")]
            {
                let Some(origin) = drag_origin.get() else { return };
                let Some((press_x, press_y)) = drag_press_cursor.get() else { return };
                let Some((cur_x, cur_y)) = chrome::cursor_position() else { return };
                let position = slint::PhysicalPosition { x: origin.x + (cur_x - press_x), y: origin.y + (cur_y - press_y) };
                app.window().window().set_position(slint::WindowPosition::Physical(position));
            }
        });
    }

    let _event_timer = {
        let app = app.clone();
        let router = router.clone();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
            poll_app_events(&app, &router);
        });
        timer
    };

    start_self_update_check(&app);
    start_catalog_sync(&app);
    start_themes_sync(&app);

    let tray = AppTray::new().expect("échec de la création de l'icône du tray");
    *app.tray.borrow_mut() = Some(tray.as_weak());
    if let Some((rgba, width, height)) = chrome::extract_app_icon_rgba() {
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, width, height);
        tray.set_tray_icon(slint::Image::from_rgba8(buffer));
    }
    app.refresh_tray_recent_games();
    {
        let app = app.clone();
        let router = router.clone();
        tray.on_game_activated(move |key| {
            let port = app.catalog.borrow().iter().find(|p| p.key() == key.as_str()).cloned();
            if let Some(port) = port {
                launch_flow(&app, &router, &port);
            }
        });
    }
    {
        let app = app.clone();
        let router = router.clone();
        tray.on_settings_requested(move || open_settings_dialog(&app, &router));
    }
    tray.on_quit_requested(|| {
        let _ = slint::quit_event_loop();
    });
    {
        let app = app.clone();
        tray.on_restore_requested(move || lock(&app.events).push(AppEvent::BringToForeground));
    }
    let _ = tray.show();

    let _gamepad_timer = if router.borrow().is_available() {
        router.borrow_mut().push_target(Rc::new(AppGamepadTarget { app: app.clone(), router: router.clone() }));
        let timer = slint::Timer::default();
        let router = router.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(ui::gamepad_router::POLL_INTERVAL_MS), move || {
            let result = router.borrow_mut().poll();
            if let Some(result) = result {
                if chrome::foreground_window_belongs_to_us() {
                    ui::gamepad_router::dispatch(result);
                }
            }
        });
        Some(timer)
    } else {
        None
    };

    #[cfg(debug_assertions)]
    let _stress_driver =
        stress_test_iterations.map(|iterations| stress_test::start_visual_stress_driver(app.clone(), router.clone(), iterations));

    slint::run_event_loop().expect("échec de la boucle d'évènements");
    let _ = window.hide();
}
