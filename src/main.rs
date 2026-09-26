#![windows_subsystem = "windows"]

mod app;
mod core;
mod ui;

use app::dialogs::{apply_theme, dialog_is_open, open_info_dialog, open_settings_dialog, open_uninstall_confirm_dialog, DialogSlot};
use app::events::{lock, poll_app_events, AppEvent};
use app::gamepad_target::AppGamepadTarget;
use app::install_launch::{activate_selection, install_row, launch_flow, open_port_folder, open_update_toggle_dialog_row, reveal_selected_folder, with_indexed_port};
use app::state::{AppPaths, AppState, DialogNav, GridNav, InstallRuntime, ThemeState, WindowGeometry, WindowedNav};
use app::sync::{launch_self_update, start_catalog_sync, start_self_update_check, start_themes_sync};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ui::chrome;
use ui::font_sizing::{apply_mode_geometry, compute_mode_geometry};
use ui::gamepad_router::GamepadRouter;

slint::include_modules!();

const DISCORD_URL: &str = "https://discord.com/invite/5GYmst9twA";
const DUPLICATE_CLICK_MS: u128 = 50;

fn base_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("."))
}

fn repeat(interval: Duration, callback: impl FnMut() + 'static) -> slint::Timer {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, interval, callback);
    timer
}

fn main() {
    chrome::enable_dark_context_menus();

    let events: Arc<Mutex<Vec<AppEvent>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let events = events.clone();
        if !chrome::claim_single_instance(move || lock(&events).push(AppEvent::BringToForeground)) {
            return;
        }
    }

    chrome::register_desktop_entry();

    let bdir = base_dir();
    let ports_json_path = bdir.join("ports.json");
    let catalog = if ports_json_path.exists() {
        match core::config::load_config(&ports_json_path) {
            Ok(ports) => ports,
            Err(e) => {
                chrome::show_startup_error(&format!("Couldn't load ports.json: {e}"));
                return;
            }
        }
    } else {
        Vec::new()
    };
    let catalog = core::config::merge_local_catalog(catalog, core::config::load_local_config(&bdir.join("ports.local.json")));

    let state_path = bdir.join("state.json");
    let is_first_run = !state_path.exists();
    let state = RefCell::new(core::state::StateManager::load(&state_path));
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

    let themes_path = bdir.join("themes.json");
    let mut theme_cfg = ui::theme::ThemeConfig::default();
    ui::theme::load(&themes_path, &mut theme_cfg, &state.borrow().active_theme);

    let window = match AppWindow::new() {
        Ok(w) => w,
        Err(e) => {
            chrome::show_startup_error(&format!("Failed to create the window: {e}"));
            return;
        }
    };
    let _ = slint::set_xdg_app_id("ports_launcher");

    let language = state.borrow().language.clone();
    if !language.is_empty() {
        let _ = slint::select_bundled_translation(&language);
    }
    if is_first_run {
        let placeholder = window.global::<Tr>().invoke_placeholder_default_search().to_string();
        state.borrow_mut().set_placeholder_text(placeholder);
    }

    let (border_width, window_width_fraction, big_mode, show_clock) = {
        let s = state.borrow();
        (s.border_width, s.window_width_fraction, s.fullscreen, s.show_clock)
    };
    let font_family = state.borrow().font_family.clone().unwrap_or_else(chrome::default_font_family);
    apply_theme(&window, &theme_cfg, border_width);
    window.set_placeholder_text(state.borrow().placeholder_text.clone().into());
    window.set_show_clock(show_clock);
    window.set_font_family(font_family.clone().into());
    window.set_big_mode(big_mode);
    window.set_self_update_available(state.borrow().launcher_update_available);

    let _clock_timer = show_clock.then(|| {
        window.set_clock_text(core::clock::format_now().into());
        let weak = window.as_weak();
        repeat(Duration::from_secs(1), move || {
            if let Some(w) = weak.upgrade() {
                w.set_clock_text(core::clock::format_now().into());
            }
        })
    });

    let area = chrome::work_area_under_cursor();
    let scale = chrome::scale_factor_under_cursor();
    window.global::<Theme>().set_scale_factor(scale);
    let grid_columns = core::grid::compute_grid_columns(area.2 as f32 / scale);
    window.set_card_width(core::grid::CARD_WIDTH);
    window.set_card_height(core::grid::CARD_HEIGHT);
    window.set_card_spacing(core::grid::CARD_SPACING);
    window.set_grid_columns(grid_columns as i32);
    let normal_mode = compute_mode_geometry(&font_family, area, scale, false, window_width_fraction, border_width);
    let fullscreen_mode = compute_mode_geometry(&font_family, area, scale, true, window_width_fraction, border_width);
    apply_mode_geometry(&window, if big_mode { &fullscreen_mode } else { &normal_mode });
    chrome::set_fullscreen(window.window(), big_mode);

    window.show().expect("failed to show the window");
    {
        let weak = window.as_weak();
        let _ = window.window().set_rendering_notifier(move |rendering_state, _| {
            if !matches!(rendering_state, slint::RenderingState::RenderingSetup) {
                return;
            }
            let Some(window) = weak.upgrade() else { return };
            if let Some(native) = chrome::native_window(window.window()) {
                chrome::apply_window_icon(native);
                chrome::force_normal_window_visibility(native);
                chrome::force_foreground_window(native);
            }
        });
    }

    let app = Rc::new(AppState {
        window: window.as_weak(),
        tray: RefCell::new(None),
        state,
        catalog: RefCell::new(catalog),
        paths: AppPaths { library_dir, cache_dir, config_dir: bdir.clone(), saves_backup_dir: bdir.join("Saves Backup"), themes_path },
        theme: ThemeState { theme_config: RefCell::new(theme_cfg), font_family },
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
            pending_image_fetches: RefCell::new(HashSet::new()),
        },
        windowed_nav: WindowedNav { displayed_windowed: RefCell::new(Vec::new()), windowed_selected: Cell::new(0), search_query: RefCell::new(String::new()) },
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
        cheats: Default::default(),
        events,
    });
    let router = Rc::new(RefCell::new(GamepadRouter::new()));

    app.rebuild_windowed("");
    if big_mode {
        app.enter_fullscreen();
    }

    wire_main_window(&window, &app, &router);

    let _event_timer = {
        let (app, router) = (app.clone(), router.clone());
        repeat(Duration::from_millis(100), move || poll_app_events(&app, &router))
    };

    start_self_update_check(&app);
    start_catalog_sync(&app);
    start_themes_sync(&app);

    let tray = AppTray::new().expect("failed to create the tray icon");
    wire_tray(&tray, &app, &router);

    let gamepad_available = router.borrow().is_available();
    let _gamepad_timer = gamepad_available.then(|| {
        router.borrow_mut().push_target(Rc::new(AppGamepadTarget { app: app.clone(), router: router.clone() }));
        let router = router.clone();
        repeat(Duration::from_millis(ui::gamepad_router::POLL_INTERVAL_MS), move || {
            let result = router.borrow_mut().poll();
            if let Some(result) = result.filter(|_| chrome::foreground_window_belongs_to_us()) {
                ui::gamepad_router::dispatch(result);
            }
        })
    });

    slint::run_event_loop().expect("event loop failed");
    let _ = window.hide();
}

fn wire_main_window(window: &AppWindow, app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    macro_rules! with {
        ($($name:ident),* => $body:expr) => {{
            $(let $name = $name.clone();)*
            $body
        }};
    }

    window.on_search_changed(with!(app => move |query| {
        let query = if app::cheats::search_changed(&app, &query) { Default::default() } else { query };
        *app.windowed_nav.search_query.borrow_mut() = query.to_string();
        app.refresh_current_view();
    }));
    window.on_fullscreen_toggle_requested(with!(app => move || app.toggle_fullscreen()));
    window.on_minimize_requested(with!(app => move || app.window().window().set_minimized(true)));
    window.on_settings_requested(with!(app, router => move || open_settings_dialog(&app, &router)));
    window.on_window_size_requested(with!(app => move |percent| app.set_window_size_percent(percent)));
    window.on_border_adjust_requested(with!(app => move |delta| app.adjust_border(delta)));
    window.on_move_requested(with!(app => move |dx, dy| app.move_selection(dx, dy)));
    window.on_activate_requested(with!(app, router => move || activate_selection(&app, &router)));
    window.on_reveal_folder_requested(with!(app => move || reveal_selected_folder(&app)));
    window.on_discord_requested(|| core::launch::open_url(DISCORD_URL));
    window.on_key_intercepted(with!(app => move |text, modified, repeat| {
        app::cheats::intercept(&app, (!repeat).then(|| app::cheats::keyboard_input(&text, modified)))
    }));
    window.on_github_requested(with!(app, router => move || {
        if app.window().get_self_update_available() {
            launch_self_update(&app, &router);
        } else {
            core::launch::open_url(core::version::PROJECT_URL);
        }
    }));
    window.on_close_requested(with!(app => move || {
        let _ = app.window().hide();
    }));

    window.on_card_activated(with!(app, router => move |row, col| {
        let cell = (row as usize, col as usize);
        app.select_grid_cell(cell);
        let now = Instant::now();
        if let Some((last_cell, last_time)) = app.grid_nav.last_card_click.get() {
            let elapsed_ms = now.duration_since(last_time).as_millis();
            if elapsed_ms < DUPLICATE_CLICK_MS {
                return;
            }
            if last_cell == cell && elapsed_ms <= app.grid_nav.double_click_ms as u128 {
                app.grid_nav.last_card_click.set(None);
                activate_selection(&app, &router);
                return;
            }
        }
        app.grid_nav.last_card_click.set(Some((cell, now)));
    }));
    window.on_card_hovered(with!(app => move |row, col| app.select_grid_cell((row as usize, col as usize))));
    window.on_card_unhovered(with!(app => move || {
        let previous = app.visible_grid_selection();
        app.grid_nav.grid_mouse_active.set(false);
        app.refresh_grid_selection(previous);
    }));

    let select_row = with!(app => move |index: i32| {
        app.windowed_nav.windowed_selected.set(index as usize);
        app.push_selected_index();
    });
    window.on_list_row_hovered(select_row.clone());
    window.on_list_row_activated(select_row);
    window.on_list_row_play_requested(with!(app, router => move |index| {
        if !dialog_is_open(&app) {
            with_indexed_port(&app, index, |port| launch_flow(&app, &router, &port));
        }
    }));
    window.on_list_row_install_requested(with!(app, router => move |index| install_row(&app, &router, index)));
    window.on_list_row_update_requested(with!(app, router => move |index| open_update_toggle_dialog_row(&app, &router, index)));
    window.on_list_row_uninstall_requested(with!(app, router => move |index| {
        if !dialog_is_open(&app) {
            with_indexed_port(&app, index, |port| open_uninstall_confirm_dialog(&app, &router, port));
        }
    }));
    window.on_list_row_open_folder_requested(with!(app => move |index| with_indexed_port(&app, index, |port| open_port_folder(&app, &port))));
    window.on_list_row_info_requested(with!(app, router => move |index| {
        if !dialog_is_open(&app) {
            with_indexed_port(&app, index, |port| open_info_dialog(&app, &router, &port));
        }
    }));

    #[cfg(target_os = "linux")]
    type DragStart = Option<(slint::PhysicalPosition, (i32, i32))>;
    #[cfg(target_os = "linux")]
    let drag_start: Rc<Cell<DragStart>> = Rc::new(Cell::new(None));
    #[cfg(target_os = "linux")]
    window.on_window_drag_moved(with!(app, drag_start => move || {
        let Some((origin, (press_x, press_y))) = drag_start.get() else { return };
        let Some((cur_x, cur_y)) = chrome::cursor_position() else { return };
        let position = slint::PhysicalPosition { x: origin.x + cur_x - press_x, y: origin.y + cur_y - press_y };
        app.window().window().set_position(slint::WindowPosition::Physical(position));
    }));
    window.on_window_drag_requested(with!(app => move || {
        let window = app.window();
        #[cfg(target_os = "linux")]
        drag_start.set(chrome::cursor_position().map(|cursor| (window.window().position(), cursor)));
        if chrome::begin_window_drag(window.window()) {
            let weak = app.window.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(window) = weak.upgrade() {
                    window.window().dispatch_event(slint::platform::WindowEvent::PointerReleased {
                        position: slint::LogicalPosition::default(),
                        button: slint::platform::PointerEventButton::Left,
                    });
                }
            });
        }
    }));
}

fn wire_tray(tray: &AppTray, app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    *app.tray.borrow_mut() = Some(tray.as_weak());
    if let Some((rgba, width, height)) = chrome::extract_app_icon_rgba() {
        tray.set_tray_icon(slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(&rgba, width, height)));
    }
    app.refresh_tray_recent_games();
    {
        let (app, router) = (app.clone(), router.clone());
        tray.on_game_activated(move |key| {
            if let Some(port) = app.find_port(&key) {
                launch_flow(&app, &router, &port);
            }
        });
    }
    {
        let (app, router) = (app.clone(), router.clone());
        tray.on_settings_requested(move || open_settings_dialog(&app, &router));
    }
    {
        let app = app.clone();
        tray.on_restore_requested(move || lock(&app.events).push(AppEvent::BringToForeground));
    }
    tray.on_quit_requested(|| {
        let _ = slint::quit_event_loop();
    });
    let _ = tray.show();
}
