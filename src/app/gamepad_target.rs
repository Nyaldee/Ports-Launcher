
use super::dialogs::DialogSlot;
use super::install_launch::{activate_selection, show_info_for_current_selection};
use super::state::AppState;
use crate::ui::gamepad_router::GamepadTarget;
use crate::{InfoDialog, PAGE_ROWS};
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) struct DialogGamepadTarget {
    pub(crate) app: Rc<AppState>,
}

fn cloned_dialog(app: &AppState) -> Option<DialogSlot> {
    match &*app.dialog_nav.dialogs.borrow() {
        DialogSlot::Message(d) => Some(DialogSlot::Message(d.clone_strong())),
        DialogSlot::Confirm(d) => Some(DialogSlot::Confirm(d.clone_strong())),
        DialogSlot::Error(d) => Some(DialogSlot::Error(d.clone_strong())),
        DialogSlot::Picker(d) => Some(DialogSlot::Picker(d.clone_strong())),
        DialogSlot::Info(d) => Some(DialogSlot::Info(d.clone_strong())),
        DialogSlot::SearchList(d) => Some(DialogSlot::SearchList(d.clone_strong())),
        DialogSlot::Progress(_) | DialogSlot::None => None,
    }
}

fn info_nav_enabled(d: &InfoDialog) -> [bool; 10] {
    [
        d.get_website_enabled(),
        d.get_mods_enabled(),
        d.get_game_folder_enabled(),
        d.get_save_folder_enabled(),
        d.get_save_folder2_enabled(),
        d.get_change_version_enabled(),
        d.get_favorite_exe_enabled(),
        d.get_update_toggle_enabled(),
        d.get_reset_playtime_enabled(),
        d.get_extra_enabled(),
    ]
}

impl GamepadTarget for DialogGamepadTarget {
    fn reject(&self) {
        match cloned_dialog(&self.app) {
            Some(DialogSlot::Message(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Confirm(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Error(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Picker(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Info(d)) => d.invoke_close_requested(),
            Some(DialogSlot::SearchList(d)) => d.invoke_close_requested(),
            _ => {}
        }
    }

    fn activate_selection(&self) {
        match cloned_dialog(&self.app) {
            Some(DialogSlot::Message(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Confirm(d)) => {
                if self.app.dialog_nav.confirm_nav_index.get() == 0 {
                    d.invoke_confirmed();
                } else {
                    d.invoke_close_requested();
                }
            }
            Some(DialogSlot::Error(d)) => {
                if self.app.dialog_nav.error_nav_index.get() == 0 {
                    d.invoke_reinstall_requested();
                } else {
                    d.invoke_info_requested();
                }
            }
            Some(DialogSlot::Picker(d)) => d.invoke_item_selected(self.app.dialog_nav.picker_index.get()),
            Some(DialogSlot::SearchList(d)) => d.invoke_item_selected(self.app.dialog_nav.picker_index.get()),
            Some(DialogSlot::Info(d)) if info_nav_enabled(&d)[self.app.dialog_nav.info_nav_index.get() as usize] => {
                match self.app.dialog_nav.info_nav_index.get() {
                    0 => d.invoke_website_requested(),
                    1 => d.invoke_mods_requested(),
                    2 => d.invoke_game_folder_requested(),
                    3 => d.invoke_save_folder_requested(),
                    4 => d.invoke_save_folder2_requested(),
                    5 => d.invoke_change_version_requested(),
                    6 => d.invoke_favorite_exe_requested(),
                    7 => d.invoke_update_toggle_requested(),
                    8 => d.invoke_reset_playtime_requested(),
                    _ => d.invoke_extra_requested(),
                }
            }
            _ => {}
        }
    }

    fn show_info_for_selection(&self) {
        let error_dialog = match &*self.app.dialog_nav.dialogs.borrow() {
            DialogSlot::Error(d) => Some(d.clone_strong()),
            _ => None,
        };
        if let Some(d) = error_dialog {
            d.invoke_info_requested();
        }
    }

    fn move_selection(&self, dx: i32, dy: i32) {
        match &*self.app.dialog_nav.dialogs.borrow() {
            DialogSlot::Confirm(d) => {
                if dy != 0 {
                    let next = (self.app.dialog_nav.confirm_nav_index.get() + dy).clamp(0, 1);
                    self.app.dialog_nav.confirm_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
            DialogSlot::Error(d) => {
                if dy != 0 {
                    let next = (self.app.dialog_nav.error_nav_index.get() + dy).clamp(0, 1);
                    self.app.dialog_nav.error_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
            DialogSlot::Picker(d) => {
                let count = d.get_items().row_count() as i32;
                if count == 0 {
                    return;
                }
                let next = (self.app.dialog_nav.picker_index.get() + dy).clamp(0, count - 1);
                self.app.dialog_nav.picker_index.set(next);
                d.set_selected_index(next);
                d.set_scroll_trigger(!d.get_scroll_trigger());
            }
            DialogSlot::Info(d) => {
                if dy != 0 {
                    d.invoke_scroll_instructions(dy);
                }
                if dx != 0 {
                    let enabled = info_nav_enabled(d);
                    let candidates: Vec<i32> = (0..10).filter(|&i| enabled[i as usize]).collect();
                    if !candidates.is_empty() {
                        let current = self.app.dialog_nav.info_nav_index.get();
                        let pos = candidates.iter().position(|&i| i == current).unwrap_or(0) as i32;
                        let next_pos = (pos + dx).clamp(0, candidates.len() as i32 - 1);
                        let next = candidates[next_pos as usize];
                        self.app.dialog_nav.info_nav_index.set(next);
                        d.set_selected_index(next);
                    }
                }
            }
            DialogSlot::SearchList(d) if dy != 0 => {
                let count = d.get_items().row_count() as i32;
                if count != 0 {
                    let next = (self.app.dialog_nav.picker_index.get() + dy).clamp(0, count - 1);
                    d.set_scroll_trigger(!d.get_scroll_trigger());
                    d.invoke_item_hovered(next);
                }
            }
            _ => {}
        }
    }
}

pub(crate) struct AppGamepadTarget {
    pub(crate) app: Rc<AppState>,
    pub(crate) router: Rc<RefCell<crate::ui::gamepad_router::GamepadRouter>>,
}

impl GamepadTarget for AppGamepadTarget {
    fn move_selection(&self, dx: i32, dy: i32) {
        if self.app.window().get_big_mode() {
            self.app.move_grid_selection(dx, dy);
        } else if dx != 0 {
            self.app.move_windowed_selection(dx * PAGE_ROWS);
        } else {
            self.app.move_windowed_selection(dy);
        }
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
}
