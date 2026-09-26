
use super::dialogs::{open_settings_dialog, DialogSlot};
use super::install_launch::{activate_selection, show_info_for_current_selection};
use super::state::AppState;
use crate::ui::gamepad_router::{GamepadRouter, GamepadTarget};
use crate::InfoDialog;
use slint::Model;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) const INFO_BUTTON_COUNT: usize = 10;

pub(crate) fn info_buttons_enabled(d: &InfoDialog) -> [bool; INFO_BUTTON_COUNT] {
    [
        d.get_change_version_enabled(),
        d.get_update_toggle_enabled(),
        d.get_favorite_exe_enabled(),
        d.get_extra_enabled(),
        d.get_reset_playtime_enabled(),
        d.get_game_folder_enabled(),
        d.get_save_folder_enabled(),
        d.get_save_folder2_enabled(),
        d.get_mods_enabled(),
        d.get_website_enabled(),
    ]
}

fn invoke_info_button(d: &InfoDialog, index: i32) {
    match index {
        0 => d.invoke_change_version_requested(),
        1 => d.invoke_update_toggle_requested(),
        2 => d.invoke_favorite_exe_requested(),
        3 => d.invoke_extra_requested(),
        4 => d.invoke_reset_playtime_requested(),
        5 => d.invoke_game_folder_requested(),
        6 => d.invoke_save_folder_requested(),
        7 => d.invoke_save_folder2_requested(),
        8 => d.invoke_mods_requested(),
        _ => d.invoke_website_requested(),
    }
}

fn current_dialog(app: &AppState) -> DialogSlot {
    app.dialog_nav.dialogs.borrow().clone_strong()
}

pub(crate) fn dialog_reject(app: &AppState) {
    match current_dialog(app) {
        DialogSlot::Message(d) => d.invoke_close_requested(),
        DialogSlot::Confirm(d) => d.invoke_close_requested(),
        DialogSlot::Error(d) => d.invoke_close_requested(),
        DialogSlot::Picker(d) => d.invoke_close_requested(),
        DialogSlot::Info(d) => d.invoke_close_requested(),
        DialogSlot::SearchList(d) => d.invoke_close_requested(),
        DialogSlot::Progress(_) | DialogSlot::None => {}
    }
}

pub(crate) fn dialog_activate(app: &AppState) {
    let nav = &app.dialog_nav;
    match current_dialog(app) {
        DialogSlot::Message(d) => d.invoke_close_requested(),
        DialogSlot::Confirm(d) if nav.confirm_nav_index.get() == 0 => d.invoke_confirmed(),
        DialogSlot::Confirm(d) => d.invoke_close_requested(),
        DialogSlot::Error(d) if nav.error_nav_index.get() == 0 => d.invoke_reinstall_requested(),
        DialogSlot::Error(d) => d.invoke_info_requested(),
        DialogSlot::Picker(d) => d.invoke_item_selected(nav.picker_index.get()),
        DialogSlot::SearchList(d) => d.invoke_item_selected(nav.picker_index.get()),
        DialogSlot::Info(d) => {
            let index = nav.info_nav_index.get();
            if info_buttons_enabled(&d).get(index as usize) == Some(&true) {
                invoke_info_button(&d, index);
            }
        }
        DialogSlot::Progress(_) | DialogSlot::None => {}
    }
}

fn step(current: i32, delta: i32, count: i32) -> Option<i32> {
    if count <= 0 {
        return None;
    }
    let next = (current + delta).clamp(0, count - 1);
    (next != current).then_some(next)
}

pub(crate) fn dialog_move_selection(app: &AppState, dx: i32, dy: i32) {
    let nav = &app.dialog_nav;
    match current_dialog(app) {
        DialogSlot::Confirm(d) => {
            if let Some(next) = step(nav.confirm_nav_index.get(), dy, 2) {
                nav.confirm_nav_index.set(next);
                d.set_selected_index(next);
            }
        }
        DialogSlot::Error(d) => {
            if let Some(next) = step(nav.error_nav_index.get(), dy, 2) {
                nav.error_nav_index.set(next);
                d.set_selected_index(next);
            }
        }
        DialogSlot::Picker(d) => {
            if let Some(next) = step(nav.picker_index.get(), dy, d.get_items().row_count() as i32) {
                nav.picker_index.set(next);
                d.set_selected_index(next);
                d.set_scroll_trigger(!d.get_scroll_trigger());
            }
        }
        DialogSlot::SearchList(d) => {
            if let Some(next) = step(nav.picker_index.get(), dy, d.get_items().row_count() as i32) {
                d.invoke_item_hovered(next);
                d.set_scroll_trigger(!d.get_scroll_trigger());
            }
        }
        DialogSlot::Info(d) => {
            if dy != 0 {
                d.invoke_scroll_instructions(dy);
            }
            if dx != 0 {
                let enabled = info_buttons_enabled(&d);
                let candidates: Vec<i32> = (0..INFO_BUTTON_COUNT as i32).filter(|&i| enabled[i as usize]).collect();
                let pos = candidates.iter().position(|&i| i == nav.info_nav_index.get()).unwrap_or(0) as i32;
                if let Some(next_pos) = step(pos, dx, candidates.len() as i32) {
                    let next = candidates[next_pos as usize];
                    nav.info_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
        }
        DialogSlot::Message(_) | DialogSlot::Progress(_) | DialogSlot::None => {}
    }
}

pub(crate) struct DialogGamepadTarget {
    pub(crate) app: Rc<AppState>,
}

impl GamepadTarget for DialogGamepadTarget {
    fn move_selection(&self, dx: i32, dy: i32) {
        dialog_move_selection(&self.app, dx, dy);
    }

    fn activate_selection(&self) {
        dialog_activate(&self.app);
    }

    fn reject(&self) {
        dialog_reject(&self.app);
    }

    fn show_info_for_selection(&self) {
        if let DialogSlot::Error(d) = current_dialog(&self.app) {
            d.invoke_info_requested();
        }
    }
}

pub(crate) struct AppGamepadTarget {
    pub(crate) app: Rc<AppState>,
    pub(crate) router: Rc<RefCell<GamepadRouter>>,
}

impl GamepadTarget for AppGamepadTarget {
    fn intercept(&self, input: Option<super::cheats::CheatInput>) -> bool {
        super::cheats::intercept(&self.app, input)
    }

    fn move_selection(&self, dx: i32, dy: i32) {
        self.app.move_selection(dx, dy);
    }

    fn activate_selection(&self) {
        activate_selection(&self.app, &self.router);
    }

    fn show_info_for_selection(&self) {
        show_info_for_current_selection(&self.app, &self.router);
    }

    fn toggle_fullscreen(&self) {
        self.app.toggle_fullscreen();
    }

    fn open_settings(&self) {
        open_settings_dialog(&self.app, &self.router);
    }
}
