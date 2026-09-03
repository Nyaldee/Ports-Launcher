//! Infrastructure de dialogue (thème/chrome/position/fermeture, partagés par
//! les 7 types de dialogue Slint) et tous les `open_*_dialog` qui les
//! construisent.

use super::events::{lock, AppEvent};
use super::gamepad_target::DialogGamepadTarget;
use super::install_launch::{
    delete_port, open_favorite_exe_picker, open_path_if_exists, open_version_picker, start_extra_install, start_install,
};
use super::playtime::format_playtime;
use super::state::AppState;
use crate::core::models::{Port, SourceType};
use crate::ui::font_sizing::FontSizes;
use crate::ui::gamepad_router::{GamepadRouter, GamepadTarget};
use crate::ui::windows_chrome;
use crate::{
    AppWindow, ConfirmDialog, ErrorDialog, InfoDialog, ListPickerDialog, MessageDialog, PickerItem, ProgressDialog, SearchListDialog, SemanticColors, Theme,
    Tr,
};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

/// Pousse les 7 couleurs éditables du thème + l'épaisseur de bordure + le
/// facteur d'échelle DPI sur N'IMPORTE QUELLE fenêtre Slint qui importe le
/// global `Theme` (voir shared.slint) -- AppWindow ET chaque dialogue,
/// qui sont des composants Window SÉPARÉS avec chacun sa PROPRE instance de
/// ce global (les globals Slint sont attachés à la fenêtre qui les héberge,
/// jamais partagés entre fenêtres même si le .slint est le même fichier
/// importé partout).
macro_rules! apply_dialog_theme {
    ($dialog:expr, $app:expr) => {{
        // Sans ce push, chaque dialogue retombe sur la police par défaut de
        // Slint quel que soit "font_family" dans themes.json.
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
        // Croix de fermeture (DialogTitleBar dans dialogs.slint, partagée
        // par tous les dialogues) -- sans ce push, elle retombe sur les
        // valeurs de repli codées en dur dans card-grid.slint.
        let gc = $dialog.global::<SemanticColors>();
        gc.set_danger($app.theme.semantic.danger);
        gc.set_text_on_accent($app.theme.semantic.text_on_accent);
    }};
}

/// Pousse la "chrome" (police/rangée/barre de titre, en pixels PHYSIQUES) à
/// n'importe quel dialogue -- un seul point d'appel pour les 4 champs (voir
/// dialogs.slint, tous les types de dialogue partagent la même forme), pour
/// qu'il soit structurellement impossible d'en oublier un.
///
/// `$fonts` reste un paramètre explicite : InfoDialog utilise TOUJOURS
/// normal_mode.fonts (dialogue carré compact -- la police fullscreen,
/// dimensionnée pour tout l'écran, le ferait déborder), alors que
/// Message/Error/Progress/Picker suivent le mode actuellement affiché.
macro_rules! apply_dialog_chrome {
    ($dialog:expr, $fonts:expr) => {{
        $dialog.set_item_font_px_physical($fonts.item_font_px as f32);
        $dialog.set_title_font_px_physical($fonts.title_font_px as f32);
        $dialog.set_row_height_physical($fonts.row_height_px as f32);
        $dialog.set_title_bar_height_physical($fonts.title_bar_height_px as f32);
    }};
}

/// `$w`/`$h` viennent de dialog_geometry en pixels PHYSIQUES -- `$scale` les
/// convertit en LOGIQUES avant de les pousser : `initial-width`/
/// `initial-height` sont des `<length>` Slint, donc logiques, et Slint les
/// remettrait à l'échelle une seconde fois s'ils arrivaient en physique.
/// `$x`/`$y` restent physiques : `WindowPosition::Physical` les prend tels
/// quels.
macro_rules! position_dialog {
    ($dialog:expr, $w:expr, $h:expr, $x:expr, $y:expr, $scale:expr) => {{
        $dialog.set_initial_width($w as f32 / $scale);
        $dialog.set_initial_height($h as f32 / $scale);
        $dialog.window().set_position(slint::WindowPosition::Physical(slint::PhysicalPosition { x: $x, y: $y }));
    }};
}

/// Raccourci pour `$app.window().global::<Tr>()` (voir Tr dans
/// dialogs/tr.slint) -- pour un site qui n'a besoin que d'UN appel
/// (`tr!(app).invoke_x()`). Pour en enchaîner plusieurs sur le même handle,
/// nommer `window`/`tr` à la main reste nécessaire (`let window = app.window();
/// let tr = window.global::<Tr>();`) : une macro ne peut pas introduire de
/// bindings visibles APRÈS son propre appel (hygiène des macros Rust).
macro_rules! tr {
    ($app:expr) => {
        $app.window().global::<Tr>()
    };
}
pub(crate) use tr;

/// Ferme le dialogue courant quand sa croix de fermeture est cliquée --
/// pour Message/Error/Info/Picker (Progress n'a pas de croix, jamais
/// annulable en cours d'install). Une macro et non une fonction générique :
/// chaque type de dialogue Slint généré est distinct, sans trait commun.
macro_rules! wire_dialog_close {
    ($dialog:expr, $app:expr, $router:expr) => {{
        let app2 = $app.clone();
        let router2 = $router.clone();
        $dialog.on_close_requested(move || close_current_dialog(&app2, &router2));
    }};
}

/// Branche move-selection-requested/activate-selection-requested (clavier)
/// vers DialogGamepadTarget::move_selection/activate_selection -- même
/// logique que la manette, jamais une seconde implémentation (voir
/// dialogs.slint pour le détail de ces deux callbacks). `$axis`:
/// `horizontal` pour une rangée de boutons (Info), `vertical` pour des
/// boutons empilés (Settings/Picker/Confirm).
/// Fait suivre la sélection manette/clavier sous la souris -- un seul
/// indicateur pour les deux (voir nav-hovered dans dialogs.slint). `$field`
/// est le `Cell<i32>` de `AppState::dialog_nav` (`$group`, toujours
/// `dialog_nav`) qui arbitre la sélection pour ce dialogue (`error_nav_index`,
/// `confirm_nav_index`, `info_nav_index`...).
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

/// Police du mode actuellement affiché + famille de police + zone de
/// travail sous le curseur -- contexte commun en tête de chaque
/// `open_*_dialog`.
pub(crate) fn dialog_context(app: &Rc<AppState>) -> (FontSizes, slint::SharedString, i32, i32) {
    let big_mode = app.window().get_big_mode();
    let fonts = if big_mode { app.window_geometry.fullscreen_mode.borrow().fonts } else { app.window_geometry.normal_mode.borrow().fonts };
    let family = app.window().get_font_family();
    let (_, _, work_w, work_h) = windows_chrome::work_area_under_cursor();
    (fonts, family, work_w, work_h)
}

/// `border_width` vit dans `state.json` désormais (voir
/// `core::state::StateManager`), séparé de `theme: &ThemeConfig` (les
/// couleurs, toujours dans `themes.json`) -- passé explicitement plutôt que
/// relu depuis `app.window_geometry.border_width` pour que cette fonction reste appelable
/// au tout premier démarrage, avant qu'`AppState` n'existe.
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

    // Couleurs de la grille plein écran -- dérivées du thème (voir
    // SemanticColors, pas éditables séparément dans themes.json) et poussées
    // ici plutôt que codées en dur dans le .slint.
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

/// Annule l'effet visuel d'une prévisualisation de thème non confirmée dans
/// le sélecteur de thème (voir `ui::theme::preview_theme`) -- réapplique
/// juste le thème RÉELLEMENT actif (`active_theme` n'a jamais bougé pendant
/// un survol de la liste, voir son commentaire dans `AppState`).
pub(crate) fn revert_theme_preview(app: &Rc<AppState>) {
    let name = app.state.borrow().active_theme.clone();
    crate::ui::theme::preview_theme(&mut app.theme.theme_config.borrow_mut(), &name);
    apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
}

/// Un seul dialogue ouvert à la fois -- jamais deux dialogues modaux
/// affichés en même temps. Possède directement la fenêtre Slint (elle
/// serait fermée/détruite si on ne la gardait pas en vie ici).
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

pub(crate) fn centered_position(app: &AppState, dialog_w: i32, dialog_h: i32) -> (i32, i32) {
    let window = app.window();
    let pos = window.window().position();
    let size = window.window().size();
    crate::ui::dialog_geometry::center_over_parent(pos.x, pos.y, size.width as i32, size.height as i32, dialog_w, dialog_h)
}

/// Referme le dialogue affiché (s'il y en a un) et dépile sa cible manette
/// -- unique point de sortie, que ce soit un clic sur ×, la touche B, ou
/// l'ouverture d'un NOUVEAU dialogue (chaque `open_*_dialog` appelle ceci en
/// premier : jamais deux dialogues modaux à la fois).
pub(crate) fn close_current_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    // Toujours effacé ici, même si le dialogue qui se ferme n'est pas Info --
    // seul open_info_dialog le repose, donc rien ne le referait sinon.
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
        // Symétrique du EnableWindow(..., false) posé à l'ouverture de
        // chaque dialogue.
        if let Some(hwnd) = windows_chrome::native_hwnd(app.window().window()) {
            windows_chrome::set_window_enabled(hwnd, true);
        }
        // Entre le `d.hide()` ci-dessus et ce réactivation, la fenêtre
        // principale reste un court instant désactivée -- donc incapable de
        // redevenir active -- alors que le dialogue a déjà disparu : Windows
        // peut laisser n'importe quelle autre fenêtre du bureau au premier
        // plan pendant cet intervalle, et la réactiver après coup ne la lui
        // rend pas. Centralisé ici pour couvrir tous les déclencheurs (clic,
        // Échap, manette, évènement d'arrière-plan).
        if let Some(hwnd) = windows_chrome::native_hwnd(app.window().window()) {
            windows_chrome::force_foreground_window(hwnd);
        }
        router.borrow_mut().pop_target();
        // Rend le focus à la barre de recherche. Une bascule, pas un
        // `set_refocus_trigger(true)` : la propriété doit CHANGER de valeur
        // pour déclencher `changed refocus-trigger` côté .slint, sinon rien
        // ne se produirait au dialogue suivant.
        let window = app.window();
        window.set_refocus_trigger(!window.get_refocus_trigger());
    }
}

pub(crate) fn push_dialog_target(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    router.borrow_mut().push_target(Rc::new(DialogGamepadTarget { app: app.clone() }));
}

/// Rattrape les fermetures qui ne passent pas par nos propres callbacks :
/// Alt+F4 ou "Fermer la fenêtre" depuis la barre des tâches envoient un
/// WM_CLOSE natif que Slint traite seul (il masque la fenêtre) sans jamais
/// invoquer notre callback `close-requested`. Sans ce hook, `app.dialog_nav.dialogs`
/// resterait bloqué sur ce dialogue et le `set_window_enabled(hwnd, false)`
/// de finish_dialog_open ne serait jamais annulé -- la fenêtre
/// principale resterait désactivée pour de bon. `Window::on_close_requested`
/// est le seul hook qui capte aussi ces fermetures natives.
pub(crate) fn wire_close_requested_cleanup(window: &slint::Window, app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let app = app.clone();
    let router = router.clone();
    window.on_close_requested(move || {
        close_current_dialog(&app, &router);
        slint::CloseRequestResponse::HideWindow
    });
}

/// Fenêtre Slint native du dialogue dans `slot`, ou `None` si aucun.
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

/// Séquence commune à la fin de chaque `open_*_dialog`, une fois le
/// `dialog.show()` fait par l'appelant : rend la fenêtre principale
/// véritablement modale (voir windows_chrome::set_window_enabled, réactivée dans
/// close_current_dialog), enregistre le dialogue comme actif, pousse sa
/// cible manette, puis lui applique icône/possession/premier plan.
pub(crate) fn finish_dialog_open(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, slot: DialogSlot) {
    if let Some(w) = dialog_window(&slot) {
        wire_close_requested_cleanup(w, app, router);
    }
    if let Some(hwnd) = windows_chrome::native_hwnd(app.window().window()) {
        windows_chrome::set_window_enabled(hwnd, false);
    }
    *app.dialog_nav.dialogs.borrow_mut() = slot;
    push_dialog_target(app, router);

    // DIFFÉRÉ : `windows_chrome::native_hwnd()` renvoie None juste après l'ouverture du
    // dialogue, la fenêtre native n'étant pas encore complètement associée
    // (même symptôme que la fenêtre principale au premier show(), voir
    // main()). Le Timer relit `app.dialog_nav.dialogs` au moment où il se déclenche
    // plutôt que de capturer un hwnd indisponible -- si le dialogue a été
    // refermé entre-temps, dialog_window renvoie None et il n'y a rien à
    // faire.
    let app2 = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        let slot_ref = app2.dialog_nav.dialogs.borrow();
        let Some(hwnd) = dialog_window(&slot_ref).and_then(windows_chrome::native_hwnd) else { return };
        drop(slot_ref);
        windows_chrome::apply_window_icon(hwnd);
        // Fenêtre POSSÉDÉE par la principale, pas seulement désactivée :
        // EnableWindow(FALSE) bloque l'entrée clavier/souris mais laisse la
        // fenêtre principale sélectionnable via Alt+Tab, qui la ramènerait
        // au premier plan tout en la laissant désactivée -- un état
        // incohérent qui plante l'appli. own_window (relation de
        // possession, pas un vrai parentage) fait flasher la fenêtre
        // possédée à la place. Le dialogue reste néanmoins listé dans
        // Alt+Tab, d'où l'icône/le titre poussés juste au-dessus.
        if let Some(main_hwnd) = windows_chrome::native_hwnd(app2.window().window()) {
            windows_chrome::own_window(hwnd, main_hwnd);
        }
        // `dialog.show()` seul ne suffit pas : si le dialogue n'est pas
        // encore devenu la fenêtre active quand la principale se désactive,
        // Windows peut donner le premier plan à n'importe quelle autre
        // fenêtre du bureau (typiquement l'Explorateur d'où l'exe a été
        // lancé, qui repasse alors devant le dialogue de progression).
        windows_chrome::force_foreground_window(hwnd);
    });
}

/// Défense en profondeur : la modalité OS (`EnableWindow`, voir
/// windows_chrome::set_window_enabled) devrait déjà empêcher tout callback de la
/// fenêtre principale d'être atteint pendant qu'un dialogue est ouvert.
/// Sans cette garde, cliquer plusieurs lignes pendant qu'un dialogue est
/// affiché lance des installs concurrents qui se partagent le même
/// `DialogSlot` et écrasent chacun le dialogue de l'autre.
pub(crate) fn dialog_is_open(app: &AppState) -> bool {
    !matches!(*app.dialog_nav.dialogs.borrow(), DialogSlot::None)
}

pub(crate) fn open_message_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, title: &str, message: &str) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    // Hauteur dérivée de la mesure RÉELLE de `message` (voir
    // ui::dialog_geometry::message_dialog_size) -- jamais de texte tronqué.
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

/// Recalcule taille et position de `dialog` pour le `status` donné -- à
/// l'ouverture comme à CHAQUE mise à jour de texte en cours d'install (voir
/// AppEvent::InstallProgress). Un `set_status_text` seul laisserait la
/// fenêtre à sa taille initiale et tronquerait un message plus long
/// arrivant ensuite, la mesure de progress_dialog_size n'étant faite qu'ici.
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
    // Hauteur dérivée de la mesure RÉELLE du message, qui contient le nom
    // du port et peut donc être long.
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
    // Repart toujours sur "Reinstall" en surbrillance, jamais la position
    // laissée par un ErrorDialog précédent.
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

/// Dialogue de confirmation générique -- `title`/`message` déjà traduits par
/// l'appelant, `confirm_label` remplace le libellé par défaut du bouton
/// ("Confirm") quand `Some`, `on_confirmed` s'exécute APRÈS la fermeture du
/// dialogue (comme `open_picker_dialog`). `error_dialog_size` sert de mesure
/// pour tout dialogue "message + deux boutons empilés", rien
/// d'error-spécifique dans son calcul.
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
    // Repart toujours sur "Confirm" en surbrillance, jamais la position
    // laissée par un ConfirmDialog précédent.
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

/// Demande confirmation avant `delete_port`.
pub(crate) fn open_uninstall_confirm_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let title = tr!(app).invoke_dialog_title_uninstall_port(port.name.clone().into());
    let message = tr!(app).invoke_message_uninstall_confirm(port.name.clone().into());
    let port2 = port.clone();
    open_confirm_dialog(app, router, title, message, None, move |app, router| delete_port(app, router, &port2));
}

pub(crate) fn open_info_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    close_current_dialog(app, router);
    // Après close_current_dialog, qui vient de l'effacer -- voir son
    // commentaire de champ pour pourquoi (rafraîchi en direct par
    // poll_app_events pendant qu'une partie tourne).
    app.dialog_nav.info_dialog_port_key.replace(Some(port.key().to_string()));
    // Même taille exacte que la fenêtre principale, lue directement sur la
    // fenêtre RÉELLEMENT affichée : un second calcul pourrait diverger de la
    // géométrie déjà appliquée à app-window selon le moniteur sous le
    // curseur.
    let main_size = app.window().window().size();
    let (dw, dh) = (main_size.width as i32, main_size.height as i32);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = InfoDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(dialog.global::<Tr>().invoke_dialog_title_info_port(port.name.clone().into()));
    // TOUJOURS normal_mode.fonts, jamais fullscreen_mode -- voir
    // apply_dialog_chrome!.
    apply_dialog_chrome!(dialog, app.window_geometry.normal_mode.borrow().fonts);

    // Messages distincts pour deux situations distinctes : une source
    // Local/DirectUrl n'a aucune notion de tag et n'est jamais vérifiée,
    // alors qu'un port GitHub/GitLab au tag inconnu (state.json reconstruit,
    // ou port déposé à la main dans Library/ puis adopté par main()) reste
    // bel et bien vérifié -- launch_with_update_check n'exige pas de tag
    // connu, update_decision se replie sur installed_at.
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

    // `resolve_save`, pas `expand_env_path` seule : un save
    // relatif (ex: "Save", sans %VARIABLE%) doit se résoudre par rapport au
    // dossier du JEU, jamais au dossier courant du processus.
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

    // GitHub/GitLab uniquement -- une source DirectUrl/Local n'a aucun
    // historique de releases à proposer (voir open_version_picker).
    let change_version_ok = matches!(port.source_type, SourceType::Github | SourceType::Gitlab) && port.repo.is_some();
    dialog.set_change_version_enabled(change_version_ok);

    // Rien à scanner comme exécutable candidat si le port n'est pas
    // installé (voir open_favorite_exe_picker).
    let favorite_exe_ok = game_folder_ok;
    dialog.set_favorite_exe_enabled(favorite_exe_ok);

    // change_version_ok ET installé -- contrairement à Select version
    // (utilisable AVANT toute install, pour choisir quelle release
    // installer en premier), basculer l'auto-MAJ n'a de sens que pour un
    // port déjà installé : sinon `set_port_update` crée quand même une
    // entrée dans state.json (avec un `installed_at` qui n'est pas une
    // vraie date d'install) juste pour retenir la préférence, gelée là sans
    // jamais être consultée.
    let update_toggle_ok = change_version_ok && game_folder_ok;
    dialog.set_update_toggle_enabled(update_toggle_ok);

    // Même condition que favorite_exe_ok -- rien à réinitialiser pour un
    // port qui n'est pas installé.
    let reset_playtime_ok = game_folder_ok;
    dialog.set_reset_playtime_enabled(reset_playtime_ok);

    // Bouton "Install extras" -- actif seulement si le port déclare un champ
    // "extra" dans ports.json ET est installé (aucun dossier où fusionner
    // les fichiers sinon, voir installer::install_extra_only).
    let extra_ok = port.extra.is_some() && game_folder_ok;
    dialog.set_extra_enabled(extra_ok);

    let favorite_exe_status = match app.state.borrow().get(port.key()).and_then(|i| i.favorite_exe.clone()) {
        None => tr.invoke_favorite_exe_status_default(),
        Some(exe) => tr.invoke_favorite_exe_status_named(exe.into()),
    };
    dialog.set_favorite_exe_status_text(favorite_exe_status);
    let update_on = app.state.borrow().get(port.key()).map(|i| i.update).unwrap_or(true);
    dialog.set_update_status_text(if update_on { tr.invoke_update_status_on() } else { tr.invoke_update_status_off() });
    let playtime_seconds = app.state.borrow().get(port.key()).map(|i| i.playtime_seconds).unwrap_or(0);
    dialog.set_playtime_status_text(tr.invoke_playtime_status(format_playtime(playtime_seconds).into()));

    // Premier bouton ACTIVÉ -- 0 si aucun ne l'est, auquel cas
    // activate_selection reste de toute façon un no-op. Même ordre que
    // InfoDialog.selected-index (voir dialogs/info.slint).
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

/// Bouton "Update" d'InfoDialog OU badge barré de la ligne principale (voir
/// app-window.slint::update-btn) -- MÊME dialogue dans les deux cas. Picker
/// générique à deux entrées fixes (voir open_picker_dialog) plutôt qu'un
/// ConfirmDialog dont le message ET le texte du bouton auraient dû changer
/// selon l'état courant -- ici rien ne dépend de `currently_on` sauf
/// l'action déclenchée par le choix. Pas de "Cancel" : le × du picker sert
/// déjà à annuler.
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

/// Bouton "Install extras" d'InfoDialog -- confirme avant de télécharger et
/// fusionner les fichiers `extra` de `port` dans son dossier (voir
/// `app::install_launch::start_extra_install`). Le message prévient que ces
/// ajouts (options de lancement, configurations prédéfinies -- souvent des
/// choix arbitraires) écrasent les copies déjà présentes dans le dossier.
pub(crate) fn open_extra_install_confirm_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let title = tr!(app).invoke_dialog_title_install_extras(port.name.clone().into());
    let message = tr!(app).invoke_message_install_extras_confirm(port.name.clone().into());
    let confirm_label = tr!(app).invoke_confirm_install();
    open_confirm_dialog(app, router, title, message, Some(confirm_label), move |app, router| {
        start_extra_install(app, router, port.clone());
    });
}

/// Bouton "Reset Game Time" d'InfoDialog -- remet à zéro le temps de jeu
/// cumulé de `port` (voir StateManager::reset_playtime), après confirmation.
pub(crate) fn open_reset_playtime_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let title = tr!(app).invoke_dialog_title_reset_playtime(port.name.clone().into());
    let message = tr!(app).invoke_message_reset_playtime_confirm(port.name.clone().into());
    let confirm_label = tr!(app).invoke_confirm_reset();
    let key = port.key().to_string();
    open_confirm_dialog(app, router, title, message, Some(confirm_label), move |app, _router| {
        app.state.borrow_mut().reset_playtime(&key);
    });
}

/// Dialogue de choix générique -- choix d'asset (install ambigu), de version
/// et d'exécutable (lancement ambigu) ; seuls les libellés et l'action
/// `on_select` changent. `on_select` est appelé APRÈS la fermeture du
/// dialogue, et peut donc rouvrir un autre dialogue (start_install,
/// launch_executable...) sans superposer deux modales.
pub(crate) fn open_picker_dialog(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    title: &str,
    labels: Vec<String>,
    on_select: impl Fn(&Rc<AppState>, &Rc<RefCell<GamepadRouter>>, usize) + 'static,
) {
    close_current_dialog(app, router);
    let big_mode = app.window().get_big_mode();
    // Mêmes tailles de police/barre de titre que la fenêtre principale et
    // InfoDialog, selon le mode actuellement affiché.
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let item_height = crate::ui::dialog_geometry::list_picker_item_height(work_h, big_mode);
    // Largeur élargie si besoin pour que le libellé le plus long tienne en
    // entier (mesure réelle) -- jamais d'ellipse.
    let (dw, dh) = crate::ui::dialog_geometry::list_picker_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, &labels, big_mode, fonts.title_bar_height_px, app.window_geometry.border_width.get(),
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ListPickerDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title.into());
    let items: Vec<PickerItem> = labels.into_iter().map(|label| PickerItem { label: label.into() }).collect();
    dialog.set_items(slint::ModelRc::new(slint::VecModel::from(items)));
    dialog.set_item_height_physical(item_height as f32);
    apply_dialog_chrome!(dialog, fonts);
    dialog.set_selected_index(0);
    app.dialog_nav.picker_index.set(0);
    position_dialog!(dialog, dw, dh, x, y, app.window_geometry.scale.get());
    {
        let app2 = app.clone();
        let router2 = router.clone();
        dialog.on_item_selected(move |index| {
            close_current_dialog(&app2, &router2);
            on_select(&app2, &router2, index as usize);
        });
    }
    {
        let app2 = app.clone();
        let dialog_weak = dialog.as_weak();
        // Comme InfoDialog.on_nav_hovered -- un seul indicateur de sélection
        // pour la souris et la manette/clavier.
        dialog.on_item_hovered(move |index| {
            app2.dialog_nav.picker_index.set(index);
            if let Some(d) = dialog_weak.upgrade() {
                d.set_selected_index(index);
            }
        });
    }
    wire_dialog_selection_nav!(dialog, app, vertical);
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Picker(dialog));
}

/// menu Settings -- 5 boutons (Themes/Language/Files/Library/Backup Saves),
/// réutilise le picker générique (même mécanisme que "Select version",
/// voir open_picker_dialog) plutôt qu'un composant dédié. Chaque sous-écran
/// (thèmes, langue, fichiers) vit dans sa propre fonction ci-dessous.
pub(crate) fn open_settings_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let window = app.window();
    let tr = window.global::<Tr>();
    let check_updates_label =
        if app.state.borrow().release_sync { tr.invoke_label_check_updates_on() } else { tr.invoke_label_check_updates_off() };
    let labels = vec![
        tr.invoke_label_themes().to_string(),
        tr.invoke_label_language().to_string(),
        tr.invoke_label_files().to_string(),
        tr.invoke_label_library().to_string(),
        tr.invoke_label_backup_saves().to_string(),
        check_updates_label.to_string(),
    ];
    let title = tr.invoke_dialog_title_settings();
    open_picker_dialog(app, router, &title, labels, move |app, router, idx| match idx {
        0 => open_theme_picker(app, router),
        1 => open_language_picker(app, router),
        2 => open_files_picker(app, router),
        3 => open_path_if_exists(&app.paths.library_dir),
        4 => start_save_backup(app, router),
        _ => toggle_release_sync(app, router),
    });
}

/// Bouton "Check for Updates: On/Off" du menu Settings -- interrupteur
/// GLOBAL (voir `StateManager::release_sync`), coupe/rétablit à la fois le
/// check self-update du launcher ET la vérification par port au Play (voir
/// `launch_with_update_check`). Rouvre le menu après le clic, comme pour
/// tout item d'un ListPickerDialog (voir son commentaire) -- le libellé
/// reflète alors immédiatement le nouvel état.
pub(crate) fn toggle_release_sync(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let new_value = !app.state.borrow().release_sync;
    app.state.borrow_mut().set_release_sync(new_value);
    open_settings_dialog(app, router);
}

type HoverFn = Box<dyn Fn(&Rc<AppState>, &SearchListDialog, &str) + 'static>;
type CloseWithoutSelectFn = Box<dyn Fn(&Rc<AppState>) + 'static>;

/// Base commune de `open_theme_picker`/`open_language_picker` -- une liste
/// recherchable (voir SearchListDialog dans dialogs.slint) de paires
/// (valeur, libellé affiché). `start_value` positionne la sélection
/// initiale. `on_hover` est `None` pour un écran sans prévisualisation en
/// direct (langue) ; quand `Some`, il pilote aussi deux comportements liés à
/// la prévisualisation : le placeholder/horloge de la barre de recherche
/// mimant la fenêtre principale (thème seul en a besoin, la langue n'a rien
/// à prévisualiser derrière), et l'aperçu automatique du premier résultat
/// après une recherche (sinon l'aperçu resterait sur l'ancienne valeur
/// jusqu'au prochain déplacement). `on_close_without_select` annule l'effet
/// d'une prévisualisation si le dialogue se ferme sans validation.
///
/// Les 8 paramètres n'ont aucun regroupement honnête (3 pour le contenu de
/// la liste, 3 closures indépendantes) -- un struct de bundling serait
/// artificiel, d'où l'`allow` plutôt qu'un `InstallPaths`-like ici (même
/// choix que `installer::download_release_asset`).
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
    // Même taille exacte que la fenêtre principale, lue sur la fenêtre
    // réellement affichée -- même principe qu'InfoDialog.
    let main_size = app.window().window().size();
    let (dw, dh) = (main_size.width as i32, main_size.height as i32);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = SearchListDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title);
    let picker_items: Vec<PickerItem> = items.iter().map(|(_, label)| PickerItem { label: label.clone().into() }).collect();
    dialog.set_items(slint::ModelRc::new(slint::VecModel::from(picker_items)));
    apply_dialog_chrome!(dialog, fonts);
    // Pas via apply_dialog_chrome! (partagée par des dialogues sans barre
    // de recherche) -- même valeur EXACTE que search-bar-height-physical
    // sur la fenêtre principale (voir apply_font_sizes), fenêtré et plein
    // écran ayant chacun leur propre formule (voir FontSizes).
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

    // Items RÉELLEMENT affichés, distincts de `items` (la liste complète,
    // gardée intacte pour la recherche) : une fois filtrés par
    // `on_search_changed`, item-hovered/item-selected doivent indexer CETTE
    // liste, sinon les clics et l'aperçu pointeraient la mauvaise entrée.
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

/// Sélecteur de thème -- navigation (survol souris ou flèches/manette) ->
/// `preview_theme` applique les couleurs EN DIRECT (jamais `active_theme` ni
/// le disque, voir son commentaire) à la fenêtre principale ET à ce
/// dialogue lui-même ; Entrée/clic -> `set_active_theme` (écrit dans
/// `themes.json`, définitif) ; croix/Échap/B manette (sans avoir validé) ->
/// `revert_theme_preview` annule l'effet visuel de la dernière
/// prévisualisation (`active_theme` n'a jamais bougé pendant un survol).
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

/// Bouton "Files" du menu Settings -- raccourcis fichiers/dossiers de config
/// (ports.json/ports.local.json/state.json/themes.json), simple picker
/// générique (liste courte, pas besoin de recherche). Revérifie l'existence
/// au clic plutôt que de se fier à un état poussé à l'ouverture : le
/// fichier a pu disparaître entre-temps.
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

/// Code de locale ("" = suit le système) + nom affiché dans son propre
/// endonyme -- voir Tr.language-default-system pour pourquoi ces noms ne
/// passent pas par @tr(). Même codes que les dossiers sous `lang/` (voir
/// build.rs) : ce sont ceux que `select_bundled_translation` doit recevoir
/// tels quels pour retrouver le bon bundle.
const LANGUAGES: &[(&str, &str)] = &[
    ("", ""),
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

/// Bouton "Language" du menu Settings -- même liste recherchable que
/// `open_theme_picker`, sans aperçu en direct sur simple survol :
/// contrairement à un thème (couleurs pures, revert instantané), changer de
/// langue pendant la recherche n'apporterait rien de plus qu'un simple
/// surlignage -- Entrée/clic reste le seul geste qui applique réellement la
/// langue.
pub(crate) fn open_language_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let default_name = tr!(app).invoke_language_default_system().to_string();
    // (code, nom affiché dans son propre endonyme) -- "" a un nom TRADUIT
    // ("Default (System)"), donc calculé à part plutôt que lu directement
    // depuis LANGUAGES.
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
            // Live -- réévalue immédiatement tous les bindings @tr() déjà
            // affichés (voir le commentaire de select_bundled_translation
            // dans i-slint-core), aucun redémarrage requis.
            let _ = slint::select_bundled_translation(code);
            app.state.borrow_mut().set_language(code.to_string());
        },
        None,
    );
}

/// Bouton "Save Backup" du menu Settings -- exporte `save`/
/// `save2` de TOUT le catalogue (installé ou non) vers un dossier
/// daté, en tâche de fond (voir `core::save_backup::run_global_backup`).
///
/// La garde contre un double déclenchement se lit directement sur
/// `app.dialog_nav.dialogs` (`DialogSlot::Progress`) plutôt qu'un `Cell<bool>` séparé --
/// `open_progress_dialog` juste en dessous y bascule `app.dialog_nav.dialogs` de façon
/// synchrone, donc un second clic traité après le retour de cet appel voit
/// déjà `Progress`, sans fenêtre de course possible (callbacks Slint non
/// réentrants sur le thread UI).
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
