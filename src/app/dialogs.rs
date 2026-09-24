
use super::events::{lock, AppEvent};
use super::gamepad_target::{dialog_activate, dialog_move_selection, info_buttons_enabled, DialogGamepadTarget};
use super::install_launch::{delete_port, open_favorite_exe_picker, open_version_picker, start_extra_install, start_install};
use super::playtime::{format_last_played, format_playtime, LastPlayed};
use super::state::AppState;
use super::sync::launch_self_update;
use crate::core::launch::{is_web_url, open_file, open_folder, open_url};
use crate::core::models::{Port, SourceType};
use crate::ui::chrome;
use crate::ui::font_sizing::FontSizes;
use crate::ui::gamepad_router::GamepadRouter;
use crate::ui::theme::{SemanticPalette, ThemeColors, ThemeConfig};
use crate::{
    AppWindow, ConfirmDialog, ErrorDialog, InfoDialog, ListPickerDialog, MessageDialog, PickerItem, ProgressDialog, SearchListDialog,
    SemanticColors, Theme, Tr,
};
use serde_json::Value;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

type Router = Rc<RefCell<GamepadRouter>>;

fn push_theme(theme: &Theme, semantic: &SemanticColors, cfg: &ThemeConfig, border_width: i32) {
    push_colors(theme, semantic, &cfg.current, &cfg.semantic, border_width);
}

fn push_colors(theme: &Theme, semantic: &SemanticColors, c: &ThemeColors, s: &SemanticPalette, border_width: i32) {
    theme.set_search_background(c.search_background);
    theme.set_search_text(c.search_text);
    theme.set_list_background(c.list_background);
    theme.set_list_text(c.list_text);
    theme.set_selected_background(c.selected_background);
    theme.set_selected_text(c.selected_text);
    theme.set_border_color(c.border);
    theme.set_border_width(border_width);

    semantic.set_selection_border(s.border_strong);
    semantic.set_fallback_text(c.list_text);
    semantic.set_card_background(c.list_background);
    semantic.set_success(s.success);
    semantic.set_success_hover(s.success_hover);
    semantic.set_warning(s.warning);
    semantic.set_warning_hover(s.warning_hover);
    semantic.set_danger(s.danger);
    semantic.set_danger_hover(s.danger_hover);
    semantic.set_info(s.info);
    semantic.set_info_hover(s.info_hover);
    semantic.set_text_on_accent(s.text_on_accent);
    semantic.set_brand_github(s.brand_github);
    semantic.set_brand_github_hover(s.brand_github_hover);
    semantic.set_brand_discord(s.brand_discord);
    semantic.set_brand_discord_hover(s.brand_discord_hover);
}

pub(crate) fn apply_theme(window: &AppWindow, cfg: &ThemeConfig, border_width: i32) {
    push_theme(&window.global::<Theme>(), &window.global::<SemanticColors>(), cfg, border_width);
}

pub(crate) fn preview_colors(window: &AppWindow, cfg: &ThemeConfig, colors: &ThemeColors, border_width: i32) {
    push_colors(&window.global::<Theme>(), &window.global::<SemanticColors>(), colors, &cfg.semantic, border_width);
}

macro_rules! apply_dialog_theme {
    ($dialog:expr, $app:expr) => {
        push_theme(&$dialog.global::<Theme>(), &$dialog.global::<SemanticColors>(), &$app.theme.theme_config.borrow(), $app.window_geometry.border_width.get())
    };
}

macro_rules! style_dialog {
    ($dialog:expr, $app:expr, $fonts:expr) => {{
        let fonts: FontSizes = $fonts;
        $dialog.set_font_family($app.theme.font_family.clone().into());
        apply_dialog_theme!($dialog, $app);
        $dialog.global::<Theme>().set_scale_factor($app.window_geometry.scale.get());
        $dialog.set_item_font_px_physical(fonts.item_font_px as f32);
        $dialog.set_title_font_px_physical(fonts.title_font_px as f32);
        $dialog.set_row_height_physical(fonts.row_height_px as f32);
        $dialog.set_title_bar_height_physical(fonts.title_bar_height_px as f32);
    }};
}

macro_rules! position_dialog {
    ($dialog:expr, $app:expr, $w:expr, $h:expr, $x:expr, $y:expr) => {{
        let scale = $app.window_geometry.scale.get();
        $dialog.set_initial_width($w as f32 / scale);
        $dialog.set_initial_height($h as f32 / scale);
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
        let (app, router) = ($app.clone(), $router.clone());
        $dialog.on_close_requested(move || close_current_dialog(&app, &router));
    }};
}

macro_rules! wire_keyboard_nav {
    ($dialog:expr, $app:expr, $to_move:expr) => {{
        let app = $app.clone();
        $dialog.on_move_selection_requested(move |delta| {
            let (dx, dy) = $to_move(delta);
            dialog_move_selection(&app, dx, dy);
        });
        let app = $app.clone();
        $dialog.on_activate_selection_requested(move || dialog_activate(&app));
    }};
}

macro_rules! wire_nav_hovered {
    ($dialog:expr, $app:expr, $field:ident) => {{
        let app = $app.clone();
        let weak = $dialog.as_weak();
        $dialog.on_nav_hovered(move |index| {
            app.dialog_nav.$field.set(index);
            if let Some(d) = weak.upgrade() {
                d.set_selected_index(index);
            }
        });
    }};
}

fn vertical(delta: i32) -> (i32, i32) {
    (0, delta)
}

fn horizontal(delta: i32) -> (i32, i32) {
    (delta, 0)
}

fn dialog_context(app: &AppState) -> (FontSizes, slint::SharedString, i32, i32) {
    let window = app.window();
    let fonts = if window.get_big_mode() { app.window_geometry.fullscreen_mode.borrow().fonts } else { app.window_geometry.normal_mode.borrow().fonts };
    let (_, _, work_w, work_h) = chrome::work_area_under_cursor();
    (fonts, window.get_font_family(), work_w, work_h)
}

fn revert_theme_preview(app: &Rc<AppState>) {
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

impl DialogSlot {
    fn window(&self) -> Option<&slint::Window> {
        match self {
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

    pub(crate) fn clone_strong(&self) -> DialogSlot {
        match self {
            DialogSlot::None => DialogSlot::None,
            DialogSlot::Message(d) => DialogSlot::Message(d.clone_strong()),
            DialogSlot::Confirm(d) => DialogSlot::Confirm(d.clone_strong()),
            DialogSlot::Error(d) => DialogSlot::Error(d.clone_strong()),
            DialogSlot::Info(d) => DialogSlot::Info(d.clone_strong()),
            DialogSlot::Progress(d) => DialogSlot::Progress(d.clone_strong()),
            DialogSlot::Picker(d) => DialogSlot::Picker(d.clone_strong()),
            DialogSlot::SearchList(d) => DialogSlot::SearchList(d.clone_strong()),
        }
    }
}

fn main_window_rect(app: &AppState) -> Option<(i32, i32, i32, i32)> {
    let window = app.window();
    if chrome::native_window(window.window()).is_some_and(chrome::is_hidden_or_minimized) {
        return None;
    }
    let (pos, size) = (window.window().position(), window.window().size());
    Some((pos.x, pos.y, size.width as i32, size.height as i32))
}

fn main_window_size(app: &AppState) -> (i32, i32) {
    if let Some((_, _, w, h)) = main_window_rect(app) {
        return (w, h);
    }
    let scale = app.window_geometry.scale.get();
    let mode = if app.window().get_big_mode() { app.window_geometry.fullscreen_mode.borrow() } else { app.window_geometry.normal_mode.borrow() };
    ((mode.logical_width * scale) as i32, (mode.logical_height * scale) as i32)
}

fn centered_position(app: &AppState, dialog_w: i32, dialog_h: i32) -> (i32, i32) {
    let (x, y, w, h) = main_window_rect(app).unwrap_or_else(chrome::work_area_under_cursor);
    crate::ui::dialog_geometry::center_over_parent(x, y, w, h, dialog_w, dialog_h)
}

pub(crate) fn close_current_dialog(app: &Rc<AppState>, router: &Router) {
    app.dialog_nav.info_dialog_port_key.replace(None);
    let slot = app.dialog_nav.dialogs.replace(DialogSlot::None);
    let Some(window) = slot.window() else { return };
    let _ = window.hide();
    if let Some(native) = chrome::native_window(app.window().window()) {
        chrome::force_foreground_window(native);
    }
    router.borrow_mut().pop_target();
    let main = app.window();
    main.set_refocus_trigger(!main.get_refocus_trigger());
}

fn finish_dialog_open(app: &Rc<AppState>, router: &Router, slot: DialogSlot) {
    if let Some(window) = slot.window() {
        let (app, router) = (app.clone(), router.clone());
        window.on_close_requested(move || {
            close_current_dialog(&app, &router);
            slint::CloseRequestResponse::HideWindow
        });
    }
    *app.dialog_nav.dialogs.borrow_mut() = slot;
    router.borrow_mut().push_target(Rc::new(DialogGamepadTarget { app: app.clone() }));

    let app = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        let slot = app.dialog_nav.dialogs.borrow();
        let Some(window) = slot.window() else { return };
        let Some(native) = chrome::native_window(window) else { return };
        let position = window.position();
        chrome::apply_window_icon(native);
        chrome::force_normal_window_visibility(native);
        if let Some(main_native) = chrome::native_window(app.window().window()) {
            chrome::own_window(native, main_native);
            window.set_position(slint::WindowPosition::Physical(position));
        }
        chrome::force_foreground_window(native);
    });
}

pub(crate) fn dialog_is_open(app: &AppState) -> bool {
    !matches!(*app.dialog_nav.dialogs.borrow(), DialogSlot::None)
}

pub(crate) fn open_message_dialog(app: &Rc<AppState>, router: &Router, title: &str, message: &str) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = crate::ui::dialog_geometry::message_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = MessageDialog::new() else { return };
    style_dialog!(dialog, app, fonts);
    dialog.set_dialog_title(title.into());
    dialog.set_message_text(message.into());
    position_dialog!(dialog, app, dw, dh, x, y);
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Message(dialog));
}

pub(crate) fn update_progress_dialog(app: &AppState, dialog: &ProgressDialog, status: &str) {
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = crate::ui::dialog_geometry::progress_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), status,
    );
    let (x, y) = centered_position(app, dw, dh);
    dialog.set_status_text(status.into());
    position_dialog!(dialog, app, dw, dh, x, y);
}

pub(crate) fn open_progress_dialog(app: &Rc<AppState>, router: &Router, title: &str, status: &str) {
    close_current_dialog(app, router);
    let (fonts, ..) = dialog_context(app);
    let Ok(dialog) = ProgressDialog::new() else { return };
    style_dialog!(dialog, app, fonts);
    dialog.set_dialog_title(title.into());
    dialog.set_progress_fill_color(app.theme.theme_config.borrow().semantic.success);
    update_progress_dialog(app, &dialog, status);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Progress(dialog));
}

pub(crate) fn open_error_dialog(app: &Rc<AppState>, router: &Router, port: Port) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let message = tr!(app).invoke_message_launch_failed(port.name.clone().into());
    let (dw, dh) = crate::ui::dialog_geometry::error_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), &message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ErrorDialog::new() else { return };
    style_dialog!(dialog, app, fonts);
    dialog.set_dialog_title(dialog.global::<Tr>().invoke_dialog_title_error_port(port.name.clone().into()));
    dialog.set_message_text(message);
    position_dialog!(dialog, app, dw, dh, x, y);
    app.dialog_nav.error_nav_index.set(0);
    dialog.set_selected_index(0);
    wire_nav_hovered!(dialog, app, error_nav_index);
    wire_keyboard_nav!(dialog, app, vertical);
    {
        let (app, router, port) = (app.clone(), router.clone(), port.clone());
        dialog.on_reinstall_requested(move || {
            close_current_dialog(&app, &router);
            start_install(&app, &router, port.clone(), None, None);
        });
    }
    {
        let (app, router) = (app.clone(), router.clone());
        dialog.on_info_requested(move || open_info_dialog(&app, &router, &port));
    }
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Error(dialog));
}

fn open_confirm_dialog(
    app: &Rc<AppState>,
    router: &Router,
    title: slint::SharedString,
    message: slint::SharedString,
    confirm_label: slint::SharedString,
    on_confirmed: impl Fn(&Rc<AppState>, &Router) + 'static,
) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = crate::ui::dialog_geometry::error_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.window_geometry.border_width.get(), &message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ConfirmDialog::new() else { return };
    style_dialog!(dialog, app, fonts);
    dialog.set_dialog_title(title);
    dialog.set_message_text(message);
    dialog.set_confirm_text(confirm_label);
    position_dialog!(dialog, app, dw, dh, x, y);
    app.dialog_nav.confirm_nav_index.set(0);
    dialog.set_selected_index(0);
    wire_nav_hovered!(dialog, app, confirm_nav_index);
    wire_keyboard_nav!(dialog, app, vertical);
    {
        let (app, router) = (app.clone(), router.clone());
        dialog.on_confirmed(move || {
            close_current_dialog(&app, &router);
            on_confirmed(&app, &router);
        });
    }
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Confirm(dialog));
}

pub(crate) fn open_uninstall_confirm_dialog(app: &Rc<AppState>, router: &Router, port: Port) {
    let window = app.window();
    let tr = window.global::<Tr>();
    let name: slint::SharedString = port.name.clone().into();
    open_confirm_dialog(
        app,
        router,
        tr.invoke_dialog_title_uninstall_port(name.clone()),
        tr.invoke_message_uninstall_confirm(name),
        tr.invoke_confirm_uninstall(),
        move |app, router| delete_port(app, router, &port),
    );
}

fn open_extra_install_confirm_dialog(app: &Rc<AppState>, router: &Router, port: Port) {
    let window = app.window();
    let tr = window.global::<Tr>();
    let name: slint::SharedString = port.name.clone().into();
    open_confirm_dialog(
        app,
        router,
        tr.invoke_dialog_title_install_extras(name.clone()),
        tr.invoke_message_install_extras_confirm(name),
        tr.invoke_confirm_install(),
        move |app, router| start_extra_install(app, router, port.clone()),
    );
}

fn open_reset_playtime_dialog(app: &Rc<AppState>, router: &Router, port: Port) {
    let window = app.window();
    let tr = window.global::<Tr>();
    let name: slint::SharedString = port.name.clone().into();
    let key = port.key().to_string();
    open_confirm_dialog(
        app,
        router,
        tr.invoke_dialog_title_reset_playtime(name.clone()),
        tr.invoke_message_reset_playtime_confirm(name),
        tr.invoke_confirm_reset(),
        move |app, _| app.state.borrow_mut().reset_playtime(&key),
    );
}

fn existing_save_folder(save: Option<&Value>, game_dir: &Path) -> Option<PathBuf> {
    crate::core::platform_resolve::resolve_save_folder(save?, game_dir).filter(|p| p.is_dir())
}

fn wire_open<T: 'static>(target: Option<T>, open: fn(&T), register: impl FnOnce(Box<dyn Fn()>)) {
    if let Some(target) = target {
        register(Box::new(move || open(&target)));
    }
}

pub(crate) fn open_info_dialog(app: &Rc<AppState>, router: &Router, port: &Port) {
    close_current_dialog(app, router);
    app.dialog_nav.info_dialog_port_key.replace(Some(port.key().to_string()));
    let (dw, dh) = main_window_size(app);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = InfoDialog::new() else { return };
    style_dialog!(dialog, app, app.window_geometry.normal_mode.borrow().fonts);
    let tr = dialog.global::<Tr>();
    dialog.set_dialog_title(tr.invoke_dialog_title_info_port(port.name.clone().into()));

    let info = app.state.borrow().get(port.key()).cloned();
    let version_text = match &info {
        None => tr.invoke_version_not_installed(),
        Some(info) => match (port.source_type, &info.installed_tag) {
            (SourceType::Github | SourceType::Gitlab, Some(tag)) => tr.invoke_version_tag(tag.clone().into()),
            (SourceType::Github, None) => tr.invoke_version_installed_tag_unknown_github(),
            (SourceType::Gitlab, None) => tr.invoke_version_installed_tag_unknown_gitlab(),
            (SourceType::DirectUrl | SourceType::None, _) => tr.invoke_version_installed_no_tracking(),
        },
    };
    dialog.set_version_text(version_text);
    dialog.set_instructions_text(port.instructions.clone().into());
    if let Some(link) = port.instructions_link() {
        dialog.set_instructions_link(link.into());
        let link = link.to_string();
        dialog.on_instructions_link_requested(move || open_url(&link));
    }

    let game_dir = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder).ok();
    let installed_dir = game_dir.clone().filter(|p| p.is_dir());
    let installed = installed_dir.is_some();
    let website = port.website_url().filter(|u| is_web_url(u)).map(str::to_string);
    let mods = port.mods.clone().filter(|u| is_web_url(u));
    let save = game_dir.as_deref().and_then(|dir| existing_save_folder(port.save.as_ref(), dir));
    let save2 = game_dir.as_deref().and_then(|dir| existing_save_folder(port.save2.as_ref(), dir));
    let has_releases = matches!(port.source_type, SourceType::Github | SourceType::Gitlab) && port.repo.is_some();
    let has_files = installed_dir.as_deref().is_some_and(|dir| std::fs::read_dir(dir).is_ok_and(|mut it| it.next().is_some()));
    let playtime_seconds = info.as_ref().map_or(0, |i| i.playtime_seconds);

    dialog.set_change_version_enabled(has_releases);
    dialog.set_update_toggle_enabled(has_releases && installed);
    dialog.set_favorite_exe_enabled(has_files);
    dialog.set_extra_enabled(port.extra.is_some() && installed);
    dialog.set_reset_playtime_enabled(installed && playtime_seconds > 0);
    dialog.set_game_folder_enabled(installed);
    dialog.set_save_folder_enabled(save.is_some());
    dialog.set_save_folder2_enabled(save2.is_some());
    dialog.set_mods_enabled(mods.is_some());
    dialog.set_website_enabled(website.is_some());

    let favorite_exe = info.as_ref().and_then(|i| i.favorite_exe.clone());
    dialog.set_favorite_exe_status_text(match favorite_exe {
        None => tr.invoke_favorite_exe_status_default(),
        Some(exe) => tr.invoke_favorite_exe_status_named(exe.into()),
    });
    let update_on = info.as_ref().is_none_or(|i| i.update);
    dialog.set_update_status_text(if update_on { tr.invoke_update_status_on() } else { tr.invoke_update_status_off() });
    dialog.set_last_played_status_text(match info.as_ref().map_or(LastPlayed::Never, |i| format_last_played(&i.last_played_at)) {
        LastPlayed::Never => tr.invoke_last_played_status_never(),
        LastPlayed::Today => tr.invoke_last_played_status_today(),
        LastPlayed::Date(date) => tr.invoke_last_played_status(date.into()),
    });
    dialog.set_playtime_status_text(if playtime_seconds == 0 {
        tr.invoke_playtime_status_never()
    } else {
        tr.invoke_playtime_status(format_playtime(playtime_seconds).into())
    });

    let first_enabled = info_buttons_enabled(&dialog).iter().position(|&enabled| enabled).unwrap_or(0) as i32;
    app.dialog_nav.info_nav_index.set(first_enabled);
    dialog.set_selected_index(first_enabled);
    position_dialog!(dialog, app, dw, dh, x, y);

    wire_open(website, |url: &String| open_url(url), |f| dialog.on_website_requested(f));
    wire_open(mods, |url: &String| open_url(url), |f| dialog.on_mods_requested(f));
    wire_open(installed_dir, |dir: &PathBuf| open_folder(dir), |f| dialog.on_game_folder_requested(f));
    wire_open(save, |dir: &PathBuf| open_folder(dir), |f| dialog.on_save_folder_requested(f));
    wire_open(save2, |dir: &PathBuf| open_folder(dir), |f| dialog.on_save_folder2_requested(f));
    macro_rules! on_port_action {
        ($register:ident, $action:ident) => {{
            let (app, router, port) = (app.clone(), router.clone(), port.clone());
            dialog.$register(move || $action(&app, &router, port.clone()));
        }};
    }
    on_port_action!(on_change_version_requested, open_version_picker);
    on_port_action!(on_favorite_exe_requested, open_favorite_exe_picker);
    on_port_action!(on_update_toggle_requested, open_update_toggle_dialog);
    on_port_action!(on_reset_playtime_requested, open_reset_playtime_dialog);
    on_port_action!(on_extra_requested, open_extra_install_confirm_dialog);
    wire_dialog_close!(dialog, app, router);
    wire_nav_hovered!(dialog, app, info_nav_index);
    wire_keyboard_nav!(dialog, app, horizontal);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Info(dialog));
}

pub(crate) fn open_update_toggle_dialog(app: &Rc<AppState>, router: &Router, port: Port) {
    let window = app.window();
    let tr = window.global::<Tr>();
    let title = tr.invoke_dialog_title_toggle_update(port.name.clone().into());
    let labels = vec![tr.invoke_confirm_enable().to_string(), tr.invoke_confirm_disable().to_string()];
    let key = port.key().to_string();
    open_picker_dialog(app, router, &title, labels, move |app, _, idx| {
        app.state.borrow_mut().set_port_update(&key, idx == 0);
        app.refresh_current_view();
    });
}

fn set_picker_items<'a>(dialog: &ListPickerDialog, labels: impl IntoIterator<Item = &'a String>) {
    let items: Vec<PickerItem> = labels.into_iter().map(|label| PickerItem { label: label.into() }).collect();
    dialog.set_items(slint::ModelRc::new(slint::VecModel::from(items)));
}

fn build_picker_dialog(app: &Rc<AppState>, router: &Router, title: &str, labels: &[String]) -> Option<ListPickerDialog> {
    close_current_dialog(app, router);
    let big_mode = app.window().get_big_mode();
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let item_height = crate::ui::dialog_geometry::list_picker_item_height(work_h, big_mode);
    let (dw, dh) = crate::ui::dialog_geometry::list_picker_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, labels, big_mode, fonts.title_bar_height_px, app.window_geometry.border_width.get(),
    );
    let (x, y) = centered_position(app, dw, dh);
    let dialog = ListPickerDialog::new().ok()?;
    style_dialog!(dialog, app, fonts);
    dialog.set_dialog_title(title.into());
    set_picker_items(&dialog, labels);
    dialog.set_item_height_physical(item_height as f32);
    dialog.set_selected_index(0);
    app.dialog_nav.picker_index.set(0);
    position_dialog!(dialog, app, dw, dh, x, y);
    {
        let app = app.clone();
        let weak = dialog.as_weak();
        dialog.on_item_hovered(move |index| {
            app.dialog_nav.picker_index.set(index);
            if let Some(d) = weak.upgrade() {
                d.set_selected_index(index);
            }
        });
    }
    wire_keyboard_nav!(dialog, app, vertical);
    wire_dialog_close!(dialog, app, router);
    Some(dialog)
}

pub(crate) fn open_picker_dialog(
    app: &Rc<AppState>,
    router: &Router,
    title: &str,
    labels: Vec<String>,
    on_select: impl Fn(&Rc<AppState>, &Router, usize) + 'static,
) {
    let Some(dialog) = build_picker_dialog(app, router, title, &labels) else { return };
    {
        let (app, router) = (app.clone(), router.clone());
        dialog.on_item_selected(move |index| {
            close_current_dialog(&app, &router);
            on_select(&app, &router, index as usize);
        });
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Picker(dialog));
}

#[derive(Clone, Copy)]
enum SettingsEntry {
    Themes,
    Language,
    Files,
    Library,
    BackupSaves,
    CheckUpdates,
    ForceUpdate,
    DiscordRpc,
}

const SETTINGS_ENTRIES: [SettingsEntry; 8] = [
    SettingsEntry::Themes,
    SettingsEntry::Language,
    SettingsEntry::Files,
    SettingsEntry::Library,
    SettingsEntry::BackupSaves,
    SettingsEntry::CheckUpdates,
    SettingsEntry::ForceUpdate,
    SettingsEntry::DiscordRpc,
];

fn settings_labels(app: &AppState) -> Vec<String> {
    let window = app.window();
    let tr = window.global::<Tr>();
    let state = app.state.borrow();
    SETTINGS_ENTRIES
        .iter()
        .map(|entry| match entry {
            SettingsEntry::Themes => tr.invoke_label_themes(),
            SettingsEntry::Language => tr.invoke_label_language(),
            SettingsEntry::Files => tr.invoke_label_files(),
            SettingsEntry::Library => tr.invoke_label_library(),
            SettingsEntry::BackupSaves => tr.invoke_label_backup_saves(),
            SettingsEntry::CheckUpdates if state.release_sync => tr.invoke_label_check_updates_on(),
            SettingsEntry::CheckUpdates => tr.invoke_label_check_updates_off(),
            SettingsEntry::ForceUpdate => tr.invoke_label_force_update(),
            SettingsEntry::DiscordRpc if state.discord_rpc_enabled => tr.invoke_label_discord_rpc_on(),
            SettingsEntry::DiscordRpc => tr.invoke_label_discord_rpc_off(),
        })
        .map(|label| label.to_string())
        .collect()
}

pub(crate) fn open_settings_dialog(app: &Rc<AppState>, router: &Router) {
    if dialog_is_open(app) {
        return;
    }
    let title = tr!(app).invoke_dialog_title_settings();
    let Some(dialog) = build_picker_dialog(app, router, &title, &settings_labels(app)) else { return };
    {
        let (app, router) = (app.clone(), router.clone());
        let weak = dialog.as_weak();
        dialog.on_item_selected(move |index| {
            let Some(&entry) = SETTINGS_ENTRIES.get(index as usize) else { return };
            match entry {
                SettingsEntry::CheckUpdates => {
                    let value = !app.state.borrow().release_sync;
                    app.state.borrow_mut().set_release_sync(value);
                }
                SettingsEntry::DiscordRpc => {
                    let value = !app.state.borrow().discord_rpc_enabled;
                    app.state.borrow_mut().set_discord_rpc_enabled(value);
                }
                _ => {
                    close_current_dialog(&app, &router);
                    match entry {
                        SettingsEntry::Themes => open_theme_picker(&app, &router),
                        SettingsEntry::Language => open_language_picker(&app, &router),
                        SettingsEntry::Files => open_files_picker(&app, &router),
                        SettingsEntry::Library => open_folder(&app.paths.library_dir),
                        SettingsEntry::BackupSaves => start_save_backup(&app, &router),
                        _ => launch_self_update(&app, &router),
                    }
                    return;
                }
            }
            if let Some(d) = weak.upgrade() {
                set_picker_items(&d, &settings_labels(&app));
            }
        });
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Picker(dialog));
}

type HoverFn = Box<dyn Fn(&Rc<AppState>, &SearchListDialog, &str)>;

struct LivePreview {
    on_hover: HoverFn,
    on_cancel: fn(&Rc<AppState>),
}

fn open_search_list_dialog(
    app: &Rc<AppState>,
    router: &Router,
    title: slint::SharedString,
    items: Vec<(String, String)>,
    start_value: &str,
    preview: Option<LivePreview>,
    on_select: impl Fn(&Rc<AppState>, &str) + 'static,
) {
    close_current_dialog(app, router);
    let (fonts, ..) = dialog_context(app);
    let (dw, dh) = main_window_size(app);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = SearchListDialog::new() else { return };
    style_dialog!(dialog, app, fonts);
    dialog.set_dialog_title(title);
    dialog.set_search_bar_height_physical(fonts.search_bar_height_px as f32);
    let set_items = |d: &SearchListDialog, items: &[(String, String)]| {
        let model: Vec<PickerItem> = items.iter().map(|(_, label)| PickerItem { label: label.into() }).collect();
        d.set_items(slint::ModelRc::new(slint::VecModel::from(model)));
    };
    set_items(&dialog, &items);
    if preview.is_some() {
        let state = app.state.borrow();
        dialog.set_placeholder_text(state.placeholder_text.clone().into());
        dialog.set_show_clock(state.show_clock);
        if state.show_clock {
            dialog.set_clock_text(crate::core::clock::format_now().into());
        }
    }
    let start_index = items.iter().position(|(value, _)| value == start_value).unwrap_or(0) as i32;
    dialog.set_selected_index(start_index);
    app.dialog_nav.picker_index.set(start_index);
    position_dialog!(dialog, app, dw, dh, x, y);

    let displayed = Rc::new(RefCell::new(items.clone()));
    let preview = Rc::new(preview);
    {
        let (app, displayed, preview, weak) = (app.clone(), displayed.clone(), preview.clone(), dialog.as_weak());
        dialog.on_item_hovered(move |index| {
            app.dialog_nav.picker_index.set(index);
            let Some(d) = weak.upgrade() else { return };
            d.set_selected_index(index);
            if let (Some(preview), Some((value, _))) = (preview.as_ref(), displayed.borrow().get(index as usize)) {
                (preview.on_hover)(&app, &d, value);
            }
        });
    }
    {
        let (app, router, displayed) = (app.clone(), router.clone(), displayed.clone());
        dialog.on_item_selected(move |index| {
            let value = displayed.borrow().get(index as usize).map(|(v, _)| v.clone());
            close_current_dialog(&app, &router);
            if let Some(value) = value {
                on_select(&app, &value);
            }
        });
    }
    {
        let (app, displayed, has_preview, weak) = (app.clone(), displayed.clone(), preview.is_some(), dialog.as_weak());
        dialog.on_search_changed(move |query| {
            let query = query.to_lowercase();
            let filtered: Vec<(String, String)> = items.iter().filter(|(_, label)| label.to_lowercase().contains(&query)).cloned().collect();
            let Some(d) = weak.upgrade() else { return };
            set_items(&d, &filtered);
            let next = if filtered.is_empty() { -1 } else { 0 };
            let has_results = !filtered.is_empty();
            *displayed.borrow_mut() = filtered;
            d.set_selected_index(next);
            app.dialog_nav.picker_index.set(next);
            if has_preview && has_results {
                d.invoke_item_hovered(0);
            }
        });
    }
    wire_keyboard_nav!(dialog, app, vertical);
    {
        let (app, router) = (app.clone(), router.clone());
        dialog.on_close_requested(move || {
            if let Some(preview) = preview.as_ref() {
                (preview.on_cancel)(&app);
            }
            close_current_dialog(&app, &router);
        });
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::SearchList(dialog));
}

fn open_theme_picker(app: &Rc<AppState>, router: &Router) {
    let names = crate::ui::theme::list_theme_names(&app.theme.theme_config.borrow());
    if names.is_empty() {
        return;
    }
    let items = names.into_iter().map(|n| (n.clone(), n)).collect();
    let active = app.state.borrow().active_theme.clone();
    let preview = LivePreview {
        on_hover: Box::new(|app, dialog, name| {
            crate::ui::theme::preview_theme(&mut app.theme.theme_config.borrow_mut(), name);
            apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
            apply_dialog_theme!(dialog, app);
        }),
        on_cancel: revert_theme_preview,
    };
    open_search_list_dialog(app, router, tr!(app).invoke_label_themes(), items, &active, Some(preview), |app, name| {
        app.state.borrow_mut().set_active_theme(name.to_string());
    });
}

fn open_files_picker(app: &Rc<AppState>, router: &Router) {
    const FILES: [&str; 4] = ["ports.json", "ports.local.json", "state.json", "themes.json"];
    let labels = FILES.iter().map(|name| name.to_string()).collect();
    open_picker_dialog(app, router, &tr!(app).invoke_label_files(), labels, |app, _, idx| {
        if let Some(name) = FILES.get(idx) {
            open_file(&app.paths.config_dir.join(name));
        }
    });
}

const LANGUAGES: &[(&str, &str)] = &[
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

fn open_language_picker(app: &Rc<AppState>, router: &Router) {
    let items = std::iter::once((String::new(), tr!(app).invoke_language_default_system().to_string()))
        .chain(LANGUAGES.iter().map(|(code, name)| (code.to_string(), name.to_string())))
        .collect();
    let current = app.state.borrow().language.clone();
    open_search_list_dialog(app, router, tr!(app).invoke_dialog_title_language(), items, &current, None, |app, code| {
        let _ = slint::select_bundled_translation(code);
        app.state.borrow_mut().set_language(code.to_string());
    });
}

fn start_save_backup(app: &Rc<AppState>, router: &Router) {
    let window = app.window();
    let tr = window.global::<Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_saves_backup(), &tr.invoke_progress_backing_up_saves());

    let catalog = app.catalog.borrow().clone();
    let library_dir = app.paths.library_dir.clone();
    let saves_backup_dir = app.paths.saves_backup_dir.clone();
    let events = app.events.clone();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    std::thread::spawn(move || {
        let mut on_progress = |name: &str| lock(&events).push(AppEvent::SaveBackupProgress { name: name.to_string() });
        let summary = crate::core::save_backup::run_global_backup(&catalog, &library_dir, &saves_backup_dir, &date, &mut on_progress);
        lock(&events).push(AppEvent::SaveBackupDone { copied: summary.copied, skipped: summary.skipped, failed: summary.failed });
    });
}
