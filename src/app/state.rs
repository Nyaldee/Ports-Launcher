
use super::cards::build_card_rows;
use super::dialogs::DialogSlot;
use super::events::{lock, AppEvent};
use crate::core::models::Port;
use crate::ui::chrome;
use crate::ui::font_sizing::{apply_mode_geometry, compute_mode_geometry, ModeGeometry};
use crate::{AppTray, AppWindow, PortItem, RecentGame, Theme};
use slint::{ComponentHandle, Model};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PAGE_ROWS: i32 = 10;
const RECENT_GAMES_COUNT: usize = 5;

pub(crate) struct AppPaths {
    pub(crate) library_dir: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) config_dir: PathBuf,
    pub(crate) saves_backup_dir: PathBuf,
    pub(crate) themes_path: PathBuf,
}

pub(crate) struct ThemeState {
    pub(crate) theme_config: RefCell<crate::ui::theme::ThemeConfig>,
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
    pub(crate) pending_image_fetches: RefCell<HashSet<String>>,
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
    pub(crate) konami: super::konami::Konami,
    pub(crate) events: Arc<Mutex<Vec<AppEvent>>>,
}

impl AppState {
    pub(crate) fn window(&self) -> AppWindow {
        self.window.unwrap()
    }

    pub(crate) fn find_port(&self, key: &str) -> Option<Port> {
        self.catalog.borrow().iter().find(|p| p.key() == key).cloned()
    }

    pub(crate) fn is_installed(&self, port: &Port) -> bool {
        crate::core::installer::is_installed(port, &self.paths.library_dir)
    }

    fn recent_games(&self) -> Vec<Port> {
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
        games.into_iter().take(RECENT_GAMES_COUNT).map(|(_, port)| port).collect()
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

    fn filter_for_display(&self, ports: Vec<&Port>, query: &str) -> Vec<Port> {
        let mut ranked = crate::core::search::filter_and_sort(&ports, query);
        if query.trim().is_empty() {
            let state = self.state.borrow();
            let last_played = |p: &Port| state.get(p.key()).map(|i| i.last_played_at.clone()).unwrap_or_default();
            ranked.sort_by_cached_key(|p| std::cmp::Reverse(last_played(p)));
        }
        ranked.into_iter().cloned().collect()
    }

    pub(crate) fn rebuild_windowed(&self, query: &str) {
        *self.windowed_nav.search_query.borrow_mut() = query.to_string();
        let previous_key =
            self.windowed_nav.displayed_windowed.borrow().get(self.windowed_nav.windowed_selected.get()).map(|p| p.key().to_string());
        let displayed = {
            let catalog = self.catalog.borrow();
            self.filter_for_display(catalog.iter().filter(|p| !p.is_android_only()).collect(), query)
        };
        let new_index = previous_key.and_then(|k| displayed.iter().position(|p| p.key() == k)).unwrap_or(0);
        *self.windowed_nav.displayed_windowed.borrow_mut() = displayed;
        self.windowed_nav.windowed_selected.set(new_index);
        self.rebuild_ports_model();
    }

    fn rebuild_ports_model(&self) {
        let items: Vec<PortItem> = {
            let state = self.state.borrow();
            self.windowed_nav
                .displayed_windowed
                .borrow()
                .iter()
                .map(|p| PortItem {
                    name: p.name.clone().into(),
                    auto_update_off: state.get(p.key()).is_some_and(|i| !i.update),
                    installed: self.is_installed(p),
                    is_local: p.user_managed,
                })
                .collect()
        };
        self.window().set_ports(slint::ModelRc::new(slint::VecModel::from(items)));
        self.push_selected_index();
    }

    pub(crate) fn push_selected_index(&self) {
        let empty = self.windowed_nav.displayed_windowed.borrow().is_empty();
        self.window().set_selected_index(if empty { -1 } else { self.windowed_nav.windowed_selected.get() as i32 });
    }

    pub(crate) fn move_selection(&self, dx: i32, dy: i32) {
        if self.window().get_big_mode() {
            self.move_grid_selection(dx, dy);
        } else {
            self.move_windowed_selection(if dx != 0 { dx * PAGE_ROWS } else { dy });
        }
    }

    fn move_windowed_selection(&self, delta: i32) {
        let len = self.windowed_nav.displayed_windowed.borrow().len();
        if len == 0 {
            return;
        }
        let current = self.windowed_nav.windowed_selected.get();
        let next = (current as i32 + delta).clamp(0, len as i32 - 1) as usize;
        if next != current {
            self.windowed_nav.windowed_selected.set(next);
            self.push_selected_index();
            self.trigger_scroll();
        }
    }

    pub(crate) fn select_displayed(&self, index: usize) {
        if self.window().get_big_mode() {
            if index >= self.grid_nav.displayed_installed.borrow().len() {
                return;
            }
            let columns = self.grid_nav.grid_columns.get().max(1);
            self.select_grid_cell((index / columns, index % columns));
        } else {
            if index >= self.windowed_nav.displayed_windowed.borrow().len() {
                return;
            }
            self.windowed_nav.windowed_selected.set(index);
            self.push_selected_index();
        }
        self.trigger_scroll();
    }

    fn trigger_scroll(&self) {
        let window = self.window();
        window.set_scroll_trigger(!window.get_scroll_trigger());
    }

    pub(crate) fn rebuild_grid(&self, preserve_selection: bool) {
        let query = self.windowed_nav.search_query.borrow().clone();
        let installed = {
            let catalog = self.catalog.borrow();
            self.filter_for_display(catalog.iter().filter(|p| !p.is_android_only() && self.is_installed(p)).collect(), &query)
        };
        let columns = self.grid_nav.grid_columns.get().max(1);
        let previous_key = preserve_selection.then(|| self.current_grid_key(columns)).flatten();
        let new_flat = previous_key.and_then(|k| installed.iter().position(|p| p.key() == k)).unwrap_or(0);
        self.grid_nav.grid_selected.set((new_flat / columns, new_flat % columns));
        self.grid_nav.grid_mouse_active.set(true);
        let rows = build_card_rows(&self.grid_nav.card_image_cache, &installed, &self.paths.cache_dir, columns, Some(self.grid_nav.grid_selected.get()));
        self.window().set_card_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        self.ensure_card_images_cached(&installed);
        *self.grid_nav.displayed_installed.borrow_mut() = installed;
    }

    fn current_grid_key(&self, columns: usize) -> Option<String> {
        let (row, col) = self.grid_nav.grid_selected.get();
        self.grid_nav.displayed_installed.borrow().get(row * columns + col).map(|p| p.key().to_string())
    }

    fn ensure_card_images_cached(&self, installed: &[Port]) {
        let missing: Vec<(String, String)> = installed
            .iter()
            .filter_map(|p| {
                let url = p.image.as_deref().filter(|u| crate::core::launch::is_web_url(u))?;
                let cached = crate::core::image_cache::cached_image_path(&self.paths.cache_dir, &p.folder).ok()?;
                if cached.exists() || !self.grid_nav.pending_image_fetches.borrow_mut().insert(p.folder.clone()) {
                    return None;
                }
                Some((url.to_string(), p.folder.clone()))
            })
            .collect();
        if missing.is_empty() {
            return;
        }
        let cache_dir = self.paths.cache_dir.clone();
        let events = self.events.clone();
        std::thread::spawn(move || {
            for (url, folder) in missing {
                crate::core::image_cache::cache_image(&url, &cache_dir, &folder);
                lock(&events).push(AppEvent::ImageCached { folder });
            }
        });
    }

    fn recompute_grid_columns(&self) {
        let (_, _, screen_w, _) = chrome::work_area_under_cursor();
        let columns = crate::core::grid::compute_grid_columns(screen_w as f32 / self.window_geometry.scale.get());
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
        let current = self.visible_grid_selection();
        if previous != current {
            Self::set_card_highlight(&window, previous, false);
            Self::set_card_highlight(&window, current, true);
        }
        let empty = self.grid_nav.displayed_installed.borrow().is_empty();
        window.set_grid_selected_row(if empty { -1 } else { self.grid_nav.grid_selected.get().0 as i32 });
    }

    pub(crate) fn visible_grid_selection(&self) -> Option<(usize, usize)> {
        self.grid_nav.grid_mouse_active.get().then(|| self.grid_nav.grid_selected.get())
    }

    pub(crate) fn select_grid_cell(&self, cell: (usize, usize)) {
        let previous = self.visible_grid_selection();
        self.grid_nav.grid_selected.set(cell);
        self.grid_nav.grid_mouse_active.set(true);
        self.refresh_grid_selection(previous);
    }

    fn move_grid_selection(&self, dx: i32, dy: i32) {
        let len = self.grid_nav.displayed_installed.borrow().len();
        let columns = self.grid_nav.grid_columns.get().max(1);
        let Some(next) = crate::core::grid::next_grid_position(self.grid_nav.grid_selected.get(), dx, dy, columns, len) else { return };
        if next != self.grid_nav.grid_selected.get() {
            self.select_grid_cell(next);
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

    fn apply_live_mode(&self, big_mode: bool) {
        let window = self.window();
        let scale = window.window().scale_factor();
        window.global::<Theme>().set_scale_factor(scale);
        self.window_geometry.scale.set(scale);
        let mode = compute_mode_geometry(
            &self.theme.font_family,
            chrome::work_area_under_cursor(),
            scale,
            big_mode,
            self.window_geometry.window_width_fraction.get(),
            self.window_geometry.border_width.get(),
        );
        chrome::set_fullscreen(window.window(), big_mode);
        apply_mode_geometry(&window, &mode);
        #[cfg(target_os = "linux")]
        if !big_mode {
            let weak = self.window.clone();
            let size = slint::LogicalSize::new(mode.logical_width, mode.logical_height);
            slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
                if let Some(window) = weak.upgrade().filter(|w| !w.get_big_mode()) {
                    window.window().set_size(size);
                }
            });
        }
        let slot = if big_mode { &self.window_geometry.fullscreen_mode } else { &self.window_geometry.normal_mode };
        *slot.borrow_mut() = mode;
    }

    pub(crate) fn toggle_fullscreen(&self) {
        let big_mode = !self.state.borrow().fullscreen;
        self.state.borrow_mut().set_fullscreen(big_mode);
        self.apply_live_mode(big_mode);
        self.window().set_big_mode(big_mode);
        if big_mode {
            self.enter_fullscreen();
        } else {
            let query = self.windowed_nav.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    pub(crate) fn enter_fullscreen(&self) {
        self.recompute_grid_columns();
        self.rebuild_grid(false);
    }

    pub(crate) fn refresh_geometry_if_scale_changed(&self) {
        let window = self.window();
        if window.window().scale_factor() != self.window_geometry.scale.get() {
            self.apply_live_mode(window.get_big_mode());
        }
    }

    fn recompute_normal_mode(&self) {
        if self.window().get_big_mode() {
            return;
        }
        self.apply_live_mode(false);
    }

    pub(crate) fn set_window_size_percent(&self, percent: i32) {
        let fraction = (percent as f64 / 100.0).clamp(0.05, 1.0);
        if fraction == self.window_geometry.window_width_fraction.get() {
            return;
        }
        self.window_geometry.window_width_fraction.set(fraction);
        self.recompute_normal_mode();
        self.state.borrow_mut().set_window_size(percent);
    }

    pub(crate) fn adjust_border(&self, delta: i32) {
        let border = (self.window_geometry.border_width.get() + delta).clamp(0, 100);
        if border == self.window_geometry.border_width.get() {
            return;
        }
        self.window_geometry.border_width.set(border);
        self.window().global::<Theme>().set_border_width(border);
        self.recompute_normal_mode();
        self.state.borrow_mut().set_border(border);
    }
}
