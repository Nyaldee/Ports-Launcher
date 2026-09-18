
use super::cards::build_card_rows;
use super::dialogs::DialogSlot;
use super::events::AppEvent;
use crate::core::models::Port;
use crate::ui::font_sizing::{apply_mode_geometry, compute_mode_geometry, ModeGeometry};
use crate::ui::chrome;
use crate::{AppTray, AppWindow, RecentGame, Theme};
use slint::{ComponentHandle, Model};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) struct AppPaths {
    pub(crate) library_dir: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) config_dir: PathBuf,
    pub(crate) saves_backup_dir: PathBuf,
    pub(crate) themes_path: PathBuf,
}

pub(crate) struct ThemeState {
    pub(crate) theme_config: RefCell<crate::ui::theme::ThemeConfig>,
    pub(crate) semantic: crate::ui::theme::SemanticColors,
    pub(crate) font_family: String,
}

pub(crate) struct WindowGeometry {
    pub(crate) border_width: Cell<i32>,
    pub(crate) window_width_fraction: Cell<f64>,
    pub(crate) scale: Cell<f32>,
    pub(crate) normal_mode: RefCell<ModeGeometry>,
    pub(crate) fullscreen_mode: RefCell<ModeGeometry>,
}

pub(crate) struct GridNav {
    pub(crate) grid_columns: Cell<usize>,
    pub(crate) displayed_installed: RefCell<Vec<Port>>,
    pub(crate) grid_selected: Cell<(usize, usize)>,
    pub(crate) grid_mouse_active: Cell<bool>,
    pub(crate) last_card_click: Cell<Option<((usize, usize), Instant)>>,
    pub(crate) double_click_ms: u32,
    pub(crate) card_image_cache: RefCell<HashMap<String, slint::Image>>,
}

pub(crate) struct WindowedNav {
    pub(crate) displayed_windowed: RefCell<Vec<Port>>,
    pub(crate) windowed_selected: Cell<usize>,
    pub(crate) search_query: RefCell<String>,
}

pub(crate) struct InstallRuntime {
    pub(crate) installing: RefCell<HashSet<String>>,
    pub(crate) running_processes: RefCell<HashMap<String, crate::core::launch::LaunchedProcess>>,
    pub(crate) launch_started_at: RefCell<HashMap<String, Instant>>,
    pub(crate) discord_presence: RefCell<HashMap<String, crate::core::discord_presence::PresenceHandle>>,
    pub(crate) pending_launch_after_install: RefCell<HashSet<String>>,
    pub(crate) minimized_for_game: Cell<bool>,
}

pub(crate) struct DialogNav {
    pub(crate) dialogs: RefCell<DialogSlot>,
    pub(crate) picker_index: Cell<i32>,
    pub(crate) info_nav_index: Cell<i32>,
    pub(crate) confirm_nav_index: Cell<i32>,
    pub(crate) error_nav_index: Cell<i32>,
    pub(crate) info_dialog_port_key: RefCell<Option<String>>,
}

pub(crate) struct AppState {
    pub(crate) window: slint::Weak<AppWindow>,
    pub(crate) tray: RefCell<Option<slint::Weak<AppTray>>>,
    pub(crate) state: RefCell<crate::core::state::StateManager>,
    pub(crate) catalog: RefCell<Vec<Port>>,
    pub(crate) paths: AppPaths,
    pub(crate) theme: ThemeState,
    pub(crate) window_geometry: WindowGeometry,
    pub(crate) grid_nav: GridNav,
    pub(crate) windowed_nav: WindowedNav,
    pub(crate) install_runtime: InstallRuntime,
    pub(crate) dialog_nav: DialogNav,
    pub(crate) events: Arc<Mutex<Vec<AppEvent>>>,
    pub(crate) stress_test: bool,
}

impl AppState {
    pub(crate) fn window(&self) -> AppWindow {
        self.window.unwrap()
    }

    pub(crate) fn recent_games(&self) -> Vec<Port> {
        let state = self.state.borrow();
        let mut games: Vec<(String, Port)> = self
            .catalog
            .borrow()
            .iter()
            .filter_map(|port| {
                let info = state.get(port.key())?;
                (!info.last_played_at.is_empty()).then(|| (info.last_played_at.clone(), port.clone()))
            })
            .collect();
        games.sort_by(|a, b| b.0.cmp(&a.0));
        games.truncate(5);
        games.into_iter().map(|(_, port)| port).collect()
    }

    pub(crate) fn refresh_tray_recent_games(&self) {
        let Some(tray) = self.tray.borrow().as_ref().and_then(slint::Weak::upgrade) else { return };
        let games: Vec<RecentGame> =
            self.recent_games().iter().map(|p| RecentGame { name: p.display_name().into(), key: p.key().into() }).collect();
        tray.set_recent_games(slint::ModelRc::new(slint::VecModel::from(games)));
    }

    pub(crate) fn api_tokens(&self) -> (Option<String>, Option<String>) {
        let s = self.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    }

    pub(crate) fn current_selected_port(&self) -> Option<Port> {
        if self.window().get_big_mode() {
            let (row, col) = self.grid_nav.grid_selected.get();
            let idx = row * self.grid_nav.grid_columns.get().max(1) + col;
            self.grid_nav.displayed_installed.borrow().get(idx).cloned()
        } else {
            self.windowed_nav.displayed_windowed.borrow().get(self.windowed_nav.windowed_selected.get()).cloned()
        }
    }

    fn sort_by_recency(state: &crate::core::state::StateManager, ports: &mut [&Port]) {
        ports.sort_by(|a, b| {
            let a_played = state.get(a.key()).map(|i| i.last_played_at.as_str()).unwrap_or("");
            let b_played = state.get(b.key()).map(|i| i.last_played_at.as_str()).unwrap_or("");
            b_played.cmp(a_played)
        });
    }

    pub(crate) fn rebuild_windowed(&self, query: &str) {
        *self.windowed_nav.search_query.borrow_mut() = query.to_string();
        let previously_selected_key =
            self.windowed_nav.displayed_windowed.borrow().get(self.windowed_nav.windowed_selected.get()).map(|p| p.key().to_string());
        let catalog = self.catalog.borrow();
        let pool: Vec<&Port> = catalog.iter().filter(|p| !p.is_android_only()).collect();
        let mut filtered = crate::core::search::filter_and_sort(&pool, query);
        if query.trim().is_empty() {
            Self::sort_by_recency(&self.state.borrow(), &mut filtered);
        }
        let displayed: Vec<Port> = filtered.iter().map(|p| (*p).clone()).collect();
        let new_index = previously_selected_key.and_then(|k| displayed.iter().position(|p| p.key() == k)).unwrap_or(0);
        *self.windowed_nav.displayed_windowed.borrow_mut() = displayed;
        self.windowed_nav.windowed_selected.set(new_index);
        self.rebuild_ports_model();
    }

    pub(crate) fn rebuild_ports_model(&self) {
        let displayed = self.windowed_nav.displayed_windowed.borrow();
        let refs: Vec<&Port> = displayed.iter().collect();
        let items = crate::to_port_items(&refs, &self.paths.library_dir, &self.state.borrow());
        self.window().set_ports(slint::ModelRc::new(slint::VecModel::from(items)));
        self.push_selected_index();
    }

    pub(crate) fn push_selected_index(&self) {
        let len = self.windowed_nav.displayed_windowed.borrow().len();
        let window = self.window();
        window.set_selected_index(if len == 0 { -1 } else { self.windowed_nav.windowed_selected.get() as i32 });
    }

    pub(crate) fn move_windowed_selection(&self, dy: i32) {
        let len = self.windowed_nav.displayed_windowed.borrow().len();
        if len == 0 {
            return;
        }
        let current = self.windowed_nav.windowed_selected.get() as i32;
        let next = (current + dy).clamp(0, len as i32 - 1) as usize;
        if next != self.windowed_nav.windowed_selected.get() {
            self.windowed_nav.windowed_selected.set(next);
            self.push_selected_index();
            self.trigger_scroll();
        }
    }

    pub(crate) fn trigger_scroll(&self) {
        let window = self.window();
        window.set_scroll_trigger(!window.get_scroll_trigger());
    }

    pub(crate) fn rebuild_grid(&self, preserve_selection: bool) {
        let query = self.windowed_nav.search_query.borrow().clone();
        let catalog = self.catalog.borrow();
        let installed_refs: Vec<&Port> = catalog
            .iter()
            .filter(|p| !p.is_android_only() && crate::core::installer::is_installed(p, &self.paths.library_dir))
            .collect();
        let mut ranked = crate::core::search::filter_and_sort(&installed_refs, &query);
        if query.trim().is_empty() {
            Self::sort_by_recency(&self.state.borrow(), &mut ranked);
        }
        let installed: Vec<Port> = ranked.iter().map(|p| (*p).clone()).collect();
        let columns = self.grid_nav.grid_columns.get().max(1);
        let previously_selected_key = if preserve_selection {
            let (row, col) = self.grid_nav.grid_selected.get();
            self.grid_nav.displayed_installed.borrow().get(row * columns + col).map(|p| p.key().to_string())
        } else {
            None
        };
        let new_flat = previously_selected_key.and_then(|k| installed.iter().position(|p| p.key() == k)).unwrap_or(0);
        self.grid_nav.grid_selected.set((new_flat / columns, new_flat % columns));
        self.grid_nav.grid_mouse_active.set(true);
        self.window().set_card_rows(slint::ModelRc::new(slint::VecModel::from(build_card_rows(
            &self.grid_nav.card_image_cache,
            &installed,
            &self.paths.cache_dir,
            self.grid_nav.grid_columns.get(),
            Some(self.grid_nav.grid_selected.get()),
        ))));
        *self.grid_nav.displayed_installed.borrow_mut() = installed;
    }

    pub(crate) fn enter_fullscreen(&self) {
        self.recompute_grid_columns();
        self.rebuild_grid(false);
    }

    fn recompute_grid_columns(&self) {
        let (_, _, screen_w, _) = chrome::work_area_under_cursor();
        let grid_available_width = screen_w as f32 / self.window_geometry.scale.get();
        let columns = crate::core::grid::compute_grid_columns(grid_available_width);
        self.grid_nav.grid_columns.set(columns);
        self.window().set_grid_columns(columns as i32);
    }

    fn set_card_highlight(window: &AppWindow, pos: Option<(usize, usize)>, selected: bool) {
        let Some((row, col)) = pos else { return };
        let Some(card_row) = window.get_card_rows().row_data(row) else { return };
        let Some(mut item) = card_row.cards.row_data(col) else { return };
        if item.selected != selected {
            item.selected = selected;
            card_row.cards.set_row_data(col, item);
        }
    }

    pub(crate) fn refresh_grid_selection(&self, previous: Option<(usize, usize)>) {
        let window = self.window();
        let current = self.grid_nav.grid_mouse_active.get().then(|| self.grid_nav.grid_selected.get());
        if previous != current {
            Self::set_card_highlight(&window, previous, false);
            Self::set_card_highlight(&window, current, true);
        }
        let displayed = self.grid_nav.displayed_installed.borrow();
        window.set_grid_selected_row(if displayed.is_empty() { -1 } else { self.grid_nav.grid_selected.get().0 as i32 });
    }

    pub(crate) fn move_grid_selection(&self, dx: i32, dy: i32) {
        let len = self.grid_nav.displayed_installed.borrow().len();
        let columns = self.grid_nav.grid_columns.get().max(1);
        let Some(next) = crate::core::grid::next_grid_position(self.grid_nav.grid_selected.get(), dx, dy, columns, len) else {
            return;
        };
        if next != self.grid_nav.grid_selected.get() {
            let previous = self.grid_nav.grid_mouse_active.get().then(|| self.grid_nav.grid_selected.get());
            self.grid_nav.grid_selected.set(next);
            self.grid_nav.grid_mouse_active.set(true);
            self.refresh_grid_selection(previous);
            self.trigger_scroll();
        }
    }

    pub(crate) fn refresh_current_view(&self) {
        if self.window().get_big_mode() {
            self.rebuild_grid(true);
        } else {
            let query = self.windowed_nav.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    fn compute_live_mode(&self, big_mode: bool) -> ModeGeometry {
        let window = self.window();
        let scale = window.window().scale_factor();
        window.global::<Theme>().set_scale_factor(scale);
        self.window_geometry.scale.set(scale);
        compute_mode_geometry(
            &self.theme.font_family,
            chrome::work_area_under_cursor(),
            scale,
            big_mode,
            self.window_geometry.window_width_fraction.get(),
            self.window_geometry.border_width.get(),
        )
    }

    pub(crate) fn toggle_fullscreen(&self) {
        let now_big = !self.state.borrow().fullscreen;
        self.state.borrow_mut().set_fullscreen(now_big);
        let mode = self.compute_live_mode(now_big);
        let window = self.window();
        if now_big {
            *self.window_geometry.fullscreen_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.fullscreen_mode.borrow());
        } else {
            *self.window_geometry.normal_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.normal_mode.borrow());
        }
        window.set_big_mode(now_big);
        if now_big {
            self.enter_fullscreen();
        } else {
            let query = self.windowed_nav.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    pub(crate) fn refresh_geometry_if_scale_changed(&self) {
        let window = self.window();
        if window.window().scale_factor() == self.window_geometry.scale.get() {
            return;
        }
        let big_mode = window.get_big_mode();
        let mode = self.compute_live_mode(big_mode);
        if big_mode {
            *self.window_geometry.fullscreen_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.fullscreen_mode.borrow());
        } else {
            *self.window_geometry.normal_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.normal_mode.borrow());
        }
    }

    fn recompute_normal_mode(&self) {
        *self.window_geometry.normal_mode.borrow_mut() = self.compute_live_mode(false);
        if !self.window().get_big_mode() {
            let window = self.window();
            apply_mode_geometry(&window, &self.window_geometry.normal_mode.borrow());
        }
    }

    pub(crate) fn set_window_size_percent(&self, percent: i32) {
        let new_fraction = (percent as f64 / 100.0).clamp(0.05, 1.0);
        if new_fraction == self.window_geometry.window_width_fraction.get() {
            return;
        }
        self.window_geometry.window_width_fraction.set(new_fraction);
        self.recompute_normal_mode();
        self.state.borrow_mut().set_window_size(percent);
    }

    pub(crate) fn adjust_border(&self, delta: i32) {
        let new_border = (self.window_geometry.border_width.get() + delta).clamp(0, 100);
        if new_border == self.window_geometry.border_width.get() {
            return;
        }
        self.window_geometry.border_width.set(new_border);
        self.window().global::<Theme>().set_border_width(new_border);
        self.recompute_normal_mode();
        self.state.borrow_mut().set_border(new_border);
    }
}
