
use super::events::{lock, AppEvent};
use super::gamepad_target::DialogGamepadTarget;
use super::install_launch::{
    delete_port, open_favorite_exe_picker, open_path_if_exists, open_version_picker, start_extra_install, start_install,
};
use super::playtime::{format_last_played, format_playtime, LastPlayed};
use super::state::AppState;
use super::sync::launch_self_update;
use crate::core::models::{Port, SourceType};
use crate::ui::font_sizing::FontSizes;
use crate::ui::gamepad_router::{GamepadRouter, GamepadTarget};
use crate::ui::chrome;
use crate::{
    AppWindow, ConfirmDialog, ErrorDialog, InfoDialog, ListPickerDialog, MessageDialog, PickerItem, ProgressDialog, SearchListDialog, SemanticColors, Theme,
    Tr,
};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

macro_rules! apply_dialog_theme {
    ($dialog:expr, $app:expr) => {{
        $dialog.set_font_family($app.theme.font_family.clone().into());
        let g = $dialog.global::<Theme>();
        let current = $app.theme.theme_config.borrow().current;
        g.set_search_background(current.search_background);
        g.set_search_text(current.search_text);
        g.set_list_background(current.list_background);
        g.set_list_text(current.list_text);
        g.set_selected_background(current.selected_background);
        g.set_selected_text(current.selected_text);
        g.set_border_color(current.border);
        g.set_border_width($app.window_geometry.border_width.get());
        g.set_scale_factor($app.window_geometry.scale.get());
        let gc = $dialog.global::<SemanticColors>();
        gc.set_danger($app.theme.semantic.danger);
        gc.set_text_on_accent($app.theme.semantic.text_on_accent);
    }};
}

macro_rules! apply_dialog_chrome {
    ($dialog:expr, $fonts:expr) => {{
        $dialog.set_item_font_px_physical($fonts.item_font_px as f32);
        $dialog.set_title_font_px_physical($fonts.title_font_px as f32);
        $dialog.set_row_height_physical($fonts.row_height_px as f32);
        $dialog.set_title_bar_height_physical($fonts.title_bar_height_px as f32);
    }};
}

macro_rules! position_dialog {
    ($dialog:expr, $w:expr, $h:expr, $x:expr, $y:expr, $scale:expr) => {{
        $dialog.set_initial_width($w as f32 / $scale);
        $dialog.set_initial_height($h as f32 / $scale);
        $dialog.window().set_position(slint::WindowPosition::Physical(slint::PhysicalPosition { x: $x, y: $y }));
    }};
}

macro_rules! tr {
    ($app:expr) => {
        $app.window().global::<Tr>()
    };
}
pub(crate) use tr;

macro_rules! wire_dialog_close {
    ($dialog:expr, $app:expr, $router:expr) => {{
        let app2 = $app.clone();
        let router2 = $router.clone();
        $dialog.on_close_requested(move || close_current_dialog(&app2, &router2));
    }};
}

macro_rules! wire_dialog_selection_nav {
    ($dialog:expr, $app:expr, horizontal) => {{
        let app2 = $app.clone();
        $dialog.on_move_selection_requested(move |delta| {
            DialogGamepadTarget { app: app2.clone() }.move_selection(delta, 0);
        });
        let app3 = $app.clone();
        $dialog.on_activate_selection_requested(move || {
            DialogGamepadTarget { app: app3.clone() }.activate_selection();
        });
    }};
    ($dialog:expr, $app:expr, vertical) => {{
        let app2 = $app.clone();
        $dialog.on_move_selection_requested(move |delta| {
            DialogGamepadTarget { app: app2.clone() }.move_selection(0, delta);
        });
        let app3 = $app.clone();
        $dialog.on_activate_selection_requested(move || {
            DialogGamepadTarget { app: app3.clone() }.activate_selection();
        });
    }};
}

macro_rules! wire_dialog_nav_hovered {
    ($dialog:expr, $app:expr, $group:ident.$field:ident) => {{
        let app2 = $app.clone();
        let dialog_weak = $dialog.as_weak();
        $dialog.on_nav_hovered(move |index| {
            app2.$group.$field.set(index);
            if let Some(d) = dialog_weak.upgrade() {
                d.set_selected_index(index);
            }
        });
    }};
}

pub(crate) fn dialog_context(app: &Rc<AppState>) -> (FontSizes, slint::SharedString, i32, i32) {
    let big_mode = app.window().get_big_mode();
    let fonts = if big_mode { app.window_geometry.fullscreen_mode.borrow().fonts } else { app.window_geometry.normal_mode.borrow().fonts };
    let family = app.window().get_font_family();
    let (_, _, work_w, work_h) = chrome::work_area_under_cursor();
    (fonts, family, work_w, work_h)
}

pub(crate) fn apply_theme(window: &AppWindow, theme: &crate::ui::theme::ThemeConfig, border_width: i32) {
    let t = window.global::<Theme>();
    t.set_search_background(theme.current.search_background);
    t.set_search_text(theme.current.search_text);
    t.set_list_background(theme.current.list_background);
    t.set_list_text(theme.current.list_text);
    t.set_selected_background(theme.current.selected_background);
    t.set_selected_text(theme.current.selected_text);
    t.set_border_color(theme.current.border);
    t.set_border_width(border_width);

    let g = window.global::<SemanticColors>();
    g.set_selection_border(theme.semantic.border_strong);
    g.set_fallback_text(theme.current.list_text);
    g.set_card_background(theme.current.list_background);
    g.set_success(theme.semantic.success);
    g.set_success_hover(theme.semantic.success_hover);
    g.set_warning(theme.semantic.warning);
    g.set_warning_hover(theme.semantic.warning_hover);
    g.set_danger(theme.semantic.danger);
    g.set_danger_hover(theme.semantic.danger_hover);
    g.set_info(theme.semantic.info);
    g.set_info_hover(theme.semantic.info_hover);
    g.set_text_on_accent(theme.semantic.text_on_accent);
    g.set_brand_github(theme.semantic.brand_github);
    g.set_brand_github_hover(theme.semantic.brand_github_hover);
    g.set_brand_discord(theme.semantic.brand_discord);
    g.set_brand_discord_hover(theme.semantic.brand_discord_hover);
}

pub(crate) fn revert_theme_preview(app: &Rc<AppState>) {
    let name = app.state.borrow().active_theme.clone();
    crate::ui::theme::preview_theme(&mut app.theme.theme_config.borrow_mut(), &name);
    apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
}

pub(crate) enum DialogSlot {
    None,
    Message(MessageDialog),
    Confirm(ConfirmDialog),
    Error(ErrorDialog),
    Info(InfoDialog),
    Progress(ProgressDialog),
    Picker(ListPickerDialog),
    SearchList(SearchListDialog),
}

fn main_window_rect(app: &AppState) -> Option<(i32, i32, i32, i32)> {
    let window = app.window();
    if let Some(native) = chrome::native_window(window.window()) {
        if chrome::is_hidden_or_minimized(native) {
            return None;
        }
    }
    let pos = window.window().position();
    let size = window.window().size();
    Some((pos.x, pos.y, size.width as i32, size.height as i32))
}

pub(crate) fn main_window_size(app: &AppState) -> (i32, i32) {
    if let Some((_, _, w, h)) = main_window_rect(app) {
        return (w, h);
    }
    let window = app.window();
    let scale = app.window_geometry.scale.get();
    let mode = if window.get_big_mode() { app.window_geometry.fullscreen_mode.borrow() } else { app.window_geometry.normal_mode.borrow() };
    ((mode.logical_width * scale) as i32, (mode.logical_height * scale) as i32)
}

pub(crate) fn centered_position(app: &AppState, dialog_w: i32, dialog_h: i32) -> (i32, i32) {
    let (x, y, w, h) = main_window_rect(app).unwrap_or_else(chrome::work_area_under_cursor);
    crate::ui::dialog_geometry::center_over_parent(x, y, w, h, dialog_w, dialog_h)
}

pub(crate) fn close_current_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    app.dialog_nav.info_dialog_port_key.replace(None);
    let slot = app.dialog_nav.dialogs.replace(DialogSlot::None);
    let had_dialog = !matches!(slot, DialogSlot::None);
    match slot {
        DialogSlot::None => {}
        DialogSlot::Message(d) => {
            let _ = d.hide();
        }
        DialogSlot::Confirm(d) => {
            let _ = d.hide();
        }
        DialogSlot::Error(d) => {
            let _ = d.hide();
        }
        DialogSlot::Info(d) => {
            let _ = d.hide();
        }
        DialogSlot::Progress(d) => {
            let _ = d.hide();
        }
        DialogSlot::Picker(d) => {
            let _ = d.hide();
        }
        DialogSlot::SearchList(d) => {
            let _ = d.hide();
        }
    }
    if had_dialog {
        if let Some(native) = chrome::native_window(app.window().window()) {
            chrome::force_foreground_window(native);
        }
        router.borrow_mut().pop_target();
        let window = app.window();
        window.set_refocus_trigger(!window.get_refocus_trigger());
    }
}

pub(crate) fn push_dialog_target(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    router.borrow_mut().push_target(Rc::new(DialogGamepadTarget { app: app.clone() }));
}

pub(crate) fn wire_close_requested_cleanup(window: &slint::Window, app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let app = app.clone();
    let router = router.clone();
    window.on_close_requested(move || {
        close_current_dialog(&app, &router);
        slint::CloseRequestResponse::HideWindow
    });
}

pub(crate) fn dialog_window(slot: &DialogSlot) -> Option<&slint::Window> {
    match slot {
        DialogSlot::None => None,
        DialogSlot::Message(d) => Some(d.window()),
        DialogSlot::Confirm(d) => Some(d.window()),
        DialogSlot::Error(d) => Some(d.window()),
        DialogSlot::Info(d) => Some(d.window()),
        DialogSlot::Progress(d) => Some(d.window()),
        DialogSlot::Picker(d) => Some(d.window()),
        DialogSlot::SearchList(d) => Some(d.window()),
    }
}

pub(crate) fn finish_dialog_open(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, slot: DialogSlot) {
    if let Some(w) = dialog_window(&slot) {
        wire_close_requested_cleanup(w, app, router);
    }
    *app.dialog_nav.dialogs.borrow_mut() = slot;
    push_dialog_target(app, router);

    let app2 = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        let slot_ref = app2.dialog_nav.dialogs.borrow();
        let Some(w) = dialog_window(&slot_ref) else { return };
        let Some(native) = chrome::native_window(w) else { return };
        let pos = w.position();
        chrome::apply_window_icon(native);
        chrome::force_normal_window_visibility(native);
        if let Some(main_native) = chrome::native_window(app2.window().window()) {
            chrome::own_window(native, main_native);
            w.set_position(slint::WindowPosition::Physical(pos));
        }
        chrome::force_foreground_window(native);
    });
}

pub(crate) fn dialog_is_open(app: &AppState) -> bool {
    !matches!(*app.dialog_nav.dialogs.borrow(), DialogSlot::None)
}

pub(crate) fn open_message_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, title: &str, message: &str) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = crate::ui::dialog_geometry::message_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = MessageDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title.into());
    dialog.set_message_text(message.into());
    apply_dialog_chrome!(dialog, fonts);
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Message(dialog));
}

pub(crate) fn resize_progress_dialog(app: &Rc<AppState>, dialog: &ProgressDialog, status: &str) {
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = crate::ui::dialog_geometry::progress_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), status,
    );
    let (x, y) = centered_position(app, dw, dh);
    dialog.set_status_text(status.into());
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());
}

pub(crate) fn open_progress_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, title: &str, status: &str) {
    close_current_dialog(app, router);
    let (fonts, ..) = dialog_context(app);
    let Ok(dialog) = ProgressDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title.into());
    dialog.set_progress_fill_color(app.theme.semantic.success);
    apply_dialog_chrome!(dialog, fonts);
    resize_progress_dialog(app, &dialog, status);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Progress(dialog));
}

pub(crate) fn open_error_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let message = tr!(app).invoke_message_launch_failed(port.name.clone().into());
    let (dw, dh) = crate::ui::dialog_geometry::error_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), &message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ErrorDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(dialog.global::<Tr>().invoke_dialog_title_error_port(port.name.clone().into()));
    dialog.set_message_text(message);
    apply_dialog_chrome!(dialog, fonts);
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());
    app.dialog_nav.error_nav_index.set(0);
    dialog.set_selected_index(0);
    wire_dialog_nav_hovered!(dialog, app, dialog_nav.error_nav_index);
    wire_dialog_selection_nav!(dialog, app, vertical);
    {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_reinstall_requested(move || {
            close_current_dialog(&app2, &router2);
            start_install(&app2, &router2, port2.clone(), None, None);
        });
    }
    {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_info_requested(move || open_info_dialog(&app2, &router2, &port2));
    }
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Error(dialog));
}

fn open_confirm_dialog(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    title: slint::SharedString,
    message: slint::SharedString,
    confirm_label: Option<slint::SharedString>,
    on_confirmed: impl Fn(&Rc<AppState>, &Rc<RefCell<GamepadRouter>>) + 'static,
) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = crate::ui::dialog_geometry::error_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), &message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ConfirmDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title);
    dialog.set_message_text(message);
    if let Some(label) = confirm_label {
        dialog.set_confirm_text(label);
    }
    apply_dialog_chrome!(dialog, fonts);
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());
    app.dialog_nav.confirm_nav_index.set(0);
    dialog.set_selected_index(0);
    wire_dialog_nav_hovered!(dialog, app, dialog_nav.confirm_nav_index);
    wire_dialog_selection_nav!(dialog, app, vertical);
    {
        let app2 = app.clone();
        let router2 = router.clone();
        dialog.on_confirmed(move || {
            close_current_dialog(&app2, &router2);
            on_confirmed(&app2, &router2);
        });
    }
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Confirm(dialog));
}

pub(crate) fn open_uninstall_confirm_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let title = tr!(app).invoke_dialog_title_uninstall_port(port.name.clone().into());
    let message = tr!(app).invoke_message_uninstall_confirm(port.name.clone().into());
    let port2 = port.clone();
    open_confirm_dialog(app, router, title, message, None, move |app, router| delete_port(app, router, &port2));
}

pub(crate) fn open_info_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    close_current_dialog(app, router);
    app.dialog_nav.info_dialog_port_key.replace(Some(port.key().to_string()));
    let (dw, dh) = main_window_size(app);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = InfoDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(dialog.global::<Tr>().invoke_dialog_title_info_port(port.name.clone().into()));
    apply_dialog_chrome!(dialog, app.window_geometry.normal_mode.borrow().fonts);

    let tr = dialog.global::<Tr>();
    let version_text = match app.state.borrow().get(port.key()) {
        None => tr.invoke_version_not_installed(),
        Some(info) => match (port.source_type, &info.installed_tag) {
            (SourceType::Github | SourceType::Gitlab, Some(tag)) => tr.invoke_version_tag(tag.clone().into()),
            (SourceType::Github, None) => tr.invoke_version_installed_tag_unknown_github(),
            (SourceType::Gitlab, None) => tr.invoke_version_installed_tag_unknown_gitlab(),
            (SourceType::DirectUrl | SourceType::Local, _) => tr.invoke_version_installed_no_tracking(),
        },
    };
    dialog.set_version_text(version_text);
    dialog.set_instructions_text(port.instructions.clone().into());
    if let Some(link) = port.instructions_link() {
        dialog.set_instructions_link(link.into());
        let link = link.to_string();
        dialog.on_instructions_link_requested(move || crate::core::launch::open_url(&link));
    }

    let website_url = port.website_url().map(str::to_string);
    let website_ok = website_url.as_deref().map(|u| u.starts_with("http://") || u.starts_with("https://")).unwrap_or(false);
    dialog.set_website_enabled(website_ok);

    let mods_ok = port.mods.as_deref().map(|u| u.starts_with("http://") || u.starts_with("https://")).unwrap_or(false);
    dialog.set_mods_enabled(mods_ok);

    let game_folder = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder).ok();
    let game_folder_ok = game_folder.as_ref().map(|p| p.exists()).unwrap_or(false);
    dialog.set_game_folder_enabled(game_folder_ok);

    let save_path: Option<std::path::PathBuf> = game_folder
        .as_deref()
        .and_then(|dir| port.save.as_ref().and_then(|v| crate::core::platform_resolve::resolve_save_folder(v, dir)));
    let save_ok = save_path.as_ref().map(|p| p.exists()).unwrap_or(false);
    dialog.set_save_folder_enabled(save_ok);

    let save2_path: Option<std::path::PathBuf> = game_folder
        .as_deref()
        .and_then(|dir| port.save2.as_ref().and_then(|v| crate::core::platform_resolve::resolve_save_folder(v, dir)));
    let save2_ok = save2_path.as_ref().map(|p| p.exists()).unwrap_or(false);
    dialog.set_save_folder2_enabled(save2_ok);

    let change_version_ok = matches!(port.source_type, SourceType::Github | SourceType::Gitlab) && port.repo.is_some();
    dialog.set_change_version_enabled(change_version_ok);

    let favorite_exe_ok =
        game_folder_ok && game_folder.as_deref().map(|dir| std::fs::read_dir(dir).is_ok_and(|mut it| it.next().is_some())).unwrap_or(false);
    dialog.set_favorite_exe_enabled(favorite_exe_ok);

    let update_toggle_ok = change_version_ok && game_folder_ok;
    dialog.set_update_toggle_enabled(update_toggle_ok);

    let reset_playtime_ok = game_folder_ok && app.state.borrow().get(port.key()).map(|i| i.playtime_seconds).unwrap_or(0) > 0;
    dialog.set_reset_playtime_enabled(reset_playtime_ok);

    let extra_ok = port.extra.is_some() && game_folder_ok;
    dialog.set_extra_enabled(extra_ok);

    let favorite_exe_status = match app.state.borrow().get(port.key()).and_then(|i| i.favorite_exe.clone()) {
        None => tr.invoke_favorite_exe_status_default(),
        Some(exe) => tr.invoke_favorite_exe_status_named(exe.into()),
    };
    dialog.set_favorite_exe_status_text(favorite_exe_status);
    let update_on = app.state.borrow().get(port.key()).map(|i| i.update).unwrap_or(true);
    dialog.set_update_status_text(if update_on { tr.invoke_update_status_on() } else { tr.invoke_update_status_off() });
    let last_played = app.state.borrow().get(port.key()).map(|i| format_last_played(&i.last_played_at)).unwrap_or(LastPlayed::Never);
    let last_played_status = match last_played {
        LastPlayed::Never => tr.invoke_last_played_status_never(),
        LastPlayed::Today => tr.invoke_last_played_status_today(),
        LastPlayed::Date(date) => tr.invoke_last_played_status(date.into()),
    };
    dialog.set_last_played_status_text(last_played_status);
    let playtime_seconds = app.state.borrow().get(port.key()).map(|i| i.playtime_seconds).unwrap_or(0);
    let playtime_status = if playtime_seconds == 0 {
        tr.invoke_playtime_status_never()
    } else {
        tr.invoke_playtime_status(format_playtime(playtime_seconds).into())
    };
    dialog.set_playtime_status_text(playtime_status);

    let first_enabled = [
        website_ok,
        mods_ok,
        game_folder_ok,
        save_ok,
        save2_ok,
        change_version_ok,
        favorite_exe_ok,
        update_toggle_ok,
        reset_playtime_ok,
        extra_ok,
    ]
    .iter()
    .position(|&ok| ok)
    .unwrap_or(0);
    app.dialog_nav.info_nav_index.set(first_enabled as i32);
    dialog.set_selected_index(first_enabled as i32);

    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());

    if website_ok {
        let url = website_url.unwrap();
        dialog.on_website_requested(move || crate::core::launch::open_url(&url));
    }
    if mods_ok {
        let url = port.mods.clone().unwrap();
        dialog.on_mods_requested(move || crate::core::launch::open_url(&url));
    }
    if game_folder_ok {
        let folder = game_folder.unwrap();
        dialog.on_game_folder_requested(move || crate::core::launch::open_path(&folder));
    }
    if save_ok {
        let folder = save_path.unwrap();
        dialog.on_save_folder_requested(move || crate::core::launch::open_path(&folder));
    }
    if save2_ok {
        let folder = save2_path.unwrap();
        dialog.on_save_folder2_requested(move || crate::core::launch::open_path(&folder));
    }
    if change_version_ok {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_change_version_requested(move || open_version_picker(&app2, &router2, port2.clone()));
    }
    if favorite_exe_ok {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_favorite_exe_requested(move || open_favorite_exe_picker(&app2, &router2, port2.clone()));
    }
    if update_toggle_ok {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_update_toggle_requested(move || open_update_toggle_dialog(&app2, &router2, port2.clone()));
    }
    if reset_playtime_ok {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_reset_playtime_requested(move || open_reset_playtime_dialog(&app2, &router2, port2.clone()));
    }
    if extra_ok {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_extra_requested(move || open_extra_install_confirm_dialog(&app2, &router2, port2.clone()));
    }
    wire_dialog_close!(dialog, app, router);
    wire_dialog_nav_hovered!(dialog, app, dialog_nav.info_nav_index);
    wire_dialog_selection_nav!(dialog, app, horizontal);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Info(dialog));
}

pub(crate) fn open_update_toggle_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let window = app.window();
    let tr = window.global::<Tr>();
    let title = tr.invoke_dialog_title_toggle_update(port.name.clone().into());
    let labels = vec![tr.invoke_confirm_enable().to_string(), tr.invoke_confirm_disable().to_string()];
    let key = port.key().to_string();
    open_picker_dialog(app, router, &title, labels, move |app, _router, idx| {
        app.state.borrow_mut().set_port_update(&key, idx == 0);
        app.refresh_current_view();
    });
}

pub(crate) fn open_extra_install_confirm_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let title = tr!(app).invoke_dialog_title_install_extras(port.name.clone().into());
    let message = tr!(app).invoke_message_install_extras_confirm(port.name.clone().into());
    let confirm_label = tr!(app).invoke_confirm_install();
    open_confirm_dialog(app, router, title, message, Some(confirm_label), move |app, router| {
        start_extra_install(app, router, port.clone());
    });
}

pub(crate) fn open_reset_playtime_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let title = tr!(app).invoke_dialog_title_reset_playtime(port.name.clone().into());
    let message = tr!(app).invoke_message_reset_playtime_confirm(port.name.clone().into());
    let confirm_label = tr!(app).invoke_confirm_reset();
    let key = port.key().to_string();
    open_confirm_dialog(app, router, title, message, Some(confirm_label), move |app, _router| {
        app.state.borrow_mut().reset_playtime(&key);
    });
}

fn build_picker_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, title: &str, labels: &[String]) -> Option<ListPickerDialog> {
    close_current_dialog(app, router);
    let big_mode = app.window().get_big_mode();
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let item_height = crate::ui::dialog_geometry::list_picker_item_height(work_h, big_mode);
    let (dw, dh) = crate::ui::dialog_geometry::list_picker_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, labels, big_mode, fonts.title_bar_height_px, app.window_geometry.border_width.get(),
    );
    let (x, y) = centered_position(app, dw, dh);
    let dialog = ListPickerDialog::new().ok()?;
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title.into());
    set_picker_items(&dialog, labels);
    dialog.set_item_height_physical(item_height as f32);
    apply_dialog_chrome!(dialog, fonts);
    dialog.set_selected_index(0);
    app.dialog_nav.picker_index.set(0);
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());
    {
        let app2 = app.clone();
        let dialog_weak = dialog.as_weak();
        dialog.on_item_hovered(move |index| {
            app2.dialog_nav.picker_index.set(index);
            if let Some(d) = dialog_weak.upgrade() {
                d.set_selected_index(index);
            }
        });
    }
    wire_dialog_selection_nav!(dialog, app, vertical);
    wire_dialog_close!(dialog, app, router);
    Some(dialog)
}

fn set_picker_items(dialog: &ListPickerDialog, labels: &[String]) {
    let items: Vec<PickerItem> = labels.iter().map(|label| PickerItem { label: label.clone().into() }).collect();
    dialog.set_items(slint::ModelRc::new(slint::VecModel::from(items)));
}

pub(crate) fn open_picker_dialog(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    title: &str,
    labels: Vec<String>,
    on_select: impl Fn(&Rc<AppState>, &Rc<RefCell<GamepadRouter>>, usize) + 'static,
) {
    let Some(dialog) = build_picker_dialog(app, router, title, &labels) else { return };
    {
        let app2 = app.clone();
        let router2 = router.clone();
        dialog.on_item_selected(move |index| {
            close_current_dialog(&app2, &router2);
            on_select(&app2, &router2, index as usize);
        });
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Picker(dialog));
}

fn settings_labels(app: &AppState) -> Vec<String> {
    let window = app.window();
    let tr = window.global::<Tr>();
    let check_updates_label =
        if app.state.borrow().release_sync { tr.invoke_label_check_updates_on() } else { tr.invoke_label_check_updates_off() };
    let discord_rpc_label =
        if app.state.borrow().discord_rpc_enabled { tr.invoke_label_discord_rpc_on() } else { tr.invoke_label_discord_rpc_off() };
    vec![
        tr.invoke_label_themes().to_string(),
        tr.invoke_label_language().to_string(),
        tr.invoke_label_files().to_string(),
        tr.invoke_label_library().to_string(),
        tr.invoke_label_backup_saves().to_string(),
        check_updates_label.to_string(),
        tr.invoke_label_force_update().to_string(),
        discord_rpc_label.to_string(),
    ]
}

pub(crate) fn open_settings_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if dialog_is_open(app) {
        return;
    }
    let labels = settings_labels(app);
    let title = tr!(app).invoke_dialog_title_settings();
    let Some(dialog) = build_picker_dialog(app, router, &title, &labels) else { return };
    {
        let app2 = app.clone();
        let router2 = router.clone();
        let dialog2 = dialog.clone_strong();
        dialog.on_item_selected(move |index| match index {
            5 => toggle_release_sync(&app2, &dialog2),
            7 => toggle_discord_rpc(&app2, &dialog2),
            idx => {
                close_current_dialog(&app2, &router2);
                match idx {
                    0 => open_theme_picker(&app2, &router2),
                    1 => open_language_picker(&app2, &router2),
                    2 => open_files_picker(&app2, &router2),
                    3 => open_path_if_exists(&app2.paths.library_dir),
                    4 => start_save_backup(&app2, &router2),
                    _ => force_update(&app2, &router2),
                }
            }
        });
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Picker(dialog));
}

pub(crate) fn force_update(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    launch_self_update(app, router);
}

fn toggle_release_sync(app: &Rc<AppState>, dialog: &ListPickerDialog) {
    let new_value = !app.state.borrow().release_sync;
    app.state.borrow_mut().set_release_sync(new_value);
    set_picker_items(dialog, &settings_labels(app));
}

fn toggle_discord_rpc(app: &Rc<AppState>, dialog: &ListPickerDialog) {
    let new_value = !app.state.borrow().discord_rpc_enabled;
    app.state.borrow_mut().set_discord_rpc_enabled(new_value);
    set_picker_items(dialog, &settings_labels(app));
}

type HoverFn = Box<dyn Fn(&Rc<AppState>, &SearchListDialog, &str) + 'static>;
type CloseWithoutSelectFn = Box<dyn Fn(&Rc<AppState>) + 'static>;

#[allow(clippy::too_many_arguments)]
fn open_search_list_dialog(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    title: slint::SharedString,
    items: Vec<(String, String)>,
    start_value: &str,
    on_hover: Option<HoverFn>,
    on_select: impl Fn(&Rc<AppState>, &Rc<RefCell<GamepadRouter>>, &str) + 'static,
    on_close_without_select: Option<CloseWithoutSelectFn>,
) {
    close_current_dialog(app, router);
    let (fonts, _family, _work_w, _work_h) = dialog_context(app);
    let (dw, dh) = main_window_size(app);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = SearchListDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title);
    let picker_items: Vec<PickerItem> = items.iter().map(|(_, label)| PickerItem { label: label.clone().into() }).collect();
    dialog.set_items(slint::ModelRc::new(slint::VecModel::from(picker_items)));
    apply_dialog_chrome!(dialog, fonts);
    dialog.set_search_bar_height_physical(fonts.search_bar_height_px as f32);
    let has_live_preview = on_hover.is_some();
    if has_live_preview {
        dialog.set_placeholder_text(app.state.borrow().placeholder_text.clone().into());
        let show_clock = app.state.borrow().show_clock;
        dialog.set_show_clock(show_clock);
        if show_clock {
            dialog.set_clock_text(crate::core::clock::format_now().into());
        }
    }
    let start_index = items.iter().position(|(value, _)| value == start_value).unwrap_or(0) as i32;
    dialog.set_selected_index(start_index);
    app.dialog_nav.picker_index.set(start_index);
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());

    let displayed: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(items.clone()));
    let on_hover = Rc::new(on_hover);
    {
        let app2 = app.clone();
        let displayed2 = displayed.clone();
        let dialog_weak = dialog.as_weak();
        let on_hover2 = on_hover.clone();
        dialog.on_item_hovered(move |index| {
            app2.dialog_nav.picker_index.set(index);
            let Some(d) = dialog_weak.upgrade() else { return };
            d.set_selected_index(index);
            if let Some(hover) = on_hover2.as_ref() {
                if let Some((value, _)) = displayed2.borrow().get(index as usize) {
                    hover(&app2, &d, value);
                }
            }
        });
    }
    {
        let app2 = app.clone();
        let router2 = router.clone();
        let displayed2 = displayed.clone();
        dialog.on_item_selected(move |index| {
            let value = displayed2.borrow().get(index as usize).map(|(v, _)| v.clone());
            close_current_dialog(&app2, &router2);
            if let Some(value) = value {
                on_select(&app2, &router2, &value);
            }
        });
    }
    {
        let items2 = items;
        let displayed2 = displayed.clone();
        let app2 = app.clone();
        let dialog_weak = dialog.as_weak();
        dialog.on_search_changed(move |query| {
            let query = query.to_lowercase();
            let filtered: Vec<(String, String)> =
                items2.iter().filter(|(_, label)| label.to_lowercase().contains(&query)).cloned().collect();
            *displayed2.borrow_mut() = filtered.clone();
            let Some(d) = dialog_weak.upgrade() else { return };
            let picker_items: Vec<PickerItem> = filtered.iter().map(|(_, label)| PickerItem { label: label.clone().into() }).collect();
            d.set_items(slint::ModelRc::new(slint::VecModel::from(picker_items)));
            let next = if filtered.is_empty() { -1 } else { 0 };
            d.set_selected_index(next);
            app2.dialog_nav.picker_index.set(next);
            if has_live_preview && !filtered.is_empty() {
                d.invoke_item_hovered(0);
            }
        });
    }
    wire_dialog_selection_nav!(dialog, app, vertical);
    {
        let app2 = app.clone();
        let router2 = router.clone();
        dialog.on_close_requested(move || {
            if let Some(on_close) = &on_close_without_select {
                on_close(&app2);
            }
            close_current_dialog(&app2, &router2);
        });
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::SearchList(dialog));
}

pub(crate) fn open_theme_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let names = crate::ui::theme::list_theme_names(&app.theme.theme_config.borrow());
    if names.is_empty() {
        return;
    }
    let items: Vec<(String, String)> = names.into_iter().map(|n| (n.clone(), n)).collect();
    let active = app.state.borrow().active_theme.clone();
    let on_hover: HoverFn = Box::new(|app, dialog, name| {
        crate::ui::theme::preview_theme(&mut app.theme.theme_config.borrow_mut(), name);
        apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
        apply_dialog_theme!(dialog, app);
    });
    open_search_list_dialog(
        app,
        router,
        tr!(app).invoke_label_themes(),
        items,
        &active,
        Some(on_hover),
        |app, _router, name| app.state.borrow_mut().set_active_theme(name.to_string()),
        Some(Box::new(revert_theme_preview)),
    );
}

pub(crate) fn open_files_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let title = tr!(app).invoke_label_files();
    let labels =
        vec!["ports.json".to_string(), "ports.local.json".to_string(), "state.json".to_string(), "themes.json".to_string()];
    open_picker_dialog(app, router, &title, labels, move |app, _router, idx| match idx {
        0 => open_path_if_exists(&app.paths.config_dir.join("ports.json")),
        1 => open_path_if_exists(&app.paths.config_dir.join("ports.local.json")),
        2 => open_path_if_exists(&app.paths.config_dir.join("state.json")),
        _ => open_path_if_exists(&app.paths.themes_path),
    });
}

const LANGUAGES: &[(&str, &str)] = &[
    ("", ""),
    ("en", "English"),
    ("fr", "Français"),
    ("ja", "日本語"),
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文 (台灣)"),
    ("es", "Español"),
    ("de", "Deutsch"),
    ("pt-BR", "Português (Brasil)"),
    ("ru", "Русский"),
    ("ko", "한국어"),
    ("it", "Italiano"),
    ("ar", "العربية"),
    ("vi", "Tiếng Việt"),
    ("pl", "Polski"),
    ("tr", "Türkçe"),
    ("id", "Bahasa Indonesia"),
    ("uk", "Українська"),
    ("fa", "فارسی"),
    ("th", "ไทย"),
    ("ro", "Română"),
];

pub(crate) fn open_language_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let default_name = tr!(app).invoke_language_default_system().to_string();
    let items: Vec<(String, String)> = std::iter::once(("".to_string(), default_name))
        .chain(LANGUAGES.iter().skip(1).map(|(c, n)| (c.to_string(), n.to_string())))
        .collect();
    let current = app.state.borrow().language.clone();
    open_search_list_dialog(
        app,
        router,
        tr!(app).invoke_dialog_title_language(),
        items,
        &current,
        None,
        |app, _router, code| {
            let _ = slint::select_bundled_translation(code);
            app.state.borrow_mut().set_language(code.to_string());
        },
        None,
    );
}

pub(crate) fn start_save_backup(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if matches!(*app.dialog_nav.dialogs.borrow(), DialogSlot::Progress(_)) {
        return;
    }

    let window = app.window();
    let tr = window.global::<Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_saves_backup(), &tr.invoke_progress_backing_up_saves());

    let catalog = app.catalog.borrow().clone();
    let library_dir = app.paths.library_dir.clone();
    let saves_backup_dir = app.paths.saves_backup_dir.clone();
    let events = app.events.clone();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    std::thread::spawn(move || {
        let events_progress = events.clone();
        let mut on_progress = move |name: &str| {
            lock(&events_progress).push(AppEvent::SaveBackupProgress { name: name.to_string() });
        };
        let summary = crate::core::save_backup::run_global_backup(&catalog, &library_dir, &saves_backup_dir, &date, &mut on_progress);
        lock(&events).push(AppEvent::SaveBackupDone { copied: summary.copied, skipped: summary.skipped, failed: summary.failed });
    });
}
