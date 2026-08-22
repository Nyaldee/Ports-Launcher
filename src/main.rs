// Sous-système GUI (pas console) -- sans ça, Windows ouvre une invite de
// commandes derrière l'appli à chaque lancement (comportement par défaut
// d'un `fn main()`, pensé pour des outils en ligne de commande).
#![windows_subsystem = "windows"]

mod core;
// Harnais de test visuel (--visual-stress-test) -- utile en dev, jamais dans
// l'exe distribué aux utilisateurs (voir son commentaire de module pour le
// détail du sandbox isolé).
#[cfg(debug_assertions)]
mod stress_test;
mod ui;

use core::jobs::InstallOutcome;
use core::models::{Port, SourceType};
use core::platform_utils::ExecutableSelectionError;
#[cfg(debug_assertions)]
use serde_json::json;
use serde_json::Value;
use slint::Model;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use ui::gamepad_router::{GamepadRouter, GamepadTarget};
use ui::windows_chrome;

slint::include_modules!();

/// Port de loopback arbitraire -- sert à la fois de VERROU (le premier
/// `bind` réussi fait foi, voir `acquire_single_instance_lock`) et de CANAL
/// DE SIGNAL (une seconde instance, qui échoue à bindre, s'y connecte pour
/// demander à la première de se remettre au premier plan -- voir le thread
/// `accept()` dans `main()` et `AppEvent::BringToForeground`). Un
/// `TcpListener` loopback plutôt qu'un mutex nommé Win32 : cross-platform
/// sans code conditionnel, et un mutex nommé n'offrirait que le verrou.
const SINGLE_INSTANCE_PORT: u16 = 57391;

/// Gauche/Droite en mode fenêtré (liste à une seule colonne) sautent d'une
/// "page" de ce nombre de lignes plutôt que de ne rien faire (voir
/// move_requested).
const PAGE_ROWS: i32 = 10;

/// Page ouverte par le bouton "GitHub" -- toujours la page du dépôt, y
/// compris quand le bouton passe en texte "Update" (voir
/// AppEvent::SelfUpdateAvailable), jamais un lien vers une release précise.
const GITHUB_URL: &str = "https://github.com/Nyaldee/Ports-Launcher";
/// Dépôt de référence pour la vérification de mise à jour du launcher
/// lui-même (voir start_self_update_check).
const SELF_REPO: &str = "Nyaldee/Ports-Launcher";
const DISCORD_URL: &str = "https://discord.com/invite/5GYmst9twA";

fn acquire_single_instance_lock() -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)).ok()
}

/// Dossier de l'exécutable en cours -- `ports.json`/`themes.json` vivent
/// toujours à côté de lui, jamais empaquetés dans le binaire, pour que
/// l'utilisateur puisse les éditer/mettre à jour sans recompiler.
fn base_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

fn to_port_items(ports: &[&Port], library_dir: &Path, update_cache: &HashMap<String, bool>) -> Vec<PortItem> {
    ports
        .iter()
        .map(|p| PortItem {
            name: p.name.clone().into(),
            update_available: update_cache.get(p.key()).copied().unwrap_or(false),
            installed: core::installer::is_installed(p, library_dir),
            is_local: p.source_type == SourceType::Local,
        })
        .collect()
}

/// Pousse les 7 couleurs éditables du thème + l'épaisseur de bordure + le
/// facteur d'échelle DPI sur N'IMPORTE QUELLE fenêtre Slint qui importe le
/// global `Theme` (voir theme-colors.slint) -- AppWindow ET chaque dialogue,
/// qui sont des composants Window SÉPARÉS avec chacun sa PROPRE instance de
/// ce global (les globals Slint sont attachés à la fenêtre qui les héberge,
/// jamais partagés entre fenêtres même si le .slint est le même fichier
/// importé partout).
macro_rules! apply_dialog_theme {
    ($dialog:expr, $app:expr) => {{
        // Sans ce push, chaque dialogue retombe sur la police par défaut de
        // Slint quel que soit "font_family" dans themes.json.
        $dialog.set_font_family($app.font_family.clone().into());
        let g = $dialog.global::<Theme>();
        let current = $app.theme_config.borrow().current;
        g.set_search_background(current.search_background);
        g.set_search_text(current.search_text);
        g.set_list_background(current.list_background);
        g.set_list_text(current.list_text);
        g.set_selected_background(current.selected_background);
        g.set_selected_text(current.selected_text);
        g.set_border_color(current.border);
        g.set_border_width($app.border_width.get());
        g.set_scale_factor($app.scale.get());
        // Croix de fermeture (DialogTitleBar dans dialogs.slint, partagée
        // par tous les dialogues) -- sans ce push, elle retombe sur les
        // valeurs de repli codées en dur dans card-grid.slint.
        let gc = $dialog.global::<GridColors>();
        gc.set_danger($app.semantic.danger);
        gc.set_text_on_accent($app.semantic.text_on_accent);
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
/// est le `Cell<i32>` d'`AppState` qui arbitre la sélection pour ce dialogue
/// (`error_nav_index`, `confirm_nav_index`, `info_nav_index`...).
macro_rules! wire_dialog_nav_hovered {
    ($dialog:expr, $app:expr, $field:ident) => {{
        let app2 = $app.clone();
        let dialog_weak = $dialog.as_weak();
        $dialog.on_nav_hovered(move |index| {
            app2.$field.set(index);
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
fn dialog_context(app: &Rc<AppState>) -> (FontSizes, slint::SharedString, i32, i32) {
    let big_mode = app.window().get_big_mode();
    let fonts = if big_mode { app.fullscreen_mode.borrow().fonts } else { app.normal_mode.borrow().fonts };
    let family = app.window().get_font_family();
    let (_, _, work_w, work_h) = windows_chrome::work_area_under_cursor();
    (fonts, family, work_w, work_h)
}

fn apply_theme(window: &AppWindow, theme: &ui::theme::ThemeConfig) {
    let t = window.global::<Theme>();
    t.set_search_background(theme.current.search_background);
    t.set_search_text(theme.current.search_text);
    t.set_list_background(theme.current.list_background);
    t.set_list_text(theme.current.list_text);
    t.set_selected_background(theme.current.selected_background);
    t.set_selected_text(theme.current.selected_text);
    t.set_border_color(theme.current.border);
    t.set_border_width(theme.border_width);

    // Couleurs de la grille plein écran -- dérivées du thème (voir
    // SemanticColors, pas éditables séparément dans themes.json) et poussées
    // ici plutôt que codées en dur dans le .slint.
    let g = window.global::<GridColors>();
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

/// Cible du linespace du texte, fraction de la hauteur d'ÉCRAN -- calibrée
/// empiriquement, utilisée uniquement par le mode PLEIN ÉCRAN (voir
/// resolve_font_sizes ; le mode fenêtré dérive géométriquement, voir
/// windowed_font_sizes).
///
/// Ne pas la réduire pour compenser un problème d'affichage sans d'abord
/// écarter un bug de mise à l'échelle DPI ailleurs dans le pipeline
/// (`Theme.scale-factor` vs le facteur utilisé pour la taille de fenêtre,
/// voir `scale` dans main()) : un tel bug rend TOUT le contenu ~2x trop
/// grand, et baisser cette constante casse le plein écran sans corriger la
/// vraie cause.
const ROW_HEIGHT_FRACTION: f32 = 0.0255;
/// Plancher de lisibilité en mode fenêtré uniquement -- jamais appliqué en
/// plein écran (qui garde seulement le plancher générique de 8px de
/// `font_metrics::solve_font_for_height`).
const MIN_ITEM_FONT_PX: i32 = 11;
/// Marge verticale fixe (pas un ratio de police) autour du texte d'une
/// ligne de la liste.
const ROW_VERTICAL_PADDING_PX: i32 = 6;
/// DOIT rester synchronisé avec Theme.title-button-padding (theme-colors.slint).
const TITLE_BAR_VERTICAL_PADDING_PX: i32 = 10;
/// Padding vertical de la barre de recherche EN PLEIN ÉCRAN uniquement --
/// côté fenêtré, search_bar_height_px est géométrique (unit_h_px *
/// SEARCH_BAR_UNITS, voir windowed_font_sizes).
const SEARCH_BAR_VERTICAL_PADDING_BIG_PX: i32 = 16;
const TITLE_FONT_RATIO: f32 = 18.0 / 20.0;
/// Même ratio que search-font-px côté app-window.slint -- DOIT rester
/// synchronisé avec cette valeur-là.
const SEARCH_FONT_RATIO: f32 = 1.2;
/// Cible physique -- DOIT rester synchronisée avec content-margin dans
/// app-window.slint (`12px / Theme.scale-factor` côté .slint, donc 12
/// physique quel que soit le DPI, même principe que title_bar_height_px
/// ci-dessous) : utilisée pour reconstruire EXACTEMENT le même espace
/// disponible que le rendu .slint produira, voir windowed_font_sizes.
const CONTENT_MARGIN_PX: i32 = 12;
/// Cible physique -- DOIT rester synchronisée avec row-spacing dans
/// app-window.slint (`6px / Theme.scale-factor`, même principe que
/// CONTENT_MARGIN_PX ci-dessus). footer-body n'occupe que `row-height -
/// row-spacing * 2` (pas une unité pleine, voir sa hauteur dans le .slint) --
/// sans compenser cet écart dans windowed_font_sizes, content-body (seul
/// enfant élastique, vertical-stretch: 1) absorberait ce surplus en trop,
/// laissant une 21e ligne partiellement visible sous les 20 lignes pleines.
const ROW_SPACING_PX: i32 = 6;
/// Nombre de lignes de la liste fenêtrée visibles SANS scroll,
/// géométriquement garanti quelle que soit la valeur choisie ici. La
/// fenêtre garde exactement la taille de compute_window_size_for (la
/// redimensionner casserait le ratio voulu) : c'est la POLICE de la liste
/// qui s'ajuste pour que ces lignes tiennent -- voir windowed_font_sizes.
const VISIBLE_ROWS: i32 = 25;
/// Plancher physique de `unit_h_px` (voir windowed_font_sizes) -- sans lui,
/// une fenêtre réduite jusqu'à WINDOW_MIN_WIDTH fait descendre la hauteur de
/// la barre de titre sous title-vertical-padding*2 (20px, voir
/// app-window.slint), rendant title-button-size NÉGATIVE : les icônes de la
/// barre de titre et les boutons de ligne disparaissent au lieu de
/// rapetisser. Une fois ce plancher atteint, VISIBLE_ROWS lignes ne
/// tiennent plus forcément toutes -- voir content_body_height_px, qui
/// bascule alors sur un sous-ensemble scrollable.
const MIN_UNIT_H_PX: i32 = 32;

/// Tailles de police/hauteurs dérivées, résolues une fois par mesure GDI
/// réelle (voir ui::font_metrics), poussées telles quelles vers le .slint
/// (voir apply_font_sizes) plutôt que recalculées en fraction arbitraire à
/// l'intérieur du .slint lui-même.
#[derive(Clone, Copy)]
struct FontSizes {
    item_font_px: i32,
    title_font_px: i32,
    row_height_px: i32,
    search_bar_height_px: i32,
    title_bar_height_px: i32,
    /// Hauteur RÉELLE de content-body en mode fenêtré (voir
    /// windowed_font_sizes) -- sans objet en plein écran, où CardGrid n'a
    /// pas l'invariant "N lignes tiennent pile" et où le .slint ne lit
    /// jamais ce champ (voir content-body dans app-window.slint).
    content_body_height_px: i32,
}

/// Dérive titre/recherche/hauteurs à partir d'un item_font_px déjà résolu
/// par l'appelant (resolve_font_sizes via ROW_HEIGHT_FRACTION,
/// windowed_font_sizes via la géométrie) -- une seule formule
/// title/search partagée par les deux.
fn font_sizes_from_item_font(family: &str, item_font_px: i32, item_linespace: i32, search_padding_px: i32) -> FontSizes {
    let title_font_px = ((item_font_px as f32) * TITLE_FONT_RATIO).round().max(8.0) as i32;
    let title_linespace = ui::font_metrics::linespace_for_size(family, title_font_px);
    // Hauteur de la barre de recherche mesurée sur SA PROPRE police
    // (search-font-px = item-font-px * 1.2 côté .slint), PAS sur celle de la
    // liste -- sinon la barre serait trop petite pour son propre texte.
    let search_font_px = ((item_font_px as f32) * SEARCH_FONT_RATIO).round().max(8.0) as i32;
    let search_linespace = ui::font_metrics::linespace_for_size(family, search_font_px);

    let row_height_px = item_linespace + 2 * ROW_VERTICAL_PADDING_PX;
    FontSizes {
        item_font_px,
        title_font_px,
        row_height_px,
        search_bar_height_px: search_linespace + 2 * search_padding_px,
        title_bar_height_px: title_linespace + 2 * TITLE_BAR_VERTICAL_PADDING_PX,
        content_body_height_px: row_height_px,
    }
}

/// `base_height_px` = hauteur de la fenêtre ou de l'écran, en pixels
/// PHYSIQUES. `min_item_font_px` = 0 désactive le plancher de lisibilité
/// (voir MIN_ITEM_FONT_PX). Utilisée pour le mode PLEIN ÉCRAN : la grille de
/// cartes n'a pas de "nombre de lignes cible" comme la liste fenêtrée.
fn resolve_font_sizes(family: &str, base_height_px: i32, search_padding_px: i32, min_item_font_px: i32) -> FontSizes {
    let target = ((base_height_px as f32) * ROW_HEIGHT_FRACTION).round() as i32;
    let (mut item_font_px, mut item_linespace) = ui::font_metrics::solve_font_for_height(family, target);
    if item_font_px < min_item_font_px {
        item_font_px = min_item_font_px;
        item_linespace = ui::font_metrics::linespace_for_size(family, item_font_px);
    }
    font_sizes_from_item_font(family, item_font_px, item_linespace, search_padding_px)
}

/// L'espace disponible est divisé en tranches ÉGALES, la barre de recherche
/// valant volontairement le double d'une ligne -- VISIBLE_ROWS tranches
/// pour les lignes, une pour la barre de titre, une pour le pied de page.
const SEARCH_BAR_UNITS: i32 = 2;
const TITLE_BAR_UNITS: i32 = 1;
const FOOTER_UNITS: i32 = 1;

/// Police ET hauteurs de la liste fenêtrée. La hauteur de chaque rangée est
/// une DIVISION PURE de l'espace disponible, jamais dépendante d'une
/// police ; la police n'est dérivée qu'APRÈS coup pour habiller l'espace
/// déjà fixé. `rows` lignes tiennent donc toujours, par construction
/// arithmétique -- une itération police -> budget -> police pourrait au
/// contraire osciller et rendre le nombre de lignes visibles imprévisible.
fn windowed_font_sizes(family: &str, window_height_px: i32, border_width_px: i32, rows: i32) -> FontSizes {
    let total_units = rows + SEARCH_BAR_UNITS + TITLE_BAR_UNITS + FOOTER_UNITS;
    // border*2 (haut+bas) + 4×content-margin (padding-top/bottom du
    // VerticalLayout + son spacing entre CHACUNE des 3 rangées barre de
    // recherche/contenu/pied de page, voir app-window.slint) -- tout ce qui
    // n'est PAS une des `total_units` tranches. + 2×ROW_SPACING_PX : compense
    // le fait que footer-body ne consomme réellement qu'une unité MOINS
    // 2×row-spacing (voir sa déclaration), pas une unité pleine -- sans ce
    // terme, l'unité calculée serait trop petite, laissant systématiquement
    // 2×ROW_SPACING_PX de trop dans content-body (le seul enfant élastique),
    // débordant sous forme d'une 21e ligne partielle (voir ROW_SPACING_PX).
    let available_px = (window_height_px - border_width_px * 2 - CONTENT_MARGIN_PX * 4 + ROW_SPACING_PX * 2).max(total_units);
    // MIN_UNIT_H_PX plutôt que .max(1) -- voir son commentaire.
    let unit_h_px = (available_px / total_units).max(MIN_UNIT_H_PX);

    // La mesure GDI ne sert qu'à choisir item_font_px -- row_height_px reste
    // unit_h_px, purement géométrique.
    let target_linespace = (unit_h_px - 2 * ROW_VERTICAL_PADDING_PX).max(1);
    let (mut item_font_px, _) = ui::font_metrics::solve_font_for_height(family, target_linespace);
    if item_font_px < MIN_ITEM_FONT_PX {
        item_font_px = MIN_ITEM_FONT_PX;
    }
    // title_bar_height_px/search_bar_height_px restent géométriques
    // (unit_h_px * N) -- title_font_px n'habille que le texte du titre.
    let title_font_px = ((item_font_px as f32) * TITLE_FONT_RATIO).round().max(8.0) as i32;

    // Espace RÉELLEMENT laissé aux lignes une fois recherche/titre/pied de
    // page servis -- pas toujours `rows * unit_h_px` : au plancher
    // MIN_UNIT_H_PX, ces trois-là peuvent consommer plus que `available_px`,
    // donc min(...) plafonne à ce qui reste. .max(unit_h_px) garantit au
    // moins une ligne visible ; en-dessous de `rows` lignes pleines, la
    // ListView défile pour le reste (voir port-list dans app-window.slint).
    let content_body_height_px =
        (unit_h_px * rows).min((available_px - unit_h_px * (SEARCH_BAR_UNITS + TITLE_BAR_UNITS + FOOTER_UNITS)).max(unit_h_px));

    FontSizes {
        item_font_px,
        title_font_px,
        row_height_px: unit_h_px,
        search_bar_height_px: unit_h_px * SEARCH_BAR_UNITS,
        title_bar_height_px: unit_h_px * TITLE_BAR_UNITS,
        content_body_height_px,
    }
}

/// Pousse les tailles mesurées en pixels PHYSIQUES : app-window.slint les
/// reçoit dans les propriétés `*-physical` et dérive lui-même les versions
/// logiques (`/ Theme.scale-factor`). Aucune division côté Rust pour CES
/// propriétés-ci, contrairement à initial-width/height (la fenêtre n'a pas
/// de `Theme.scale-factor` fiable avant son premier show(), alors que
/// celles-ci ne sont poussées qu'après).
fn apply_font_sizes(window: &AppWindow, f: &FontSizes) {
    window.set_item_font_px_physical(f.item_font_px as f32);
    window.set_title_font_px_physical(f.title_font_px as f32);
    window.set_row_height_physical(f.row_height_px as f32);
    window.set_search_bar_height_physical(f.search_bar_height_px as f32);
    window.set_title_bar_height_physical(f.title_bar_height_px as f32);
    // Hauteur EXACTE de content-body en mode fenêtré, pas une fraction
    // élastique de ce qui reste dans le VerticalLayout. Poussée aussi en
    // plein écran, où le .slint l'ignore.
    window.set_content_body_height_physical(f.content_body_height_px as f32);
}

/// Géométrie complète d'un des deux modes (fenêtré ou plein écran) --
/// recalculée à chaque bascule par `toggle_fullscreen` (voir
/// `compute_mode_geometry`) pour capter un éventuel changement d'échelle
/// Windows survenu en cours de session.
struct ModeGeometry {
    /// Pixels LOGIQUES -- poussés tels quels vers `initial-width/height`
    /// (voir leur commentaire dans app-window.slint).
    logical_width: f32,
    logical_height: f32,
    /// Pixels PHYSIQUES -- poussés tels quels vers `WindowPosition::Physical`.
    pos_x: i32,
    pos_y: i32,
    fonts: FontSizes,
}

/// `scale` = facteur d'échelle DPI fourni par l'appelant (voir
/// `windows_chrome::scale_factor_under_cursor`), jamais remesuré ici : cette
/// fonction produit une géométrie de mode figée.
fn compute_mode_geometry(
    family: &str,
    area: (i32, i32, i32, i32),
    scale: f32,
    big_mode: bool,
    width_fraction: f64,
    border_width_px: i32,
) -> ModeGeometry {
    let (area_x, area_y, screen_w, screen_h) = area;
    if big_mode {
        ModeGeometry {
            logical_width: screen_w as f32 / scale,
            logical_height: screen_h as f32 / scale,
            pos_x: area_x,
            pos_y: area_y,
            fonts: resolve_font_sizes(family, screen_h, SEARCH_BAR_VERTICAL_PADDING_BIG_PX, 0),
        }
    } else {
        // La taille de fenêtre fixe le cadre (voir compute_window_size_for) ;
        // c'est la police de la liste qui s'adapte pour que VISIBLE_ROWS
        // lignes pleines y tiennent, jamais l'inverse.
        let (win_width, win_height) = ui::geometry::compute_window_size_for(screen_w, screen_h, width_fraction);
        let fonts = windowed_font_sizes(family, win_height, border_width_px, VISIBLE_ROWS);
        ModeGeometry {
            logical_width: win_width as f32 / scale,
            logical_height: win_height as f32 / scale,
            pos_x: area_x + (screen_w - win_width) / 2,
            pos_y: area_y + (screen_h - win_height) / 2,
            fonts,
        }
    }
}

fn apply_mode_geometry(window: &AppWindow, mode: &ModeGeometry) {
    window.set_initial_width(mode.logical_width);
    window.set_initial_height(mode.logical_height);
    window.window().set_position(slint::WindowPosition::Physical(slint::PhysicalPosition { x: mode.pos_x, y: mode.pos_y }));
    apply_font_sizes(window, &mode.fonts);
}

/// Image d'une carte depuis le cache disque uniquement -- jamais de
/// téléchargement déclenché par l'affichage de la grille, le cache est
/// rempli à l'installation (voir core::installer). Absente/illisible ->
/// image vide, et `card.image.width == 0` bascule sur le repli texte côté
/// .slint (voir card-grid.slint).
///
/// `image_cache` évite de relire+redécoder le disque à chaque appel :
/// `rebuild_grid` tourne à chaque frappe de recherche, install/désinstall et
/// entrée en plein écran (survol et navigation clavier/manette ne font que
/// déplacer la surbrillance, voir `refresh_grid_selection`, jamais un
/// rebuild), donc sans ce cache une bibliothèque de 20-30 jeux redécode
/// autant de PNG/JPEG à chaque recherche. `slint::Image` se clone en O(1)
/// (poignée partagée), jamais une copie de pixels.
fn load_cached_card_image(image_cache: &RefCell<HashMap<String, slint::Image>>, cache_dir: &Path, folder_name: &str) -> slint::Image {
    if let Some(img) = image_cache.borrow().get(folder_name) {
        return img.clone();
    }
    let image = (|| {
        let path = core::image_cache::cached_image_path(cache_dir, folder_name).ok()?;
        let bytes = std::fs::read(&path).ok()?;
        let decoded = image::load_from_memory(&bytes).ok()?;
        let rgba = decoded.into_rgba8();
        let (w, h) = rgba.dimensions();
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba.as_raw(), w, h);
        Some(slint::Image::from_rgba8(buffer))
    })()
    .unwrap_or_default();
    image_cache.borrow_mut().insert(folder_name.to_string(), image.clone());
    image
}

fn build_card_item(image_cache: &RefCell<HashMap<String, slint::Image>>, port: &Port, cache_dir: &Path, selected: bool) -> CardItem {
    CardItem { name: port.name.clone().into(), image: load_cached_card_image(image_cache, cache_dir, &port.folder_name), selected }
}

/// Regroupe les ports installés en lignes de `columns` cartes -- la
/// ListView de card-grid.slint virtualise par LIGNE, pas par carte (voir
/// core::grid). `selected` = (ligne, colonne) en surbrillance.
fn build_card_rows(
    image_cache: &RefCell<HashMap<String, slint::Image>>,
    ports: &[Port],
    cache_dir: &Path,
    columns: usize,
    // `None` -- aucune carte en surbrillance (souris sortie de la grille,
    // voir `grid_mouse_active`) sans pour autant perdre la position
    // mémorisée dont la navigation clavier/manette repart.
    selected: Option<(usize, usize)>,
) -> Vec<CardRow> {
    let cols = columns.max(1);
    let items: Vec<CardItem> = ports
        .iter()
        .enumerate()
        .map(|(i, p)| build_card_item(image_cache, p, cache_dir, Some((i / cols, i % cols)) == selected))
        .collect();
    core::grid::chunk_into_rows(&items, columns)
        .into_iter()
        .map(|cards| CardRow { cards: slint::ModelRc::new(slint::VecModel::from(cards)) })
        .collect()
}

/// Verrou tolérant à l'empoisonnement. `.lock().unwrap()` condamnerait le
/// Mutex dès qu'UN thread panique en le tenant (une vérif de MAJ sur une
/// réponse API inattendue, par exemple) : tout locker suivant, thread UI
/// compris, paniquerait à son tour et tuerait l'appli. Les données protégées
/// ici sont une simple file de messages, sans invariant qu'un producteur
/// mort en plein milieu pourrait laisser cassé -- les récupérer malgré
/// l'empoisonnement est donc sûr.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Messages produits par les threads d'arrière-plan (install, vérif de MAJ,
/// sync du catalogue) -- que des données `Send`, jamais un `Rc`/composant
/// Slint : empilés dans `AppState.events` depuis n'importe quel thread,
/// dépilés et appliqués uniquement par `poll_app_events` sur le thread UI.
/// Ce détour évite d'avoir à rendre `Rc<AppState>` `Send` (il ne l'est pas)
/// et se contente du `slint::Timer` qui réveille déjà le thread UI, sans
/// `slint::invoke_from_event_loop`.
enum AppEvent {
    InstallProgress { message: String },
    InstallDone { key: String, tag: Option<String> },
    InstallAssetAmbiguous { key: String, assets: Vec<Value> },
    InstallError { key: String, message: String },
    UpdateCheckResult { key: String, available: bool },
    UpdateCheckError(String),
    SelfUpdateAvailable,
    /// Voir `open_version_picker` -- liste des releases disponibles pour
    /// `key` récupérée en arrière-plan, prête à peupler un `ListPickerDialog`.
    VersionsFetched { key: String, releases: Vec<Value> },
    VersionsFetchError { key: String, message: String },
    /// Voir `repair_missing_cached_image` -- la jaquette de `folder_name`
    /// vient d'être retéléchargée avec succès en arrière-plan, la grille
    /// doit relire le fichier au prochain rendu au lieu du repli texte déjà
    /// affiché.
    ImageCached { folder_name: String },
    /// Une seconde instance vient de se connecter au verrou loopback pour
    /// signaler sa présence -- voir le thread `accept()` dans `main()` et
    /// `SINGLE_INSTANCE_PORT`.
    BringToForeground,
    /// Voir `start_catalog_sync` -- un `ports.json` plus récent vient d'être
    /// téléchargé ET déjà validé (voir `catalog_sync::fetch_if_changed`),
    /// prêt à remplacer `app.catalog` et rafraîchir la vue affichée.
    RemoteCatalogFetched(Vec<Port>),
    /// Voir `start_catalog_sync` -- une vérification vient de se conclure
    /// (`304` ou `200`, jamais sur une erreur réseau) : remet le throttle à
    /// zéro depuis le thread UI, `StateManager` n'étant jamais muté depuis
    /// un thread d'arrière-plan.
    PortsCheckDone { etag: String },
}

/// Un seul dialogue ouvert à la fois -- jamais deux dialogues modaux
/// affichés en même temps. Possède directement la fenêtre Slint (elle
/// serait fermée/détruite si on ne la gardait pas en vie ici).
enum DialogSlot {
    None,
    Message(MessageDialog),
    Confirm(ConfirmDialog),
    Error(ErrorDialog),
    Info(InfoDialog),
    Progress(ProgressDialog),
    Picker(ListPickerDialog),
    Settings(SettingsDialog),
}

/// État partagé entre les callbacks Slint (souris/clavier) et la cible
/// manette (voir ui::gamepad_router) -- un seul Rc cloné partout plutôt
/// qu'une dizaine de `Rc<RefCell<...>>` séparés, pour que les deux chemins
/// d'entrée appellent exactement la même logique.
struct AppState {
    window: slint::Weak<AppWindow>,
    state: RefCell<core::state::StateManager>,
    /// `RefCell` : un rafraîchissement distant réussi peut remplacer le
    /// catalogue en cours de session (voir AppEvent::RemoteCatalogFetched).
    catalog: RefCell<Vec<Port>>,
    library_dir: PathBuf,
    cache_dir: PathBuf,
    /// Dossier contenant `ports.json`/`ports.local.json`/`state.json`
    /// (`bdir` dans `main()`) -- pour les raccourcis fichiers du dialogue
    /// Settings. `themes.json` passe par `themes_path` plutôt que d'être
    /// reconstruit depuis ici.
    config_dir: PathBuf,
    /// Recalculé à chaque entrée en plein écran (voir `recompute_grid_columns`)
    /// -- figé au démarrage causait une grille mal centrée si l'utilisateur
    /// déplaçait la fenêtre vers un moniteur de résolution différente avant
    /// de basculer.
    grid_columns: Cell<usize>,
    /// Struct COMPLÈTE (thèmes nommés/thème actif/couleurs appliquées),
    /// contrairement à `semantic` -- nécessaire au sélecteur de thème
    /// (`open_settings_dialog` et `ui::theme::preview_theme`/
    /// `list_theme_names`/`commit_theme` y lisent/écrivent). `apply_theme`
    /// la prend en paramètre plutôt que de lire ce champ, pour rester
    /// appelable au démarrage comme pendant une prévisualisation.
    theme_config: RefCell<ui::theme::ThemeConfig>,
    semantic: ui::theme::SemanticColors,
    /// Résolue une fois au démarrage (`ui::theme::resolve_font_family`) --
    /// poussée à la fenêtre principale ET, via `apply_dialog_theme!`, à
    /// chaque dialogue : une seule source de vérité pour toute l'appli.
    font_family: String,
    /// `Cell` : modifiable en direct par Ctrl+-/Ctrl+= (voir
    /// `adjust_border`), `AppState` étant partagé via `Rc` entre callbacks
    /// Slint et donc jamais accessible en `&mut`.
    border_width: Cell<i32>,
    /// Fraction 0.05-1.0 de la taille d'écran visée par le mode FENÊTRÉ --
    /// `Cell` pour la même raison que `border_width` (Ctrl+chiffres, voir
    /// `set_window_size_percent`). Sans objet en plein écran, qui occupe
    /// l'écran entier par définition.
    window_width_fraction: Cell<f64>,
    /// Facteur d'échelle DPI réel, initialisé juste après le premier
    /// `show()`. `recompute_normal_mode`/`toggle_fullscreen` le remettent à
    /// jour depuis `window.scale_factor()` -- seule façon dont un changement
    /// d'échelle Windows en cours de session se répercute, sans Timer dédié.
    /// `apply_dialog_theme!` le relit à chaque ouverture de dialogue.
    scale: Cell<f32>,
    /// Chemin de `themes.json` -- persiste immédiatement la taille de
    /// fenêtre/bordure choisie au clavier (voir `set_window_size_percent`/
    /// `adjust_border`), comme `commit_theme` pour le sélecteur de thème.
    themes_path: PathBuf,
    /// Géométrie du mode FENÊTRÉ -- calculée au démarrage, recalculée
    /// UNIQUEMENT sur une demande explicite de redimensionnement clavier
    /// (voir `set_window_size_percent`/`adjust_border`). `toggle_fullscreen`
    /// la relit telle quelle sans jamais la recalculer, pour ne pas "bouger"
    /// d'un aller-retour plein écran à l'autre.
    normal_mode: RefCell<ModeGeometry>,
    /// Géométrie du mode plein écran -- les raccourcis de redimensionnement
    /// ne s'y appliquent pas (garde `!root.big-mode` dans app-window.slint).
    /// `RefCell` quand même : `toggle_fullscreen` la recalcule à chaque
    /// ENTRÉE en plein écran avec le `scale` du moment.
    fullscreen_mode: RefCell<ModeGeometry>,
    /// Ports affichés dans la grille plein écran -- reconstruits à chaque
    /// entrée en plein écran et après chaque install/désinstall (voir
    /// rebuild_grid) ; `refresh_grid_selection` se contente de relire cette
    /// liste pour recolorer la sélection.
    displayed_installed: RefCell<Vec<Port>>,
    grid_selected: Cell<(usize, usize)>,
    /// Faux entre un `card-unhovered` (souris sortie de la grille) et le
    /// prochain survol/navigation -- masque seulement la surbrillance dans
    /// `build_card_rows` sans toucher `grid_selected`, pour que la
    /// navigation clavier/manette reprenne là où le survol l'avait laissée
    /// plutôt qu'en (0, 0).
    grid_mouse_active: Cell<bool>,
    /// Dernier clic souris sur une carte en plein écran (position, instant)
    /// -- voir `on_card_activated` : un clic isolé ne fait que sélectionner,
    /// lancer exige un vrai double-clic dans le délai configuré par Windows.
    /// `None` tant qu'aucun clic n'a eu lieu, ou juste après qu'un
    /// double-clic ait été consommé (un 3e clic rapide ne redéclenche rien).
    last_card_click: Cell<Option<((usize, usize), Instant)>>,
    /// `windows_chrome::double_click_time_ms()`, lu une fois au démarrage --
    /// le délai réel configuré dans Windows, jamais une valeur en dur qui
    /// ignorerait les réglages d'accessibilité de l'utilisateur.
    double_click_ms: u32,
    /// Ports actuellement affichés dans la liste fenêtrée (après filtrage
    /// par la recherche) -- dans le même ordre que le modèle Slint.
    displayed_windowed: RefCell<Vec<Port>>,
    windowed_selected: Cell<usize>,
    /// Dernière recherche tapée -- retenue pour reconstruire la liste après
    /// un install/uninstall/vérif de MAJ sans perdre le filtre en cours
    /// (voir refresh_current_view).
    search_query: RefCell<String>,
    /// Clés (`Port::key`) actuellement en cours d'installation -- ignore les
    /// activations répétées pendant qu'un install tourne déjà.
    installing: RefCell<HashSet<String>>,
    /// Process lancé pour chaque port en cours d'exécution (voir
    /// `is_port_running`) -- évite de relancer un port déjà ouvert et
    /// interdit désinstallation/mise à jour pendant qu'il tourne :
    /// `remove_dir_all`/l'extraction échoueraient sur des fichiers
    /// verrouillés par Windows, potentiellement après avoir déjà supprimé
    /// une partie de l'arborescence.
    running_processes: RefCell<HashMap<String, core::launch::LaunchedProcess>>,
    /// Résultat de la dernière vérification de mise à jour par clé --
    /// purement indicatif (voir PortItem.update-available).
    update_cache: RefCell<HashMap<String, bool>>,
    /// Vérifications de MAJ encore en vol (voir start_update_checks) --
    /// `state.mark_release_check()` n'est appelé qu'une fois ce compteur
    /// revenu à 0, jamais après chaque résultat individuel.
    pending_update_checks: Cell<usize>,
    events: Arc<Mutex<Vec<AppEvent>>>,
    dialogs: RefCell<DialogSlot>,
    /// Option actuellement en surbrillance manette dans un ListPickerDialog
    /// ouvert (voir DialogGamepadTarget) -- sans objet tant qu'aucun picker
    /// n'est affiché.
    picker_index: Cell<i32>,
    /// Bouton actuellement en surbrillance manette dans un InfoDialog ouvert
    /// (Website=0/Mods=1/Game folder=2/Save folder=3/Save folder 2=4, voir
    /// DialogGamepadTarget et InfoDialog.selected-index) -- sans objet tant
    /// qu'aucun InfoDialog n'est affiché.
    info_nav_index: Cell<i32>,
    /// Bouton en surbrillance manette/clavier dans un ConfirmDialog ouvert
    /// (0 = confirmer, 1 = annuler, voir DialogGamepadTarget et
    /// ConfirmDialog.selected-index) -- sans objet tant qu'aucun n'est affiché.
    confirm_nav_index: Cell<i32>,
    /// Bouton en surbrillance manette/clavier dans un ErrorDialog ouvert
    /// (0 = réinstaller, 1 = infos, voir DialogGamepadTarget et
    /// ErrorDialog.selected-index) -- sans objet tant qu'aucun n'est affiché.
    error_nav_index: Cell<i32>,
    /// Bouton en surbrillance dans la rangée de raccourcis d'un
    /// SettingsDialog ouvert (Library=0/ports.json=1/ports.local.json=2/
    /// state.json=3/themes.json=4, voir SettingsDialog.footer-selected-index).
    /// Contrairement à `info_nav_index`, -1 est ici un état valide et
    /// courant : la liste de thèmes garde le focus à l'ouverture jusqu'à ce
    /// que Gauche/Droite fasse entrer dans la rangée, Haut/Bas le rend à la
    /// liste (voir move_selection). Arbitre aussi ce qu'active_selection
    /// déclenche : un thème (-1) ou un raccourci (0..4).
    footer_nav_index: Cell<i32>,
    /// Vrai entre la minimisation faite par `launch_executable` au lancement
    /// d'un jeu en plein écran et la remontée par `poll_app_events` -- sert
    /// à distinguer NOTRE minimisation de celle d'un raccourci Windows,
    /// qu'il ne faudrait surtout pas annuler de force.
    minimized_for_game: Cell<bool>,
    /// Cache mémoire des jaquettes déjà décodées (clé : folder_name, voir
    /// load_cached_card_image) -- une entrée n'est invalidée qu'après un
    /// (ré)install (voir AppEvent::InstallDone), une jaquette ne changeant
    /// pas spontanément.
    card_image_cache: RefCell<HashMap<String, slint::Image>>,
    /// Vrai uniquement sous `--visual-stress-test` -- fait bifurquer
    /// start_install et launch_executable vers un chemin 100% local, sans
    /// téléchargement ni lancement de process réel.
    stress_test: bool,
}

impl AppState {
    fn window(&self) -> AppWindow {
        self.window.unwrap()
    }

    /// Le port en surbrillance, quel que soit le mode affiché -- source
    /// unique de vérité pour "sur quoi agit Entrée/A/clic".
    fn current_selected_port(&self) -> Option<Port> {
        if self.window().get_big_mode() {
            let (row, col) = self.grid_selected.get();
            let idx = row * self.grid_columns.get().max(1) + col;
            self.displayed_installed.borrow().get(idx).cloned()
        } else {
            self.displayed_windowed.borrow().get(self.windowed_selected.get()).cloned()
        }
    }

    /// Reconstruit la liste filtrée/triée pour `query` -- conserve la
    /// sélection sur le MÊME port (par `Port::key`) s'il est toujours
    /// affiché après filtrage, sinon repart du premier élément.
    fn rebuild_windowed(&self, query: &str) {
        *self.search_query.borrow_mut() = query.to_string();
        let previously_selected_key =
            self.displayed_windowed.borrow().get(self.windowed_selected.get()).map(|p| p.key().to_string());
        let catalog = self.catalog.borrow();
        let pool: Vec<&Port> = catalog.iter().collect();
        let filtered = core::search::filter_and_sort(&pool, query);
        let displayed: Vec<Port> = filtered.iter().map(|p| (*p).clone()).collect();
        let new_index = previously_selected_key.and_then(|k| displayed.iter().position(|p| p.key() == k)).unwrap_or(0);
        *self.displayed_windowed.borrow_mut() = displayed;
        self.windowed_selected.set(new_index);
        self.rebuild_ports_model();
    }

    /// Reconstruit ENTIÈREMENT le modèle `ports` -- uniquement quand la
    /// LISTE a changé (recherche, install/désinstall), jamais pour un simple
    /// changement de sélection (voir push_selected_index) : la ligne
    /// surlignée se déduit d'un `index == root.selected-index` côté .slint,
    /// et remplacer tout le modèle à chaque survol souris perturberait le
    /// défilement à la molette en cours.
    fn rebuild_ports_model(&self) {
        let displayed = self.displayed_windowed.borrow();
        let refs: Vec<&Port> = displayed.iter().collect();
        let items = to_port_items(&refs, &self.library_dir, &self.update_cache.borrow());
        self.window().set_ports(slint::ModelRc::new(slint::VecModel::from(items)));
        self.push_selected_index();
    }

    /// Pousse UNIQUEMENT l'index sélectionné -- jamais le modèle `ports`
    /// lui-même, voir rebuild_ports_model.
    fn push_selected_index(&self) {
        let len = self.displayed_windowed.borrow().len();
        let window = self.window();
        // -1 sur une liste vide, jamais 0 : 0 ferait défiler vers une
        // première ligne qui n'existe pas.
        window.set_selected_index(if len == 0 { -1 } else { self.windowed_selected.get() as i32 });
    }

    fn move_windowed_selection(&self, dy: i32) {
        let len = self.displayed_windowed.borrow().len();
        if len == 0 {
            return;
        }
        let current = self.windowed_selected.get() as i32;
        let next = (current + dy).clamp(0, len as i32 - 1) as usize;
        if next != self.windowed_selected.get() {
            self.windowed_selected.set(next);
            self.push_selected_index();
            self.trigger_scroll();
        }
    }

    /// Bascule `scroll-trigger` pour ramener la sélection dans la vue --
    /// appelée UNIQUEMENT depuis la navigation clavier/manette, jamais
    /// depuis le survol souris : un survol pendant un défilement à la
    /// molette ne doit pas l'interrompre.
    fn trigger_scroll(&self) {
        let window = self.window();
        window.set_scroll_trigger(!window.get_scroll_trigger());
    }

    /// Reconstruit la liste des ports installés + la grille. À l'entrée en
    /// plein écran, `preserve_selection: false` repart de (0, 0) ; après un
    /// install/uninstall grille affichée, `true` retrouve le même port par
    /// clé s'il est toujours présent, comme `rebuild_windowed`. Filtrée par
    /// `self.search_query`, les deux modes partageant la même requête.
    fn rebuild_grid(&self, preserve_selection: bool) {
        let query = self.search_query.borrow().clone();
        // Vérité disque via `installer::is_installed`, pas `state.json` (qui
        // ne fait que du bookkeeping de tag/date) -- reflète l'état réel de
        // `Library` même si `state.json` est périmé ou absent. Filtre sur
        // des références dans `self.catalog` : un seul clone final.
        let catalog = self.catalog.borrow();
        let installed_refs: Vec<&Port> =
            catalog.iter().filter(|p| core::installer::is_installed(p, &self.library_dir)).collect();
        let installed: Vec<Port> =
            core::search::filter_and_sort(&installed_refs, &query).iter().map(|p| (*p).clone()).collect();
        let columns = self.grid_columns.get().max(1);
        let previously_selected_key = if preserve_selection {
            let (row, col) = self.grid_selected.get();
            self.displayed_installed.borrow().get(row * columns + col).map(|p| p.key().to_string())
        } else {
            None
        };
        let new_flat = previously_selected_key.and_then(|k| installed.iter().position(|p| p.key() == k)).unwrap_or(0);
        self.grid_selected.set((new_flat / columns, new_flat % columns));
        // Reconstruction complète, pas un simple survol -- la surbrillance
        // est toujours affichée dans ce cas (voir grid_mouse_active).
        self.grid_mouse_active.set(true);
        self.window().set_card_rows(slint::ModelRc::new(slint::VecModel::from(build_card_rows(
            &self.card_image_cache,
            &installed,
            &self.cache_dir,
            self.grid_columns.get(),
            Some(self.grid_selected.get()),
        ))));
        *self.displayed_installed.borrow_mut() = installed;
    }

    fn enter_fullscreen(&self) {
        self.recompute_grid_columns();
        self.rebuild_grid(false);
    }

    /// Recalcule `grid_columns` depuis la zone de travail SOUS LE CURSEUR et
    /// l'échelle courante (déjà rafraîchie par `compute_live_mode`, appelé
    /// juste avant par `toggle_fullscreen`) -- sinon un changement de
    /// moniteur avant un passage en plein écran laisserait la grille sur un
    /// nombre de colonnes périmé (le centrage dans card-grid.slint suppose
    /// qu'il correspond exactement à ce qui est affiché).
    fn recompute_grid_columns(&self) {
        let (_, _, screen_w, _) = windows_chrome::work_area_under_cursor();
        let grid_available_width = screen_w as f32 / self.scale.get();
        let columns = core::grid::compute_grid_columns(grid_available_width);
        self.grid_columns.set(columns);
        self.window().set_grid_columns(columns as i32);
    }

    /// Bascule la surbrillance d'UNE carte dans le modèle déjà affiché --
    /// jamais un rebuild (voir build_card_rows/rebuild_grid, qui eux
    /// reconstruisent tout). `cards` reste le même `ModelRc` que celui déjà
    /// dans `card_rows` : le modifier via `set_row_data` met donc directement
    /// à jour l'affichage sans recréer aucun `CardItem`/`ModelRc`.
    fn set_card_highlight(window: &AppWindow, pos: Option<(usize, usize)>, selected: bool) {
        let Some((row, col)) = pos else { return };
        let Some(card_row) = window.get_card_rows().row_data(row) else { return };
        let Some(mut item) = card_row.cards.row_data(col) else { return };
        if item.selected != selected {
            item.selected = selected;
            card_row.cards.set_row_data(col, item);
        }
    }

    /// `previous` -- position (et visibilité) de la surbrillance AVANT ce
    /// changement, à effacer ; `self.grid_selected`/`self.grid_mouse_active`
    /// donnent la nouvelle carte à surligner. Les deux mutations touchent au
    /// plus 2 `CardItem`, jamais toute la grille.
    fn refresh_grid_selection(&self, previous: Option<(usize, usize)>) {
        let window = self.window();
        let current = self.grid_mouse_active.get().then(|| self.grid_selected.get());
        if previous != current {
            Self::set_card_highlight(&window, previous, false);
            Self::set_card_highlight(&window, current, true);
        }
        let displayed = self.displayed_installed.borrow();
        // Garde la ligne sélectionnée visible (voir le `changed
        // selected-row-local` sur card-list dans card-grid.slint) -- -1 si
        // la bibliothèque est vide.
        window.set_grid_selected_row(if displayed.is_empty() { -1 } else { self.grid_selected.get().0 as i32 });
    }

    fn move_grid_selection(&self, dx: i32, dy: i32) {
        let len = self.displayed_installed.borrow().len();
        let columns = self.grid_columns.get().max(1);
        // Arithmétique dans core::grid::next_grid_position -- stress-testée
        // là-bas, indépendamment de toute fenêtre réelle.
        let Some(next) = core::grid::next_grid_position(self.grid_selected.get(), dx, dy, columns, len) else {
            return;
        };
        if next != self.grid_selected.get() {
            let previous = self.grid_mouse_active.get().then(|| self.grid_selected.get());
            self.grid_selected.set(next);
            // Ré-affiche la surbrillance si la souris l'avait effacée en
            // sortant de la grille (voir grid_mouse_active).
            self.grid_mouse_active.set(true);
            self.refresh_grid_selection(previous);
            self.trigger_scroll();
        }
    }

    /// Recharge la vue actuellement affichée (liste OU grille selon
    /// big-mode) après tout install/uninstall/vérif de MAJ. La vue MASQUÉE
    /// reste périmée jusqu'au prochain basculement, qui la reconstruit
    /// intégralement (voir toggle_fullscreen).
    fn refresh_current_view(&self) {
        if self.window().get_big_mode() {
            self.rebuild_grid(true);
        } else {
            let query = self.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    /// Relit `window.scale_factor()` (lecture stable mise en cache par
    /// Slint, contrairement aux appels Win32 bruts à l'origine du bug de
    /// jitter historique -- rien à voir ici), la pousse à `Theme` et
    /// `self.scale`, et calcule la géométrie du mode demandé -- partagé par
    /// `toggle_fullscreen` et `recompute_normal_mode`, seuls points qui
    /// recalculent une géométrie suite à une action utilisateur.
    fn compute_live_mode(&self, big_mode: bool) -> ModeGeometry {
        let window = self.window();
        let scale = window.window().scale_factor();
        window.global::<Theme>().set_scale_factor(scale);
        self.scale.set(scale);
        compute_mode_geometry(
            &self.font_family,
            windows_chrome::work_area_under_cursor(),
            scale,
            big_mode,
            self.window_width_fraction.get(),
            self.border_width.get(),
        )
    }

    /// Bascule entre les deux géométries de mode. Les DEUX sont recalculées
    /// à chaque bascule (voir `compute_live_mode`) : sans ça, un changement
    /// d'échelle Windows survenu pendant que l'appli tourne ne serait capté
    /// que par le mode qu'on est en train d'ACTIVER, laissant l'autre
    /// géométrie périmée -- désaccord fenêtre/contenu à la bascule suivante
    /// (petite fenêtre, contenu à l'ancienne échelle, ou l'inverse).
    fn toggle_fullscreen(&self) {
        let now_big = !self.state.borrow().fullscreen;
        self.state.borrow_mut().set_fullscreen(now_big);
        let mode = self.compute_live_mode(now_big);
        let window = self.window();
        if now_big {
            *self.fullscreen_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.fullscreen_mode.borrow());
        } else {
            *self.normal_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.normal_mode.borrow());
        }
        window.set_big_mode(now_big);
        if now_big {
            self.enter_fullscreen();
        } else {
            let query = self.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    /// Recalcule `normal_mode` à partir des `window_width_fraction`/
    /// `border_width` courants et la réapplique (voir `compute_live_mode`).
    fn recompute_normal_mode(&self) {
        *self.normal_mode.borrow_mut() = self.compute_live_mode(false);
        // Rien à réappliquer en plein écran : cette géométrie n'est affichée
        // qu'à la sortie, et toggle_fullscreen la recalculera à nouveau à ce
        // moment-là de toute façon.
        if !self.window().get_big_mode() {
            let window = self.window();
            apply_mode_geometry(&window, &self.normal_mode.borrow());
        }
    }

    /// Ctrl+1..9/0 (voir app-window.slint) : passe la taille de fenêtre à
    /// `percent` (10..100) et la persiste immédiatement dans themes.json,
    /// comme `commit_theme` pour le sélecteur de thème. Écriture
    /// best-effort : l'affichage a déjà changé, un themes.json en lecture
    /// seule ne doit pas faire échouer l'action visible.
    fn set_window_size_percent(&self, percent: i32) {
        let new_fraction = (percent as f64 / 100.0).clamp(0.05, 1.0);
        // Sans ce garde-fou, marteler Ctrl+1 (déjà à 10%) ou Ctrl+0 (déjà à
        // 100%) relance à chaque frappe tout le cycle recalcul de
        // géométrie/repositionnement/écriture disque pour rien.
        if new_fraction == self.window_width_fraction.get() {
            return;
        }
        self.window_width_fraction.set(new_fraction);
        self.recompute_normal_mode();
        let _ = ui::theme::commit_window_size(&self.themes_path, percent);
    }

    /// Ctrl+-/Ctrl+= (voir app-window.slint) : ajuste l'épaisseur de bordure
    /// de `delta` px et la persiste tout de suite, même principe que
    /// `set_window_size_percent` -- y compris le même garde-fou contre un
    /// no-op (bordure déjà à 0 ou déjà à sa borne haute).
    fn adjust_border(&self, delta: i32) {
        let new_border = (self.border_width.get() + delta).clamp(0, 100);
        if new_border == self.border_width.get() {
            return;
        }
        self.border_width.set(new_border);
        // recompute_normal_mode ne pousse que géométrie et polices ;
        // l'épaisseur visuelle de la bordure vient de Theme.border-width
        // (voir border-px dans app-window.slint), un global que seul
        // apply_theme pousse au démarrage. Sans ce push, il faudrait
        // redémarrer pour voir la nouvelle valeur prendre effet.
        self.window().global::<Theme>().set_border_width(new_border);
        self.recompute_normal_mode();
        let _ = ui::theme::commit_border(&self.themes_path, new_border);
    }
}

fn open_path_if_exists(path: &Path) {
    if path.exists() {
        core::launch::open_path(path);
    }
}

fn centered_position(app: &AppState, dialog_w: i32, dialog_h: i32) -> (i32, i32) {
    let window = app.window();
    let pos = window.window().position();
    let size = window.window().size();
    ui::dialog_geometry::center_over_parent(pos.x, pos.y, size.width as i32, size.height as i32, dialog_w, dialog_h)
}

/// Referme le dialogue affiché (s'il y en a un) et dépile sa cible manette
/// -- unique point de sortie, que ce soit un clic sur ×, la touche B, ou
/// l'ouverture d'un NOUVEAU dialogue (chaque `open_*_dialog` appelle ceci en
/// premier : jamais deux dialogues modaux à la fois).
fn close_current_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let slot = app.dialogs.replace(DialogSlot::None);
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
        DialogSlot::Settings(d) => {
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

fn push_dialog_target(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    router.borrow_mut().push_target(Rc::new(DialogGamepadTarget { app: app.clone() }));
}

/// Rattrape les fermetures qui ne passent pas par nos propres callbacks :
/// Alt+F4 ou "Fermer la fenêtre" depuis la barre des tâches envoient un
/// WM_CLOSE natif que Slint traite seul (il masque la fenêtre) sans jamais
/// invoquer notre callback `close-requested`. Sans ce hook, `app.dialogs`
/// resterait bloqué sur ce dialogue et le `set_window_enabled(hwnd, false)`
/// de finish_dialog_open ne serait jamais annulé -- la fenêtre
/// principale resterait désactivée pour de bon. `Window::on_close_requested`
/// est le seul hook qui capte aussi ces fermetures natives.
fn wire_close_requested_cleanup(window: &slint::Window, app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let app = app.clone();
    let router = router.clone();
    window.on_close_requested(move || {
        close_current_dialog(&app, &router);
        slint::CloseRequestResponse::HideWindow
    });
}

/// Fenêtre Slint native du dialogue dans `slot`, ou `None` si aucun.
fn dialog_window(slot: &DialogSlot) -> Option<&slint::Window> {
    match slot {
        DialogSlot::None => None,
        DialogSlot::Message(d) => Some(d.window()),
        DialogSlot::Confirm(d) => Some(d.window()),
        DialogSlot::Error(d) => Some(d.window()),
        DialogSlot::Info(d) => Some(d.window()),
        DialogSlot::Progress(d) => Some(d.window()),
        DialogSlot::Picker(d) => Some(d.window()),
        DialogSlot::Settings(d) => Some(d.window()),
    }
}

/// Séquence commune à la fin de chaque `open_*_dialog`, une fois le
/// `dialog.show()` fait par l'appelant : rend la fenêtre principale
/// véritablement modale (voir windows_chrome::set_window_enabled, réactivée dans
/// close_current_dialog), enregistre le dialogue comme actif, pousse sa
/// cible manette, puis lui applique icône/possession/premier plan.
fn finish_dialog_open(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, slot: DialogSlot) {
    if let Some(w) = dialog_window(&slot) {
        wire_close_requested_cleanup(w, app, router);
    }
    if let Some(hwnd) = windows_chrome::native_hwnd(app.window().window()) {
        windows_chrome::set_window_enabled(hwnd, false);
    }
    *app.dialogs.borrow_mut() = slot;
    push_dialog_target(app, router);

    // DIFFÉRÉ : `windows_chrome::native_hwnd()` renvoie None juste après l'ouverture du
    // dialogue, la fenêtre native n'étant pas encore complètement associée
    // (même symptôme que la fenêtre principale au premier show(), voir
    // main()). Le Timer relit `app.dialogs` au moment où il se déclenche
    // plutôt que de capturer un hwnd indisponible -- si le dialogue a été
    // refermé entre-temps, dialog_window renvoie None et il n'y a rien à
    // faire.
    let app2 = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        let slot_ref = app2.dialogs.borrow();
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

/// Cible manette générique pour n'importe quel dialogue ouvert -- relit
/// `app.dialogs` à chaque appel plutôt que de garder un type par dialogue :
/// un seul dialogue est ouvert à la fois (voir DialogSlot). Les actions
/// passent par les MÊMES callbacks Slint que la souris
/// (`invoke_xxx_requested`), pour n'avoir qu'un chemin de code par action.
struct DialogGamepadTarget {
    app: Rc<AppState>,
}

/// Clone fort (voir `ComponentHandle::clone_strong`) du dialogue dans
/// `app.dialogs`, ou `None` s'il n'y en a pas ou si c'est `Progress` (jamais
/// fermable/activable au clavier-manette). Le clone DOIT sortir du scope du
/// `.borrow()` avant que l'appelant invoque quoi que ce soit dessus -- voir
/// `activate_selection`.
fn cloned_dialog(app: &AppState) -> Option<DialogSlot> {
    match &*app.dialogs.borrow() {
        DialogSlot::Message(d) => Some(DialogSlot::Message(d.clone_strong())),
        DialogSlot::Confirm(d) => Some(DialogSlot::Confirm(d.clone_strong())),
        DialogSlot::Error(d) => Some(DialogSlot::Error(d.clone_strong())),
        DialogSlot::Picker(d) => Some(DialogSlot::Picker(d.clone_strong())),
        DialogSlot::Info(d) => Some(DialogSlot::Info(d.clone_strong())),
        DialogSlot::Settings(d) => Some(DialogSlot::Settings(d.clone_strong())),
        DialogSlot::Progress(_) | DialogSlot::None => None,
    }
}

impl GamepadTarget for DialogGamepadTarget {
    // Passe par `invoke_close_requested()` plutôt que d'appeler
    // `close_current_dialog` directement : SettingsDialog a sa propre
    // logique dans `close-requested` (annuler l'effet visuel d'une
    // prévisualisation de thème non confirmée, voir `open_settings_dialog`),
    // et B doit suivre exactement le même chemin que la croix/Échap. Même
    // clone-avant-invoke que `activate_selection`, même raison.
    fn reject(&self) {
        // ProgressDialog n'est pas fermable (interrompre un install
        // laisserait le port bloqué "en cours d'installation") -- B n'y fait
        // rien, comme son bouton fermer désactivé.
        match cloned_dialog(&self.app) {
            Some(DialogSlot::Message(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Confirm(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Error(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Picker(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Info(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Settings(d)) => d.invoke_close_requested(),
            _ => {}
        }
    }

    // Le clone (léger -- même poignée Rc sous-jacente) DOIT sortir du scope
    // du `.borrow()` AVANT d'appeler `invoke_xxx` : ce callback rejoue la
    // logique du clic souris, qui referme le dialogue via
    // `app.dialogs.borrow_mut()`. Un `match &*self.app.dialogs.borrow() {
    // ... => d.invoke_xxx() }` garderait l'emprunt actif pendant tout le
    // bras de match et paniquerait ("already borrowed").
    fn activate_selection(&self) {
        match cloned_dialog(&self.app) {
            // Pas de bouton OK (voir dialogs.slint) -- A/Entrée ferme
            // directement, comme la croix/Échap.
            Some(DialogSlot::Message(d)) => d.invoke_close_requested(),
            // Selon le bouton en surbrillance (voir move_selection) --
            // jamais "confirmer" par défaut pour une action destructive.
            Some(DialogSlot::Confirm(d)) => {
                if self.app.confirm_nav_index.get() == 0 {
                    d.invoke_confirmed();
                } else {
                    d.invoke_close_requested();
                }
            }
            // Selon le bouton en surbrillance (voir move_selection) --
            // jamais "réinstaller" par défaut, même logique que Confirm.
            Some(DialogSlot::Error(d)) => {
                if self.app.error_nav_index.get() == 0 {
                    d.invoke_reinstall_requested();
                } else {
                    d.invoke_info_requested();
                }
            }
            Some(DialogSlot::Picker(d)) => d.invoke_item_selected(self.app.picker_index.get()),
            // footer_nav_index (voir son commentaire dans AppState) arbitre
            // qui est ciblé : -1 choisit le thème en surbrillance, 0..4
            // déclenche le raccourci correspondant.
            Some(DialogSlot::Settings(d)) => match self.app.footer_nav_index.get() {
                0 => d.invoke_library_requested(),
                1 => d.invoke_ports_json_requested(),
                2 => d.invoke_ports_local_json_requested(),
                3 => d.invoke_state_json_requested(),
                4 => d.invoke_themes_json_requested(),
                _ => d.invoke_item_selected(self.app.picker_index.get()),
            },
            // Rien ne se passe si le bouton en surbrillance est désactivé
            // (ex: pas de site web renseigné pour ce port).
            Some(DialogSlot::Info(d)) if info_nav_enabled(&d)[self.app.info_nav_index.get() as usize] => {
                match self.app.info_nav_index.get() {
                    0 => d.invoke_website_requested(),
                    1 => d.invoke_mods_requested(),
                    2 => d.invoke_game_folder_requested(),
                    3 => d.invoke_save_folder_requested(),
                    4 => d.invoke_save_folder2_requested(),
                    5 => d.invoke_change_version_requested(),
                    _ => d.invoke_favorite_exe_requested(),
                }
            }
            _ => {}
        }
    }

    fn show_info_for_selection(&self) {
        let error_dialog = match &*self.app.dialogs.borrow() {
            DialogSlot::Error(d) => Some(d.clone_strong()),
            _ => None,
        };
        if let Some(d) = error_dialog {
            d.invoke_info_requested();
        }
    }

    fn move_selection(&self, dx: i32, dy: i32) {
        match &*self.app.dialogs.borrow() {
            // Un seul axe (dy) -- Confirmer/Annuler empilés verticalement,
            // voir ConfirmDialog dans dialogs.slint.
            DialogSlot::Confirm(d) => {
                if dy != 0 {
                    let next = (self.app.confirm_nav_index.get() + dy).clamp(0, 1);
                    self.app.confirm_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
            // Même pattern -- Réinstaller/Infos empilés verticalement, voir
            // ErrorDialog dans dialogs.slint.
            DialogSlot::Error(d) => {
                if dy != 0 {
                    let next = (self.app.error_nav_index.get() + dy).clamp(0, 1);
                    self.app.error_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
            DialogSlot::Picker(d) => {
                let count = d.get_items().row_count() as i32;
                if count == 0 {
                    return;
                }
                let next = (self.app.picker_index.get() + dy).clamp(0, count - 1);
                self.app.picker_index.set(next);
                d.set_selected_index(next);
                d.set_scroll_trigger(!d.get_scroll_trigger());
            }
            // Deux axes INDÉPENDANTS : Haut/Bas défile le texte
            // d'instructions (voir scroll-instructions dans dialogs.slint,
            // seul texte qui peut dépasser la hauteur du dialogue),
            // Gauche/Droite navigue la rangée horizontale de boutons,
            // restreinte aux boutons ACTIVÉS (un bouton désactivé -- pas de
            // dossier de sauvegarde connu, par ex. -- ne doit jamais
            // recevoir la sélection).
            DialogSlot::Info(d) => {
                if dy != 0 {
                    d.invoke_scroll_instructions(dy);
                }
                if dx != 0 {
                    let enabled = info_nav_enabled(d);
                    let candidates: Vec<i32> = (0..7).filter(|&i| enabled[i as usize]).collect();
                    if !candidates.is_empty() {
                        let current = self.app.info_nav_index.get();
                        let pos = candidates.iter().position(|&i| i == current).unwrap_or(0) as i32;
                        let next_pos = (pos + dx).clamp(0, candidates.len() as i32 - 1);
                        let next = candidates[next_pos as usize];
                        self.app.info_nav_index.set(next);
                        d.set_selected_index(next);
                    }
                }
            }
            // Deux axes INDÉPENDANTS, même principe que Info. Haut/Bas
            // navigue la liste de thèmes et REND le focus à la liste s'il
            // était sur la rangée de raccourcis (footer_nav_index à -1) ;
            // Gauche/Droite navigue la rangée de raccourcis sans toucher à
            // la sélection de thème, restreinte aux boutons ACTIVÉS (voir
            // settings_footer_enabled).
            //
            // Le défilement de liste, dont Picker se passe (ses listes
            // réelles restent courtes), est nécessaire ici : themes.json
            // peut dépasser la centaine de thèmes. `invoke_item_hovered`
            // réutilise le callback du survol souris pour que la
            // prévisualisation suive aussi la sélection clavier/manette,
            // sans seconde implémentation à maintenir.
            DialogSlot::Settings(d) => {
                if dy != 0 {
                    let count = d.get_items().row_count() as i32;
                    if count != 0 {
                        let next = (self.app.picker_index.get() + dy).clamp(0, count - 1);
                        self.app.footer_nav_index.set(-1);
                        d.set_footer_selected_index(-1);
                        d.set_scroll_trigger(!d.get_scroll_trigger());
                        d.invoke_item_hovered(next);
                    }
                }
                if dx != 0 {
                    let enabled = settings_footer_enabled(d);
                    let candidates: Vec<i32> = (0..5).filter(|&i| enabled[i as usize]).collect();
                    if !candidates.is_empty() {
                        let current = self.app.footer_nav_index.get();
                        let pos = candidates.iter().position(|&i| i == current).map(|p| p as i32);
                        let next_pos = match pos {
                            Some(p) => (p + dx).clamp(0, candidates.len() as i32 - 1),
                            // Hors focus (-1) -- entre par le bord dans le
                            // sens du mouvement : Droite -> premier bouton,
                            // Gauche -> dernier.
                            None => if dx > 0 { 0 } else { candidates.len() as i32 - 1 },
                        };
                        let next = candidates[next_pos as usize];
                        self.app.footer_nav_index.set(next);
                        d.set_footer_selected_index(next);
                    }
                }
            }
            _ => {}
        }
    }
}

/// [Website, Mods website, Game folder, Save folder, Save folder 2, Change
/// version, Favorite executable] activés -- même ordre que
/// InfoDialog.selected-index (voir dialogs.slint).
fn info_nav_enabled(d: &InfoDialog) -> [bool; 7] {
    [
        d.get_website_enabled(),
        d.get_mods_enabled(),
        d.get_game_folder_enabled(),
        d.get_save_folder_enabled(),
        d.get_save_folder2_enabled(),
        d.get_change_version_enabled(),
        d.get_favorite_exe_enabled(),
    ]
}

/// [Library, ports.json, ports.local.json, state.json, themes.json] activés
/// -- même ordre que SettingsDialog.footer-selected-index (voir
/// dialogs.slint), même principe que info_nav_enabled ci-dessus.
fn settings_footer_enabled(d: &SettingsDialog) -> [bool; 5] {
    [
        d.get_library_enabled(),
        d.get_ports_json_enabled(),
        d.get_ports_local_json_enabled(),
        d.get_state_json_enabled(),
        d.get_themes_json_enabled(),
    ]
}

fn open_message_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, title: &str, message: &str) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    // Hauteur dérivée de la mesure RÉELLE de `message` (voir
    // ui::dialog_geometry::message_dialog_size) -- jamais de texte tronqué.
    let (dw, dh) = ui::dialog_geometry::message_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.border_width.get(), message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = MessageDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title.into());
    dialog.set_message_text(message.into());
    apply_dialog_chrome!(dialog, fonts);
    position_dialog!(dialog, dw, dh, x, y, app.scale.get());
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Message(dialog));
}

/// Recalcule taille et position de `dialog` pour le `status` donné -- à
/// l'ouverture comme à CHAQUE mise à jour de texte en cours d'install (voir
/// AppEvent::InstallProgress). Un `set_status_text` seul laisserait la
/// fenêtre à sa taille initiale et tronquerait un message plus long
/// arrivant ensuite, la mesure de progress_dialog_size n'étant faite qu'ici.
fn resize_progress_dialog(app: &Rc<AppState>, dialog: &ProgressDialog, status: &str) {
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let (dw, dh) = ui::dialog_geometry::progress_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.border_width.get(), status,
    );
    let (x, y) = centered_position(app, dw, dh);
    dialog.set_status_text(status.into());
    position_dialog!(dialog, dw, dh, x, y, app.scale.get());
}

fn open_progress_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, title: &str, status: &str) {
    close_current_dialog(app, router);
    let (fonts, ..) = dialog_context(app);
    let Ok(dialog) = ProgressDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(title.into());
    dialog.set_progress_fill_color(app.semantic.success);
    apply_dialog_chrome!(dialog, fonts);
    resize_progress_dialog(app, &dialog, status);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Progress(dialog));
}

fn open_error_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let message = format!("\"{}\" could not be launched. The game files may be missing or incomplete.", port.name);
    // Hauteur dérivée de la mesure RÉELLE du message, qui contient le nom
    // du port et peut donc être long.
    let (dw, dh) = ui::dialog_geometry::error_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.border_width.get(), &message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ErrorDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(format!("Error - {}", port.name).into());
    dialog.set_message_text(message.into());
    apply_dialog_chrome!(dialog, fonts);
    position_dialog!(dialog, dw, dh, x, y, app.scale.get());
    // Repart toujours sur "Reinstall" en surbrillance, jamais la position
    // laissée par un ErrorDialog précédent.
    app.error_nav_index.set(0);
    dialog.set_selected_index(0);
    wire_dialog_nav_hovered!(dialog, app, error_nav_index);
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

/// Demande confirmation avant `delete_port` -- `error_dialog_size` réutilisée
/// telle quelle : même géométrie (message + deux boutons empilés), rien
/// d'error-spécifique dans son calcul.
fn open_uninstall_confirm_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    close_current_dialog(app, router);
    let (fonts, family, work_w, work_h) = dialog_context(app);
    let message = format!("Uninstall \"{}\"? This deletes its installed files.", port.name);
    let (dw, dh) = ui::dialog_geometry::error_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, fonts.title_bar_height_px, app.border_width.get(), &message,
    );
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = ConfirmDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(format!("Uninstall - {}", port.name).into());
    dialog.set_message_text(message.into());
    dialog.set_confirm_text("Uninstall".into());
    apply_dialog_chrome!(dialog, fonts);
    position_dialog!(dialog, dw, dh, x, y, app.scale.get());
    // Repart toujours sur "Uninstall" en surbrillance, jamais la position
    // laissée par un ConfirmDialog précédent.
    app.confirm_nav_index.set(0);
    dialog.set_selected_index(0);
    wire_dialog_nav_hovered!(dialog, app, confirm_nav_index);
    wire_dialog_selection_nav!(dialog, app, vertical);
    {
        let app2 = app.clone();
        let router2 = router.clone();
        let port2 = port.clone();
        dialog.on_confirmed(move || {
            close_current_dialog(&app2, &router2);
            delete_port(&app2, &router2, &port2);
        });
    }
    wire_dialog_close!(dialog, app, router);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Confirm(dialog));
}

fn open_info_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    close_current_dialog(app, router);
    // Même taille exacte que la fenêtre principale, lue directement sur la
    // fenêtre RÉELLEMENT affichée : un second calcul pourrait diverger de la
    // géométrie déjà appliquée à app-window selon le moniteur sous le
    // curseur.
    let main_size = app.window().window().size();
    let (dw, dh) = (main_size.width as i32, main_size.height as i32);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = InfoDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title(format!("Info - {}", port.name).into());
    // TOUJOURS normal_mode.fonts, jamais fullscreen_mode -- voir
    // apply_dialog_chrome!.
    apply_dialog_chrome!(dialog, app.normal_mode.borrow().fonts);

    // Messages distincts pour deux situations distinctes : une source
    // Local/DirectUrl n'a aucune notion de tag et n'est jamais vérifiée,
    // alors qu'un port GitHub/GitLab au tag inconnu (state.json reconstruit,
    // ou port déposé à la main dans Library/ puis adopté par main()) reste
    // bel et bien vérifié à chaque cycle -- start_update_checks n'exige pas
    // de tag connu et update_decision se replie sur installed_at.
    let version_text = match app.state.borrow().get(port.key()) {
        None => "Version: Not installed".to_string(),
        Some(info) => match (port.source_type, &info.installed_tag) {
            (SourceType::Github | SourceType::Gitlab, Some(tag)) => format!("Version: {tag}"),
            (SourceType::Github, None) => "Version: Installed (tag unknown -- still checked against GitHub)".to_string(),
            (SourceType::Gitlab, None) => "Version: Installed (tag unknown -- still checked against GitLab)".to_string(),
            (SourceType::DirectUrl | SourceType::Local, _) => "Version: Installed (this source has no version tracking)".to_string(),
        },
    };
    dialog.set_version_text(version_text.into());
    dialog.set_instructions_text(port.instructions.clone().into());
    if let Some(link) = port.instructions_link() {
        dialog.set_instructions_link(link.into());
        let link = link.to_string();
        dialog.on_instructions_link_requested(move || core::launch::open_url(&link));
    }

    let website_url = port.website_url().map(str::to_string);
    let website_ok = website_url.as_deref().map(|u| u.starts_with("http://") || u.starts_with("https://")).unwrap_or(false);
    dialog.set_website_enabled(website_ok);

    let mods_ok = port.mods_url.as_deref().map(|u| u.starts_with("http://") || u.starts_with("https://")).unwrap_or(false);
    dialog.set_mods_enabled(mods_ok);

    let game_folder = core::platform_utils::safe_join(&app.library_dir, &port.folder_name).ok();
    let game_folder_ok = game_folder.as_ref().map(|p| p.exists()).unwrap_or(false);
    dialog.set_game_folder_enabled(game_folder_ok);

    // `resolve_save_folder`, pas `expand_env_path` seule : un save_folder
    // relatif (ex: "Save", sans %VARIABLE%) doit se résoudre par rapport au
    // dossier du JEU, jamais au dossier courant du processus.
    let save_path: Option<PathBuf> =
        game_folder.as_deref().and_then(|dir| port.save_folder.as_ref().and_then(|v| core::platform_utils::resolve_save_folder(v, dir)));
    let save_ok = save_path.as_ref().map(|p| p.exists()).unwrap_or(false);
    dialog.set_save_folder_enabled(save_ok);

    let save2_path: Option<PathBuf> =
        game_folder.as_deref().and_then(|dir| port.save_folder2.as_ref().and_then(|v| core::platform_utils::resolve_save_folder(v, dir)));
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

    // Premier bouton ACTIVÉ -- 0 si aucun ne l'est, auquel cas
    // activate_selection reste de toute façon un no-op.
    let first_enabled = [website_ok, mods_ok, game_folder_ok, save_ok, save2_ok, change_version_ok, favorite_exe_ok]
        .iter()
        .position(|&ok| ok)
        .unwrap_or(0);
    app.info_nav_index.set(first_enabled as i32);
    dialog.set_selected_index(first_enabled as i32);

    position_dialog!(dialog, dw, dh, x, y, app.scale.get());

    if website_ok {
        let url = website_url.unwrap();
        dialog.on_website_requested(move || core::launch::open_url(&url));
    }
    if mods_ok {
        let url = port.mods_url.clone().unwrap();
        dialog.on_mods_requested(move || core::launch::open_url(&url));
    }
    if game_folder_ok {
        let folder = game_folder.unwrap();
        dialog.on_game_folder_requested(move || core::launch::open_path(&folder));
    }
    if save_ok {
        let folder = save_path.unwrap();
        dialog.on_save_folder_requested(move || core::launch::open_path(&folder));
    }
    if save2_ok {
        let folder = save2_path.unwrap();
        dialog.on_save_folder2_requested(move || core::launch::open_path(&folder));
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
    wire_dialog_close!(dialog, app, router);
    wire_dialog_nav_hovered!(dialog, app, info_nav_index);
    wire_dialog_selection_nav!(dialog, app, horizontal);
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Info(dialog));
}

/// Dialogue de choix générique -- choix d'asset (install ambigu), de version
/// et d'exécutable (lancement ambigu) ; seuls les libellés et l'action
/// `on_select` changent. `on_select` est appelé APRÈS la fermeture du
/// dialogue, et peut donc rouvrir un autre dialogue (start_install,
/// launch_executable...) sans superposer deux modales.
fn open_picker_dialog(
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
    let item_height = ui::dialog_geometry::list_picker_item_height(work_h, big_mode);
    // Largeur élargie si besoin pour que le libellé le plus long tienne en
    // entier (mesure réelle) -- jamais d'ellipse.
    let (dw, dh) = ui::dialog_geometry::list_picker_dialog_size(
        work_w, work_h, &family, fonts.item_font_px, &labels, big_mode, fonts.title_bar_height_px, app.border_width.get(),
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
    app.picker_index.set(0);
    position_dialog!(dialog, dw, dh, x, y, app.scale.get());
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
            app2.picker_index.set(index);
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

/// Sélecteur de thème + raccourcis fichiers/dossiers -- même squelette que
/// `open_picker_dialog` (SettingsDialog partage la navigation de
/// ListPickerDialog) mais avec un câblage dédié plutôt que son `on_select`
/// générique :
///
/// - navigation (survol souris ou flèches/manette) -> `preview_theme`
///   applique les couleurs EN DIRECT (jamais `active_theme` ni le disque,
///   voir son commentaire) à la fenêtre principale ET à ce dialogue
///   lui-même ;
/// - Entrée/clic sur un thème -> `commit_theme` (écrit dans
///   `themes.json`, définitif) puis ferme ;
/// - croix/Échap/B manette (sans jamais avoir validé) -> rien n'a
///   RÉELLEMENT changé (`active_theme` n'a pas bougé pendant la
///   prévisualisation) -- réapplique juste les couleurs du thème toujours
///   actif pour annuler l'effet visuel de la dernière prévisualisation,
///   puis ferme.
fn open_settings_dialog(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    close_current_dialog(app, router);
    let names = ui::theme::list_theme_names(&app.theme_config.borrow());
    if names.is_empty() {
        return;
    }
    let (fonts, _family, _work_w, _work_h) = dialog_context(app);
    // Même taille exacte que la fenêtre principale, lue sur la fenêtre
    // réellement affichée -- même principe qu'InfoDialog.
    let main_size = app.window().window().size();
    let (dw, dh) = (main_size.width as i32, main_size.height as i32);
    let (x, y) = centered_position(app, dw, dh);
    let Ok(dialog) = SettingsDialog::new() else { return };
    apply_dialog_theme!(dialog, app);
    dialog.set_dialog_title("Settings".into());
    let items: Vec<PickerItem> = names.iter().map(|n| PickerItem { label: n.clone().into() }).collect();
    dialog.set_items(slint::ModelRc::new(slint::VecModel::from(items)));
    apply_dialog_chrome!(dialog, fonts);
    // Pas via apply_dialog_chrome! (partagée par des dialogues sans barre
    // de recherche) -- même valeur EXACTE que search-bar-height-physical
    // sur la fenêtre principale (voir apply_font_sizes), fenêtré et plein
    // écran ayant chacun leur propre formule (voir FontSizes).
    dialog.set_search_bar_height_physical(fonts.search_bar_height_px as f32);
    // Positionné sur le thème actif, pas sur 0.
    let start_index = names.iter().position(|n| *n == app.theme_config.borrow().active_theme).unwrap_or(0) as i32;
    dialog.set_selected_index(start_index);
    app.picker_index.set(start_index);
    // Focus initial sur la liste, jamais sur la rangée de raccourcis (voir
    // footer_nav_index dans AppState).
    app.footer_nav_index.set(-1);
    dialog.set_footer_selected_index(-1);
    dialog.set_library_enabled(app.library_dir.exists());
    dialog.set_ports_json_enabled(app.config_dir.join("ports.json").exists());
    dialog.set_ports_local_json_enabled(app.config_dir.join("ports.local.json").exists());
    dialog.set_state_json_enabled(app.config_dir.join("state.json").exists());
    dialog.set_themes_json_enabled(app.themes_path.exists());
    dialog.set_placeholder_text(app.theme_config.borrow().placeholder_text.clone().into());
    let show_clock = app.theme_config.borrow().show_clock;
    dialog.set_show_clock(show_clock);
    if show_clock {
        dialog.set_clock_text(core::clock::format_now().into());
    }
    position_dialog!(dialog, dw, dh, x, y, app.scale.get());
    // Noms RÉELLEMENT affichés, distincts de `names` (la liste complète,
    // gardée intacte pour la recherche) : une fois filtrés par
    // `on_search_changed`, item-hovered/item-selected doivent indexer CETTE
    // liste, sinon les clics et l'aperçu pointeraient le mauvais thème.
    let displayed_names: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(names.clone()));
    {
        let app2 = app.clone();
        let displayed_names2 = displayed_names.clone();
        let dialog_weak = dialog.as_weak();
        dialog.on_item_hovered(move |index| {
            app2.picker_index.set(index);
            let Some(name) = displayed_names2.borrow().get(index as usize).cloned() else { return };
            ui::theme::preview_theme(&mut app2.theme_config.borrow_mut(), &name);
            apply_theme(&app2.window(), &app2.theme_config.borrow());
            if let Some(d) = dialog_weak.upgrade() {
                d.set_selected_index(index);
                apply_dialog_theme!(d, app2);
            }
        });
    }
    {
        let app2 = app.clone();
        let router2 = router.clone();
        let displayed_names2 = displayed_names.clone();
        dialog.on_item_selected(move |index| {
            if let Some(name) = displayed_names2.borrow().get(index as usize) {
                let _ = ui::theme::commit_theme(&app2.themes_path, name);
                app2.theme_config.borrow_mut().active_theme = name.clone();
            }
            close_current_dialog(&app2, &router2);
        });
    }
    {
        let app2 = app.clone();
        let names2 = names.clone();
        let displayed_names2 = displayed_names.clone();
        let dialog_weak = dialog.as_weak();
        dialog.on_search_changed(move |query| {
            let query = query.to_lowercase();
            let filtered: Vec<String> = names2.iter().filter(|n| n.to_lowercase().contains(&query)).cloned().collect();
            *displayed_names2.borrow_mut() = filtered.clone();
            let Some(d) = dialog_weak.upgrade() else { return };
            let items: Vec<PickerItem> = filtered.iter().map(|n| PickerItem { label: n.clone().into() }).collect();
            d.set_items(slint::ModelRc::new(slint::VecModel::from(items)));
            if filtered.is_empty() {
                d.set_selected_index(-1);
                app2.picker_index.set(-1);
                return;
            }
            // Prévisualise le premier résultat -- sinon l'aperçu resterait
            // sur l'ancien thème jusqu'au prochain déplacement.
            d.invoke_item_hovered(0);
        });
    }
    wire_dialog_selection_nav!(dialog, app, vertical);
    {
        let app2 = app.clone();
        dialog.on_move_footer_selection_requested(move |delta| {
            DialogGamepadTarget { app: app2.clone() }.move_selection(delta, 0);
        });
    }
    {
        let app2 = app.clone();
        let dialog_weak = dialog.as_weak();
        dialog.on_footer_hovered(move |index| {
            app2.footer_nav_index.set(index);
            if let Some(d) = dialog_weak.upgrade() {
                d.set_footer_selected_index(index);
            }
        });
    }
    {
        let app2 = app.clone();
        let router2 = router.clone();
        dialog.on_close_requested(move || {
            let name = app2.theme_config.borrow().active_theme.clone();
            ui::theme::preview_theme(&mut app2.theme_config.borrow_mut(), &name);
            apply_theme(&app2.window(), &app2.theme_config.borrow());
            close_current_dialog(&app2, &router2);
        });
    }
    // Rangée de raccourcis -- revérifie l'existence au clic plutôt que de se
    // fier au `*_enabled` poussé à l'ouverture : le fichier a pu disparaître
    // entre-temps.
    {
        let app2 = app.clone();
        dialog.on_library_requested(move || open_path_if_exists(&app2.library_dir));
    }
    {
        let app2 = app.clone();
        dialog.on_ports_json_requested(move || open_path_if_exists(&app2.config_dir.join("ports.json")));
    }
    {
        let app2 = app.clone();
        dialog.on_ports_local_json_requested(move || open_path_if_exists(&app2.config_dir.join("ports.local.json")));
    }
    {
        let app2 = app.clone();
        dialog.on_state_json_requested(move || open_path_if_exists(&app2.config_dir.join("state.json")));
    }
    {
        let app2 = app.clone();
        dialog.on_themes_json_requested(move || open_path_if_exists(&app2.themes_path));
    }
    let _ = dialog.show();
    finish_dialog_open(app, router, DialogSlot::Settings(dialog));
}

/// Lance une install en tâche de fond -- ignore les activations répétées
/// pendant qu'une install pour CE port tourne déjà (`installing`).
///
/// `asset_override` : fichier choisi manuellement après une erreur
/// `Ambiguous`, contourne l'heuristique automatique pour cette tentative.
/// `release_override` : release choisie via `open_version_picker` (bouton
/// "Change version" dans Info), contourne "toujours la dernière" pour cette
/// tentative -- voir installer::install_port.
fn start_install(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    port: Port,
    asset_override: Option<Value>,
    release_override: Option<Value>,
) {
    let key = port.key().to_string();
    // Ne jamais écraser les fichiers d'un port en cours d'exécution -- vaut
    // pour un premier install comme pour une mise à jour, les deux passant
    // par ici.
    if app.installing.borrow().contains(&key) || is_port_running(app, &key) {
        return;
    }
    app.installing.borrow_mut().insert(key.clone());

    open_progress_dialog(app, router, "Installing", &format!("Installing {}...", port.name));

    if app.stress_test {
        // --visual-stress-test : aucun téléchargement réel -- crée un
        // dossier et un "game.exe" au contenu arbitraire plutôt que de
        // passer par core::jobs::run_install.
        let dest_dir = app.library_dir.join(&port.folder_name);
        let _ = std::fs::create_dir_all(&dest_dir);
        let _ = std::fs::write(dest_dir.join("game.exe"), b"not a real executable -- visual stress test");
        lock(&app.events).push(AppEvent::InstallDone { key, tag: Some("v1.0.0".to_string()) });
        return;
    }

    let (github_token, gitlab_token) = {
        let s = app.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    };
    let library_dir = app.library_dir.clone();
    let cache_dir = app.cache_dir.clone();
    let events = app.events.clone();
    let progress_key = key.clone();

    std::thread::spawn(move || {
        let events_progress = events.clone();
        let mut on_progress = move |message: &str| {
            lock(&events_progress).push(AppEvent::InstallProgress { message: message.to_string() });
        };
        let overrides = core::installer::InstallOverrides { asset: asset_override.as_ref(), release: release_override.as_ref() };
        let outcome = core::jobs::run_install(&port, &library_dir, &cache_dir, github_token.as_deref(), gitlab_token.as_deref(), overrides, &mut on_progress);
        let event = match outcome {
            InstallOutcome::Done { tag } => AppEvent::InstallDone { key: progress_key, tag },
            InstallOutcome::AssetAmbiguous { assets } => AppEvent::InstallAssetAmbiguous { key: progress_key, assets },
            InstallOutcome::Error(message) => AppEvent::InstallError { key: progress_key, message },
        };
        lock(&events).push(event);
    });
}

/// Bouton "Change version" de l'InfoDialog -- récupère en arrière-plan les
/// 3 dernières releases GitHub/GitLab de `port`, puis ouvre un
/// `ListPickerDialog` (voir `AppEvent::VersionsFetched`) pour choisir
/// laquelle installer. Même chemin que le choix d'asset ambigu, avec
/// `release_override` au lieu d'`asset_override`.
fn open_version_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let key = port.key().to_string();
    if app.installing.borrow().contains(&key) || is_port_running(app, &key) {
        return;
    }
    // Réserve la clé comme start_install -- le ProgressDialog ouvert juste
    // après masque déjà le bouton, mais ça protège d'un double-fetch si un
    // futur appelant atteint ce chemin autrement.
    app.installing.borrow_mut().insert(key.clone());
    open_progress_dialog(app, router, "Loading", &format!("Fetching versions for {}...", port.name));

    let (github_token, gitlab_token) = {
        let s = app.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    };
    let events = app.events.clone();
    let repo = port.repo.clone().unwrap_or_default();
    let source_type = port.source_type;

    std::thread::spawn(move || {
        let result = match source_type {
            SourceType::Github => core::github_api::list_releases(&repo, github_token.as_deref(), 3).map_err(|e| e.message().to_string()),
            SourceType::Gitlab => core::gitlab_api::list_releases(&repo, gitlab_token.as_deref(), 3).map_err(|e| e.message().to_string()),
            SourceType::DirectUrl | SourceType::Local => Err("This source has no version history.".to_string()),
        };
        let event = match result {
            Ok(releases) => AppEvent::VersionsFetched { key, releases },
            Err(message) => AppEvent::VersionsFetchError { key, message },
        };
        lock(&events).push(event);
    });
}

/// Bouton "favori" de l'InfoDialog -- scanne les exécutables candidats du
/// dossier du port (même détection que le Play ambigu, voir
/// `autodetect_executable`) et laisse choisir lequel sera lancé directement
/// aux prochains Play, sans repasser par le picker (voir
/// `set_favorite_exe`). Tout est local : pas de thread, pas de réseau.
fn open_favorite_exe_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let Ok(game_dir) = core::platform_utils::safe_join(&app.library_dir, &port.folder_name) else { return };
    if !game_dir.exists() {
        return;
    }
    let candidates: Vec<PathBuf> = match core::platform_utils::autodetect_executable(&game_dir) {
        Ok(single) => vec![single],
        Err(ExecutableSelectionError::Ambiguous(_, candidates)) => candidates,
        Err(ExecutableSelectionError::Message(message)) => {
            open_message_dialog(app, router, "No Executable Found", &message);
            return;
        }
    };
    // "Ask every time" en tête de liste -- efface le favori défini
    // (set_favorite_exe(key, None)) pour revenir au comportement par défaut
    // sans désinstaller/réinstaller.
    let mut labels = vec!["Ask every time (default)".to_string()];
    labels.extend(candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()));
    let key = port.key().to_string();
    // Stocké RELATIF à game_dir, jamais en absolu : déplacer le dossier
    // Ports Launcher (donc library_dir) casserait sinon silencieusement
    // tous les favoris déjà choisis. Rejoint à game_dir au moment du Play
    // (voir launch_flow).
    let game_dir_owned = game_dir.clone();
    open_picker_dialog(app, router, "Choose Favorite Executable", labels, move |app, _router, idx| {
        let exe = if idx == 0 {
            None
        } else {
            candidates.get(idx - 1).and_then(|p| p.strip_prefix(&game_dir_owned).ok()).map(|p| p.to_string_lossy().to_string())
        };
        app.state.borrow_mut().set_favorite_exe(&key, exe);
    });
}

/// Retélécharge la jaquette de `port` si son fichier de cache a disparu --
/// normalement rempli à l'Install/Update, mais il peut avoir été supprimé à
/// la main ou un premier téléchargement avoir échoué silencieusement (voir
/// `cache_image`, best-effort). Déclenché au clic sur Jouer sans jamais
/// bloquer le lancement : la vérification est un `Path::exists()` local et
/// le téléchargement tourne sur un thread séparé.
fn repair_missing_cached_image(app: &Rc<AppState>, port: &Port) {
    let Some(url) = port.image_url.clone() else { return };
    let Ok(dest) = core::image_cache::cached_image_path(&app.cache_dir, &port.folder_name) else { return };
    if dest.exists() {
        return;
    }
    let cache_dir = app.cache_dir.clone();
    let folder_name = port.folder_name.clone();
    let events = app.events.clone();
    std::thread::spawn(move || {
        core::image_cache::cache_image(&url, &cache_dir, &folder_name);
        if core::image_cache::cached_image_path(&cache_dir, &folder_name).map(|p| p.exists()).unwrap_or(false) {
            lock(&events).push(AppEvent::ImageCached { folder_name });
        }
    });
}

fn launch_executable(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port, exe: &Path) {
    // Point d'entrée UNIQUE de tous les chemins de lancement (clic, Entrée,
    // manette, choix d'exécutable ambigu) -- le seul endroit où ce garde-fou
    // contre les activations répétées doit vivre.
    if is_port_running(app, port.key()) {
        return;
    }
    if !exe.exists() {
        open_error_dialog(app, router, port.clone());
        return;
    }
    if app.stress_test {
        // --visual-stress-test : ne tente jamais de lancer le faux
        // "game.exe" créé par start_install -- son contenu arbitraire fait
        // apparaître la boîte système "application 16 bits non prise en
        // charge" (Windows passe par NTVDM avant de renvoyer une erreur),
        // que le driver ne peut pas fermer sans simuler un clic au niveau
        // OS, ce qu'il s'interdit.
        return;
    }
    if let Ok(child) = core::launch::launch(exe) {
        app.running_processes.borrow_mut().insert(port.key().to_string(), child);
        repair_missing_cached_image(app, port);
        // Se minimiser en plein écran : notre fenêtre couvre tout l'écran
        // sans être un mode exclusif OS, et Windows ne redonne pas toujours
        // le premier plan au jeu qui démarre -- il se retrouverait ouvert
        // mais caché derrière nous. `poll_app_events` remonte la fenêtre dès
        // qu'aucun jeu ne tourne plus.
        if app.window().get_big_mode() {
            app.window().window().set_minimized(true);
            app.minimized_for_game.set(true);
        }
    }
}

fn launch_flow(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    let Ok(game_dir) = core::platform_utils::safe_join(&app.library_dir, &port.folder_name) else {
        open_message_dialog(app, router, "Invalid Port", "This port's folder name is invalid.");
        return;
    };
    if !game_dir.exists() {
        open_error_dialog(app, router, port.clone());
        return;
    }
    // Exécutable favori choisi par l'utilisateur (voir
    // open_favorite_exe_picker) -- stocké relatif à game_dir, rejoint ici
    // via safe_join, même garde-fou contre une entrée qui sortirait du
    // dossier que pour le champ "executable" de ports.json. Revalidé sur
    // disque à CHAQUE Play : périmé ou disparu (dossier réinstallé, version
    // changée), on retombe sur la détection normale sans bloquer le Play.
    if let Some(favorite) = app.state.borrow().get(port.key()).and_then(|i| i.favorite_exe.clone()) {
        if let Ok(path) = core::platform_utils::safe_join(&game_dir, &favorite) {
            if path.exists() {
                launch_executable(app, router, port, &path);
                return;
            }
        }
    }
    match core::platform_utils::resolve_executable(port.executable.as_ref(), &game_dir) {
        Ok(exe) => launch_executable(app, router, port, &exe),
        Err(ExecutableSelectionError::Ambiguous(_, candidates)) => {
            let labels: Vec<String> =
                candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()).collect();
            let port2 = port.clone();
            open_picker_dialog(app, router, "Choose an executable", labels, move |app, router, idx| {
                if let Some(exe) = candidates.get(idx) {
                    launch_executable(app, router, &port2, exe);
                }
            });
        }
        // ports.json mal configuré ("executable" introuvable) plutôt qu'une
        // install incomplète -- MessageDialog et non ErrorDialog, dont le
        // bouton "Reinstall" ne réglerait rien ici.
        Err(ExecutableSelectionError::Message(message)) => open_message_dialog(app, router, "Executable Not Found", &message),
    }
}

/// Défense en profondeur : la modalité OS (`EnableWindow`, voir
/// windows_chrome::set_window_enabled) devrait déjà empêcher tout callback de la
/// fenêtre principale d'être atteint pendant qu'un dialogue est ouvert.
/// Sans cette garde, cliquer plusieurs lignes pendant qu'un dialogue est
/// affiché lance des installs concurrents qui se partagent le même
/// `DialogSlot` et écrasent chacun le dialogue de l'autre.
fn dialog_is_open(app: &AppState) -> bool {
    !matches!(*app.dialogs.borrow(), DialogSlot::None)
}

/// Récupère le port à `index` dans la liste fenêtrée affichée et applique
/// `action` dessus -- corps commun des callbacks `on_list_row_*_requested`.
///
/// Le `let` séparé, hors du scrutinee du `if let`, est nécessaire : un
/// emprunt créé DANS le scrutinee reste vivant pendant tout le bloc, donc un
/// `action` qui ré-emprunte `displayed_windowed` (delete_port ->
/// refresh_current_view -> rebuild_windowed, qui fait un `borrow_mut()`)
/// paniquerait sur "already borrowed" -- et un panic dans un callback Slint
/// ferme l'application. Uninstall, synchrone jusqu'au rebuild, est le seul
/// bouton directement exposé.
fn with_indexed_port(app: &Rc<AppState>, index: i32, action: impl FnOnce(Port)) {
    let port = app.displayed_windowed.borrow().get(index as usize).cloned();
    if let Some(port) = port {
        action(port);
    }
}

/// Corps commun à `on_list_row_install_requested` et
/// `on_list_row_update_requested` (même action : lancer un install sans
/// asset imposé -- `start_install` distingue lui-même installation initiale
/// et mise à jour selon l'état déjà connu du port).
fn install_or_update_row(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, index: i32) {
    if dialog_is_open(app) {
        return;
    }
    with_indexed_port(app, index, |port| start_install(app, router, port, None, None));
}

/// Vrai si un process lancé pour `key` tourne ENCORE -- vérifié
/// paresseusement à chaque action concernée (lancement/install/
/// désinstallation) plutôt que par un timer dédié. Nettoie l'entrée au
/// passage : sans ce `try_wait`, un process terminé resterait zombie dans
/// la table.
fn is_port_running(app: &AppState, key: &str) -> bool {
    let mut processes = app.running_processes.borrow_mut();
    let Some(process) = processes.get_mut(key) else { return false };
    if process.is_running() {
        true
    } else {
        processes.remove(key);
        false
    }
}

/// Vrai si AU MOINS UN port lancé tourne encore -- sert uniquement à savoir
/// quand remonter la fenêtre minimisée après un lancement en plein écran
/// (voir poll_app_events). Plusieurs jeux peuvent tourner en même temps,
/// donc la remontée n'a lieu qu'une fois TOUS terminés, jamais dès que l'un
/// d'eux se ferme. Nettoie les entrées terminées, comme is_port_running.
fn any_process_running(app: &AppState) -> bool {
    let mut processes = app.running_processes.borrow_mut();
    processes.retain(|_, process| process.is_running());
    !processes.is_empty()
}

/// Point d'entrée unique de "activer la sélection courante" -- réutilisé par
/// le clavier (Entrée), la souris (clic sur une ligne/carte) et la manette
/// (A/Start).
fn activate_port(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    if dialog_is_open(app) || app.installing.borrow().contains(port.key()) {
        return;
    }
    if core::installer::is_installed(port, &app.library_dir) {
        launch_flow(app, router, port);
    } else {
        start_install(app, router, port.clone(), None, None);
    }
}

fn activate_selection(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if let Some(port) = app.current_selected_port() {
        activate_port(app, router, &port);
    }
}

fn show_info_for_current_selection(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if let Some(port) = app.current_selected_port() {
        open_info_dialog(app, router, &port);
    }
}

/// Maj+Entrée -- ouvre le dossier du port sélectionné dans l'Explorateur
/// plutôt que de le lancer. `game_dir.exists()` tient lieu de vérification
/// d'installation : c'est exactement la définition de
/// `core::installer::is_installed`. Silencieux si rien n'est
/// sélectionné/installé.
fn reveal_selected_folder(app: &Rc<AppState>) {
    let Some(port) = app.current_selected_port() else { return };
    let Ok(game_dir) = core::platform_utils::safe_join(&app.library_dir, &port.folder_name) else { return };
    open_path_if_exists(&game_dir);
}

/// Seul point d'entrée de désinstallation (bouton "×" de
/// app-window.slint) -- ignore un port en cours d'installation, et affiche
/// une erreur si la suppression échoue plutôt que de l'avaler.
fn delete_port(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    // Jeu encore lancé : un `remove_dir_all` sur des fichiers verrouillés
    // par Windows échoue de toute façon, mais potentiellement après avoir
    // déjà supprimé une partie de l'arborescence -- mieux vaut une erreur
    // propre avant toute suppression.
    //
    // Un port LOCAL n'est jamais supprimé ici : app-window.slint route déjà
    // ces ports vers un bouton "Open Folder" séparé, la garde reste en
    // défense en profondeur comme dialog_is_open.
    if dialog_is_open(app) || app.installing.borrow().contains(port.key()) || is_port_running(app, port.key()) || port.source_type == SourceType::Local {
        return;
    }
    match core::installer::uninstall_port(port, &app.library_dir) {
        Ok(()) => {
            app.state.borrow_mut().mark_removed(port.key());
            app.refresh_current_view();
        }
        Err(message) => open_message_dialog(app, router, "Uninstall Error", &message),
    }
}

/// Libellés d'un `ListPickerDialog` extraits d'un champ texte commun à une
/// liste d'objets JSON -- `InstallAssetAmbiguous` ("name") et
/// `VersionsFetched` ("tag_name"), avec le même repli "?".
fn json_field_labels(items: &[Value], field: &str) -> Vec<String> {
    items.iter().map(|v| v.get(field).and_then(Value::as_str).unwrap_or("?").to_string()).collect()
}

/// Vide la file d'évènements d'arrière-plan et applique chacun sur le thread
/// UI (voir `AppEvent`) -- appelée par un `slint::Timer` dans `main()`,
/// qu'une install ou une vérif de MAJ soit en cours ou non : vider une file
/// vide ne coûte rien.
fn poll_app_events(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let drained: Vec<AppEvent> = std::mem::take(&mut *lock(&app.events));
    for event in drained {
        match event {
            AppEvent::InstallProgress { message } => {
                if let DialogSlot::Progress(d) = &*app.dialogs.borrow() {
                    resize_progress_dialog(app, d, &message);
                }
            }
            AppEvent::InstallDone { key, tag } => {
                app.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                // Invalide la jaquette en cache : un (ré)install peut en
                // avoir livré une différente, et refresh_current_view juste
                // en dessous afficherait sinon l'ancienne.
                if let Some(port) = app.catalog.borrow().iter().find(|p| p.key() == key) {
                    app.card_image_cache.borrow_mut().remove(&port.folder_name);
                }
                app.state.borrow_mut().mark_installed(&key, tag);
                // Sans ça, `to_port_items` relirait le `true` laissé par le
                // dernier `UpdateCheckResult` pour cette clé et garderait le
                // bouton Update affiché après une mise à jour réussie,
                // jusqu'au prochain cycle de vérification ou redémarrage.
                app.update_cache.borrow_mut().insert(key.clone(), false);
                app.refresh_current_view();
            }
            AppEvent::InstallAssetAmbiguous { key, assets } => {
                app.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                if let Some(port) = app.catalog.borrow().iter().find(|p| p.key() == key).cloned() {
                    let labels = json_field_labels(&assets, "name");
                    open_picker_dialog(app, router, "Choose a file", labels, move |app, router, idx| {
                        if let Some(chosen) = assets.get(idx) {
                            start_install(app, router, port.clone(), Some(chosen.clone()), None);
                        }
                    });
                }
            }
            AppEvent::VersionsFetched { key, releases } => {
                app.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                if let Some(port) = app.catalog.borrow().iter().find(|p| p.key() == key).cloned() {
                    let labels = json_field_labels(&releases, "tag_name");
                    open_picker_dialog(app, router, "Choose a version", labels, move |app, router, idx| {
                        if let Some(release) = releases.get(idx) {
                            start_install(app, router, port.clone(), None, Some(release.clone()));
                        }
                    });
                }
            }
            AppEvent::VersionsFetchError { key, message } => {
                app.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                open_message_dialog(app, router, "Error", &message);
            }
            AppEvent::InstallError { key, message } => {
                app.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                open_message_dialog(app, router, "Installation Error", &message);
            }
            AppEvent::UpdateCheckResult { key, available } => {
                app.update_cache.borrow_mut().insert(key, available);
                finish_one_update_check(app);
            }
            AppEvent::UpdateCheckError(message) => {
                eprintln!("[update check] {message}");
                finish_one_update_check(app);
            }
            // Seul le libellé change : le bouton ouvre toujours la page
            // GitHub (voir on_github_requested), jamais de remplacement
            // automatique de l'exe en cours d'exécution.
            AppEvent::SelfUpdateAvailable => {
                app.window().set_github_button_text("Update".into());
            }
            AppEvent::ImageCached { folder_name } => {
                app.card_image_cache.borrow_mut().remove(&folder_name);
                app.refresh_current_view();
            }
            AppEvent::RemoteCatalogFetched(ports) => {
                *app.catalog.borrow_mut() = ports;
                // La liste affichée doit refléter le nouveau catalogue tout
                // de suite, pas au prochain changement de recherche/mode.
                app.refresh_current_view();
            }
            AppEvent::PortsCheckDone { etag } => {
                app.state.borrow_mut().mark_catalog_check(etag);
            }
            AppEvent::BringToForeground => {
                let window = app.window();
                // Même geste que la remontée post-jeu plus bas. L'évènement
                // vient du thread accept() de SINGLE_INSTANCE_PORT, sans
                // entrée utilisateur associée à notre thread : un
                // SetForegroundWindow simple serait silencieusement bloqué.
                window.window().set_minimized(false);
                if let Some(hwnd) = windows_chrome::native_hwnd(window.window()) {
                    windows_chrome::force_foreground_window(hwnd);
                }
            }
        }
    }

    // Remonte la fenêtre minimisée par launch_executable dès que TOUS les
    // jeux lancés ont quitté. `get_big_mode()` revérifié ici : si
    // l'utilisateur est repassé en fenêtré entre-temps, la fenêtre n'est
    // plus minimisée et il n'y a rien à forcer.
    if app.minimized_for_game.get() && app.window().get_big_mode() && !any_process_running(app) {
        app.minimized_for_game.set(false);
        let window = app.window();
        window.window().set_minimized(false);
        // `set_minimized(false)` dé-minimise sans garantir de repasser
        // AU-DESSUS des autres fenêtres : le jeu qui vient de quitter a pu
        // laisser une autre fenêtre au premier plan. Un SetForegroundWindow
        // simple ne suffit pas non plus depuis ce Timer d'arrière-plan --
        // voir force_foreground_window.
        if let Some(hwnd) = windows_chrome::native_hwnd(window.window()) {
            windows_chrome::force_foreground_window(hwnd);
        }
    }

    reclaim_dialog_foreground_if_needed(app);
}

/// Corrige un Alt+Tab qui ramène la fenêtre PRINCIPALE devant un dialogue
/// encore ouvert. `own_window` (voir finish_dialog_open) traite le
/// dialogue, pas la fenêtre principale, qui reste un top-level Alt-tabable.
/// Compare spécifiquement à `main_hwnd` et non
/// `foreground_window_belongs_to_us`, trop large : Alt-Tab vers une AUTRE
/// appli pendant un install doit rester possible sans vol de focus.
fn reclaim_dialog_foreground_if_needed(app: &Rc<AppState>) {
    let dialog_hwnd = dialog_window(&app.dialogs.borrow()).and_then(windows_chrome::native_hwnd);
    let (Some(dialog_hwnd), Some(main_hwnd)) = (dialog_hwnd, windows_chrome::native_hwnd(app.window().window())) else { return };
    if windows_chrome::is_foreground_window(main_hwnd) {
        windows_chrome::force_foreground_window(dialog_hwnd);
    }
}

fn finish_one_update_check(app: &Rc<AppState>) {
    let remaining = app.pending_update_checks.get().saturating_sub(1);
    app.pending_update_checks.set(remaining);
    if remaining == 0 {
        app.state.borrow_mut().mark_release_check();
        app.refresh_current_view();
    }
}

/// Pool de threads manuel (spawn + file partagée), plafonné à 8
/// concurrents, pour les vérifications de mise à jour -- sans dépendance
/// supplémentaire. Ne vérifie que les ports GitHub/GitLab installés :
/// direct_url n'a pas de release à comparer (voir
/// core::jobs::run_update_check) et un port non installé n'a rien à mettre
/// à jour.
fn start_update_checks(app: &Rc<AppState>) {
    if !app.state.borrow().should_check_releases() {
        return;
    }
    // Aucun filtre sur un `installed_tag` connu : update_decision (voir
    // github_api.rs) gère un tag inconnu (port adopté automatiquement,
    // déposé à la main, ou state.json reconstruit) en repliant sur
    // `installed_at`, toujours connu.
    let trackable: Vec<(Port, Option<String>, String)> = {
        let state = app.state.borrow();
        app.catalog
            .borrow()
            .iter()
            .filter(|p| matches!(p.source_type, SourceType::Github | SourceType::Gitlab))
            .filter(|p| core::installer::is_installed(p, &app.library_dir))
            .filter_map(|p| {
                let info = state.get(p.key())?;
                Some((p.clone(), info.installed_tag.clone(), info.installed_at.clone()))
            })
            .collect()
    };

    if trackable.is_empty() {
        app.state.borrow_mut().mark_release_check();
        return;
    }
    app.pending_update_checks.set(trackable.len());

    let (github_token, gitlab_token) = {
        let s = app.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    };

    let queue = Arc::new(Mutex::new(VecDeque::from(trackable)));
    let worker_count = lock(&queue).len().min(8);
    for _ in 0..worker_count {
        let queue = queue.clone();
        let events = app.events.clone();
        let github_token = github_token.clone();
        let gitlab_token = gitlab_token.clone();
        std::thread::spawn(move || loop {
            let job = lock(&queue).pop_front();
            let Some((port, tag, installed_at)) = job else { break };
            let key = port.key().to_string();
            let event = match core::jobs::run_update_check(&port, tag.as_deref(), &installed_at, github_token.as_deref(), gitlab_token.as_deref())
            {
                Ok(available) => AppEvent::UpdateCheckResult { key, available },
                Err(message) => AppEvent::UpdateCheckError(message),
            };
            lock(&events).push(event);
        });
    }
}

/// Vérifie si une nouvelle release du launcher LUI-MÊME est disponible. Le
/// launcher n'a pas d'`InstalledInfo` (aucune étape "install") --
/// `core::version::APP_VERSION`, dérivée de la date de compilation par
/// build.rs, sert de référence. `NEUTRAL_INSTALLED_AT` neutralise le repli
/// par date d'`update_decision` : seule la comparaison de tag compte ici,
/// chaque publication du launcher ayant un tag garanti différent.
///
/// Gardé par `should_check_releases()` comme `start_update_checks` (même
/// throttle/quota API partagé, voir son commentaire), mais sans appeler
/// `mark_release_check()` -- `start_update_checks`, toujours démarré en
/// même temps, s'en charge déjà.
fn start_self_update_check(app: &Rc<AppState>) {
    let github_token = {
        let state = app.state.borrow();
        if !state.should_check_releases() {
            return;
        }
        state.github_token.clone()
    };
    let events = app.events.clone();
    std::thread::spawn(move || {
        let result = core::github_api::check_update_available(
            SELF_REPO,
            Some(core::version::APP_VERSION),
            core::version::NEUTRAL_INSTALLED_AT,
            github_token.as_deref(),
        );
        match result {
            Ok((true, _, _)) => lock(&events).push(AppEvent::SelfUpdateAvailable),
            Ok((false, _, _)) => {}
            Err(e) => eprintln!("[self update check] {}", e.message()),
        }
    });
}

/// Rafraîchit `ports.json` depuis GitHub en tâche de fond -- voir
/// `core::catalog_sync` pour le mécanisme (ETag, throttle dédié
/// `should_check_catalog`/`mark_catalog_check`, ENTIÈREMENT séparé du
/// `should_check_releases` des deux fonctions ci-dessus -- pas le même
/// quota, voir CATALOG_CHECK_INTERVAL_HOURS). Démarrée APRÈS
/// `window.show()`, pour ne jamais bloquer le démarrage sur le réseau.
fn start_catalog_sync(app: &Rc<AppState>) {
    let known_etag = {
        let state = app.state.borrow();
        if !state.should_check_catalog() {
            return;
        }
        state.last_catalog_etag.clone()
    };
    let ports_json_path = app.config_dir.join("ports.json");
    let ports_local_json_path = app.config_dir.join("ports.local.json");
    let events = app.events.clone();
    std::thread::spawn(move || match core::catalog_sync::fetch_if_changed(&known_etag) {
        Ok(core::catalog_sync::CatalogUpdate::NotModified) => {
            lock(&events).push(AppEvent::PortsCheckDone { etag: known_etag });
        }
        Ok(core::catalog_sync::CatalogUpdate::Updated { text, etag }) => {
            // Déjà validé par fetch_if_changed -- unwrap_or_default plutôt
            // qu'un panic dans un thread de rafraîchissement.
            let remote_ports = core::config::parse_catalog(&text).unwrap_or_default();
            // Refusionne ports.local.json : sans ça, les ports ajoutés à la
            // main, absents du catalogue distant, disparaîtraient de la vue
            // jusqu'au prochain démarrage. Même fonction qu'au lancement,
            // pas une seconde logique de fusion à maintenir.
            let ports = core::config::merge_local_catalog(remote_ports, core::config::load_local_config(&ports_local_json_path));
            // Écrit aussi sur disque -- le prochain lancement démarre
            // directement sur cette version, sans attendre un nouveau fetch.
            let _ = std::fs::write(&ports_json_path, &text);
            let mut events = lock(&events);
            events.push(AppEvent::PortsCheckDone { etag });
            events.push(AppEvent::RemoteCatalogFetched(ports));
        }
        Err(e) => eprintln!("[catalog sync] {e}"),
    });
}

/// Cible manette de base (voir ui::gamepad_router) -- reste tout en bas de
/// la pile pour toute la durée de l'appli et aiguille vers la liste fenêtrée
/// ou la grille selon le mode courant. Atteinte uniquement quand aucun
/// dialogue n'est ouvert : le routeur ne dispatche qu'au sommet de la pile,
/// et DialogGamepadTarget s'y empile dès qu'un dialogue s'affiche.
struct AppGamepadTarget {
    app: Rc<AppState>,
    router: Rc<RefCell<GamepadRouter>>,
}

impl GamepadTarget for AppGamepadTarget {
    fn move_selection(&self, dx: i32, dy: i32) {
        if self.app.window().get_big_mode() {
            self.app.move_grid_selection(dx, dy);
        } else if dx != 0 {
            // Saut de page (voir PAGE_ROWS) -- même règle que Gauche/Droite
            // au clavier, voir on_move_requested dans main().
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

fn main() {
    // Partagé avec le thread accept() ci-dessous ET avec AppState (voir son
    // champ `events`) -- créé tôt pour que les deux puissent le cloner sans
    // dépendance d'ordre.
    let events: Arc<Mutex<Vec<AppEvent>>> = Arc::new(Mutex::new(Vec::new()));

    // Une autre instance tourne déjà : plutôt qu'une sortie silencieuse
    // (double-clic accidentel, raccourci relancé...), on lui demande de se
    // remettre au premier plan. N'importe quel octet fait office de signal,
    // la première instance n'inspecte jamais ce qu'elle reçoit.
    let Some(single_instance_listener) = acquire_single_instance_lock() else {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
            let _ = stream.write_all(&[0u8]);
        }
        return;
    };
    // Le thread prend possession du listener pour toute la durée du
    // programme : le port reste bindé (verrou) et les connexions entrantes
    // sont réellement écoutées (canal de signal).
    {
        let events = events.clone();
        std::thread::spawn(move || {
            for stream in single_instance_listener.incoming().flatten() {
                drop(stream);
                lock(&events).push(AppEvent::BringToForeground);
            }
        });
    }

    windows_chrome::enable_dpi_awareness();

    // Le module stress_test n'existe qu'en build debug -- en release,
    // `stress_test_iterations` reste structurellement None.
    #[cfg(debug_assertions)]
    let stress_test_iterations = stress_test::parse_stress_test_iterations();
    #[cfg(not(debug_assertions))]
    let stress_test_iterations: Option<u32> = None;

    #[cfg(debug_assertions)]
    let bdir = if stress_test_iterations.is_some() { stress_test::build_stress_sandbox() } else { base_dir() };
    #[cfg(not(debug_assertions))]
    let bdir = base_dir();

    // Doit rester en vie jusqu'à la toute fin de main() -- voir son Drop.
    #[cfg(debug_assertions)]
    let _sandbox_cleanup = stress_test_iterations.is_some().then(|| stress_test::CleanupSandboxOnDrop(bdir.clone()));

    // Amorce ports.json avec le catalogue embarqué au BUILD s'il n'existe
    // pas encore localement (voir core::config::embedded_fallback) : le
    // premier lancement reste amorçable hors-ligne sans le fournir dans la
    // release zippée, start_catalog_sync le rafraîchira ensuite. Écriture
    // best-effort -- si elle échoue (dossier en lecture seule), load_config
    // juste en dessous rapporte l'erreur normalement.
    let ports_json_path = bdir.join("ports.json");
    if !ports_json_path.exists() {
        let _ = std::fs::write(&ports_json_path, core::config::embedded_fallback());
    }
    let catalog = match core::config::load_config(&ports_json_path) {
        Ok(v) => v,
        Err(e) => {
            windows_chrome::show_startup_error(&format!("Couldn't load ports.json: {e}"));
            return;
        }
    };
    // Catalogue local de l'utilisateur -- ses propres ajouts (sans
    // "source", voir SourceType::Local), dans un fichier séparé pour
    // n'être jamais écrasés par une mise à jour de ports.json. Absent, ce
    // n'est pas fatal (voir load_local_config).
    let catalog = core::config::merge_local_catalog(catalog, core::config::load_local_config(&bdir.join("ports.local.json")));

    let mut theme_cfg = ui::theme::ThemeConfig::default();
    ui::theme::load(&bdir.join("themes.json"), &mut theme_cfg);

    let window = match AppWindow::new() {
        Ok(w) => w,
        Err(e) => {
            windows_chrome::show_startup_error(&format!("Failed to create the window: {e}"));
            return;
        }
    };

    apply_theme(&window, &theme_cfg);
    window.set_placeholder_text(theme_cfg.placeholder_text.clone().into());
    window.set_show_clock(theme_cfg.show_clock);

    // Horloge de la barre de recherche -- rafraîchie chaque seconde, jamais
    // créée si "show_clock" est désactivé dans themes.json. Le Timer doit
    // rester en vie jusqu'à la fin de main() : le laisser tomber hors de
    // portée l'arrêterait silencieusement.
    let _clock_timer = if theme_cfg.show_clock {
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

    let library_dir = bdir.join("Library");
    let cache_dir = bdir.join("cache");
    let _ = std::fs::create_dir_all(&library_dir);
    let _ = std::fs::create_dir_all(&cache_dir);

    let state = RefCell::new(core::state::StateManager::load(&bdir.join("state.json")));
    // Adopte les ports déposés à la main dans Library/ sans passer par
    // Install : `is_installed` (vérité disque) les traite déjà comme
    // installés, mais sans entrée dans state.json Info les afficherait "Not
    // installed". `mark_installed(key, None)` les enregistre avec
    // `installed_at` = maintenant et un tag inconnu ; `update_decision`
    // (voir github_api.rs) repère ensuite une vraie mise à jour publiée
    // après cette adoption via `installed_at`.
    {
        let mut s = state.borrow_mut();
        for port in &catalog {
            if s.get(port.key()).is_none() && core::installer::is_installed(port, &library_dir) {
                s.mark_installed(port.key(), None);
            }
        }
    }

    // Zone de travail du moniteur SOUS LE CURSEUR (hors barre des tâches),
    // pas la résolution brute ni forcément l'écran principal -- voir
    // windows_chrome::work_area_under_cursor, valable fenêtré ET
    // plein écran (qui n'est d'ailleurs pas exclusif : la barre des tâches
    // reste visible/utilisable).
    let (area_x, area_y, screen_w, screen_h) = windows_chrome::work_area_under_cursor();

    // Lu avant le premier show() -- voir plus bas pourquoi
    // GetDpiForMonitor plutôt que GetDpiForWindow à ce stade. Sert à la fois
    // à grid_columns et aux géométries figées des deux modes.
    let pre_show_scale = windows_chrome::scale_factor_under_cursor();

    // CARD_WIDTH/CARD_SPACING sont des pixels LOGIQUES (Slint les remet
    // lui-même à l'échelle au rendu), donc comparés à une largeur d'écran
    // elle aussi ramenée en logique : les comparer au `screen_w` physique
    // sous-compterait l'espace réellement occupé par chaque carte et
    // entasserait plus de colonnes qu'il n'en tient.
    //
    // Largeur RÉELLE à 100%, sans marge de sécurité : le bloc de cartes est
    // centré côté .slint (voir row-block-width/row-left-margin dans
    // card-grid.slint), et ce centrage n'est correct que si le nombre de
    // colonnes budgété ici correspond exactement à ce qui est affiché.
    let grid_available_width = screen_w as f32 / pre_show_scale;
    let grid_columns = core::grid::compute_grid_columns(grid_available_width);

    window.set_card_width(core::grid::CARD_WIDTH);
    window.set_card_height(core::grid::CARD_HEIGHT);
    window.set_card_spacing(core::grid::CARD_SPACING);
    window.set_grid_columns(grid_columns as i32);

    let font_family = ui::theme::resolve_font_family(&theme_cfg);
    window.set_font_family(font_family.clone().into());

    let big_mode = state.borrow().fullscreen;
    window.set_big_mode(big_mode);

    // Géométries FIGÉES des deux modes, calculées une seule fois pour toute
    // la session et AVANT le premier show(), avec `pre_show_scale`
    // (`GetDpiForMonitor` sur le moniteur sous le curseur) -- pas
    // `GetDpiForWindow` sur le HWND réel : interrogé juste après `show()`,
    // il donne parfois une lecture encore instable (Windows n'a pas
    // forcément fini d'associer la fenêtre à son moniteur), et les deux
    // géométries hériteraient alors définitivement d'un facteur d'échelle
    // faux. `GetDpiForMonitor` ne dépend d'aucun HWND.
    let area = (area_x, area_y, screen_w, screen_h);
    let normal_mode = compute_mode_geometry(&font_family, area, pre_show_scale, false, theme_cfg.window_width_fraction, theme_cfg.border_width);
    let fullscreen_mode = compute_mode_geometry(&font_family, area, pre_show_scale, true, theme_cfg.window_width_fraction, theme_cfg.border_width);
    apply_mode_geometry(&window, if big_mode { &fullscreen_mode } else { &normal_mode });

    // Montre la fenêtre MAINTENANT plutôt que via window.run() en fin de
    // main(), uniquement pour pouvoir lire ensuite
    // `slint::Window::scale_factor()` de façon fiable : il reste bloqué à
    // 1.0 tant que la fenêtre n'est pas associée à un moniteur, ce qui
    // n'arrive qu'après le retour de main() avec un run() classique.
    // `ComponentHandle::run()` n'est qu'un raccourci pour show() +
    // slint::run_event_loop() + hide().
    window.show().expect("échec de l'affichage de la fenêtre");
    // Icône (voir apply_window_icon) et éligibilité Alt+Tab, DIFFÉRÉES :
    // `windows_chrome::native_hwnd()` renvoie None juste après show(), la fenêtre native
    // n'étant pas encore complètement associée. `single_shot` laisse la
    // boucle d'évènements tourner une fois avant de réessayer.
    {
        let weak = window.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            let Some(window) = weak.upgrade() else { return };
            if let Some(hwnd) = windows_chrome::native_hwnd(window.window()) {
                windows_chrome::apply_window_icon(hwnd);
                windows_chrome::force_alt_tab_visible(hwnd);
            }
        });
    }

    // Facteur d'échelle DPI pour le CONTENU de la fenêtre
    // (Theme.scale-factor). MÊME SOURCE que pre_show_scale, jamais
    // `GetDpiForWindow` : sur un écran à 200%, un désaccord entre les deux
    // lectures ferait rendre tout le contenu deux fois trop grand alors que
    // la taille de fenêtre resterait correcte.
    //
    // Valable uniquement à cet instant. Une fois la fenêtre affichée,
    // `recompute_normal_mode`/`toggle_fullscreen` relisent
    // `window.scale_factor()`, source stable que Slint maintient à jour, ce
    // qui fait suivre un changement d'échelle en cours de session dès le
    // prochain redimensionnement plutôt qu'au redémarrage.
    let scale = windows_chrome::scale_factor_under_cursor();
    window.global::<Theme>().set_scale_factor(scale);

    let app = Rc::new(AppState {
        window: window.as_weak(),
        state,
        catalog: RefCell::new(catalog),
        library_dir: library_dir.clone(),
        cache_dir: cache_dir.clone(),
        config_dir: bdir.clone(),
        grid_columns: Cell::new(grid_columns),
        semantic: theme_cfg.semantic,
        font_family: font_family.clone(),
        border_width: Cell::new(theme_cfg.border_width),
        window_width_fraction: Cell::new(theme_cfg.window_width_fraction),
        // Move de theme_cfg -- doit rester APRÈS toutes les lectures de ses
        // champs ci-dessus : une struct déplacée ne peut plus voir ses
        // champs lus individuellement, même Copy.
        theme_config: RefCell::new(theme_cfg),
        scale: Cell::new(scale),
        themes_path: bdir.join("themes.json"),
        normal_mode: RefCell::new(normal_mode),
        fullscreen_mode: RefCell::new(fullscreen_mode),
        displayed_installed: RefCell::new(Vec::new()),
        grid_selected: Cell::new((0, 0)),
        grid_mouse_active: Cell::new(true),
        last_card_click: Cell::new(None),
        double_click_ms: windows_chrome::double_click_time_ms(),
        displayed_windowed: RefCell::new(Vec::new()),
        windowed_selected: Cell::new(0),
        search_query: RefCell::new(String::new()),
        installing: RefCell::new(HashSet::new()),
        running_processes: RefCell::new(HashMap::new()),
        update_cache: RefCell::new(HashMap::new()),
        pending_update_checks: Cell::new(0),
        events: events.clone(),
        dialogs: RefCell::new(DialogSlot::None),
        picker_index: Cell::new(0),
        info_nav_index: Cell::new(0),
        confirm_nav_index: Cell::new(0),
        error_nav_index: Cell::new(0),
        footer_nav_index: Cell::new(-1),
        minimized_for_game: Cell::new(false),
        card_image_cache: RefCell::new(HashMap::new()),
        stress_test: stress_test_iterations.is_some(),
    });

    // Créé avant le câblage des callbacks ci-dessous : plusieurs d'entre eux
    // (install/lancement/dialogues) en ont besoin pour pousser/dépiler leurs
    // propres cibles manette.
    let router = Rc::new(RefCell::new(GamepadRouter::new()));

    app.rebuild_windowed("");
    if big_mode {
        app.enter_fullscreen();
    }

    {
        let app = app.clone();
        window.on_search_changed(move |query| {
            *app.search_query.borrow_mut() = query.to_string();
            // Filtre la vue affichée, liste OU grille selon le mode courant
            // (même branchement que refresh_current_view) -- un
            // rebuild_windowed inconditionnel laisserait la recherche sans
            // effet en plein écran.
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
            let previous = app.grid_mouse_active.get().then(|| app.grid_selected.get());
            app.grid_selected.set((row as usize, col as usize));
            app.refresh_grid_selection(previous);
            // Un clic isolé ne fait que sélectionner, comme
            // list-row-activated en mode fenêtré (Jouer est un bouton
            // séparé). Lancer exige un vrai double-clic sur la MÊME carte
            // dans le délai Windows configuré (voir double_click_ms), pour
            // qu'un clic accidentel ne démarre pas un jeu.
            let key = (row as usize, col as usize);
            let now = Instant::now();
            if let Some((last_key, last_time)) = app.last_card_click.get() {
                let elapsed_ms = now.duration_since(last_time).as_millis();
                // < 50ms : doublon du MÊME clic physique, card-grid.slint
                // pouvant émettre card-activated deux fois pour un seul clic
                // (`clicked` et son repli bas-niveau). Ignoré entièrement --
                // le compter comme second clic suffirait à déclencher un
                // lancement sur un simple clic.
                if elapsed_ms < 50 {
                    return;
                }
                if last_key == key && elapsed_ms <= app.double_click_ms as u128 {
                    // Consommé : un 3e clic rapide doit repartir d'un
                    // double-clic complet, pas redéclencher seul.
                    app.last_card_click.set(None);
                    activate_selection(&app, &router);
                    return;
                }
            }
            app.last_card_click.set(Some((key, now)));
        });
    }

    // Survol souris -- déplace la sélection sous le curseur, comme la
    // navigation clavier/manette (voir PortWidget.hovered/PortCard.hovered).
    {
        let app = app.clone();
        window.on_list_row_hovered(move |index| {
            app.windowed_selected.set(index as usize);
            app.push_selected_index();
        });
    }
    {
        let app = app.clone();
        window.on_card_hovered(move |row, col| {
            let previous = app.grid_mouse_active.get().then(|| app.grid_selected.get());
            app.grid_selected.set((row as usize, col as usize));
            app.grid_mouse_active.set(true);
            app.refresh_grid_selection(previous);
        });
    }
    // La souris quitte la grille -- efface la surbrillance sans toucher à
    // grid_selected (voir grid_mouse_active) : une carte ne reste
    // surlignée que tant que la souris est réellement dessus.
    {
        let app = app.clone();
        window.on_card_unhovered(move || {
            let previous = app.grid_mouse_active.get().then(|| app.grid_selected.get());
            app.grid_mouse_active.set(false);
            app.refresh_grid_selection(previous);
        });
    }

    {
        let app = app.clone();
        window.on_move_requested(move |dx, dy| {
            if app.window().get_big_mode() {
                app.move_grid_selection(dx, dy);
            } else if dx != 0 {
                // Gauche/Droite : saut de page en liste à une seule colonne
                // (voir PAGE_ROWS), pas un déplacement horizontal qui
                // n'aurait pas de sens ici.
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

    window.on_github_requested(|| core::launch::open_url(GITHUB_URL));
    window.on_discord_requested(|| core::launch::open_url(DISCORD_URL));

    // Clic sur le CORPS d'une ligne -- sélectionne seulement (voir
    // list-row-activated dans app-window.slint) : lancer ou installer passe
    // par les boutons d'action de la ligne.
    {
        let app = app.clone();
        window.on_list_row_activated(move |index| {
            app.windowed_selected.set(index as usize);
            app.push_selected_index();
        });
    }

    // Boutons d'action colorés -- agissent sur LEUR ligne (par index dans la
    // liste affichée), jamais sur "la sélection courante". Gardés par
    // dialog_is_open comme activate_port/delete_port : un clic sur une autre
    // ligne pendant qu'un dialogue est ouvert ne doit rien déclencher (voir
    // dialog_is_open).
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
        window.on_list_row_install_requested(move |index| install_or_update_row(&app, &router, index));
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_update_requested(move |index| install_or_update_row(&app, &router, index));
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
        // Port LOCAL uniquement (voir PortItem.is-local dans
        // app-window.slint) -- jamais de suppression, seulement l'ouverture
        // de son dossier : ses fichiers appartiennent à l'utilisateur.
        window.on_list_row_open_folder_requested(move |index| {
            with_indexed_port(&app, index, |port| {
                if let Ok(dir) = core::platform_utils::safe_join(&app.library_dir, &port.folder_name) {
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

    window.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });

    // Déplacement de la fenêtre à la souris (no-frame, Windows ne le fait
    // pas tout seul -- voir drag-area dans app-window.slint). Recalculer la
    // position à la main sur chaque évènement "moved" de Slint donne un
    // déplacement nettement plus lent que le curseur, chaque évènement
    // traversant la boucle Slint et winit -- voir windows_chrome::begin_window_drag,
    // qui délègue le glissé ENTIER à Windows.
    {
        let app = app.clone();
        window.on_window_drag_requested(move || {
            let Some(hwnd) = windows_chrome::native_hwnd(app.window().window()) else { return };
            windows_chrome::begin_window_drag(hwnd);
        });
    }

    // File d'évènements d'arrière-plan (voir AppEvent) -- toujours actif,
    // contrairement au Timer manette : une install peut être déclenchée sans
    // aucune manette branchée.
    let _event_timer = {
        let app = app.clone();
        let router = router.clone();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
            poll_app_events(&app, &router);
        });
        timer
    };

    start_update_checks(&app);
    start_self_update_check(&app);
    start_catalog_sync(&app);

    // Manette -- un seul routeur pour toute l'appli (voir
    // ui::gamepad_router), la fenêtre principale restant tout en bas de la
    // pile. Jamais démarré si aucune manette n'est branchée ; le Timer doit
    // rester en vie jusqu'à la fin de main(), comme celui de l'horloge.
    let _gamepad_timer = if router.borrow().is_available() {
        router.borrow_mut().push_target(Rc::new(AppGamepadTarget { app: app.clone(), router: router.clone() }));
        let timer = slint::Timer::default();
        let router = router.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(ui::gamepad_router::POLL_INTERVAL_MS), move || {
            // L'emprunt de `router` s'achève à la fin de CETTE instruction :
            // `poll()` renvoie des données sans dispatcher lui-même, donc
            // `dispatch` appelé après peut rouvrir/fermer un dialogue
            // (push_dialog_target/close_current_dialog empruntent aussi
            // `router`) sans chevaucher cet emprunt. Voir GamepadRouter::poll
            // pour le crash que ça évite.
            let result = router.borrow_mut().poll();
            // `poll()` tourne même en arrière-plan pour garder `held_dirs`/
            // `held_buttons` synchronisés avec l'état RÉEL de la manette --
            // sinon reprendre le focus avec un bouton encore enfoncé le
            // lirait comme un appui neuf. Seul le DISPATCH est sauté hors
            // focus (voir foreground_window_belongs_to_us).
            if let Some(result) = result {
                if windows_chrome::foreground_window_belongs_to_us() {
                    ui::gamepad_router::dispatch(result);
                }
            }
        });
        Some(timer)
    } else {
        None
    };

    // Doit rester en vie jusqu'à la fin de main(), comme les Timers
    // ci-dessus -- None en usage normal, et toujours None en release où le
    // module stress_test n'est pas compilé.
    #[cfg(debug_assertions)]
    let _stress_driver =
        stress_test_iterations.map(|iterations| stress_test::start_visual_stress_driver(app.clone(), router.clone(), iterations));

    // window.show() a déjà été appelé plus haut -- reste la boucle
    // d'évènements, puis hide() en sortie, exactement ce qu'aurait fait
    // run(). Sous --visual-stress-test, le driver appelle lui-même
    // slint::quit_event_loop() après `iterations`.
    slint::run_event_loop().expect("échec de la boucle d'évènements");
    let _ = window.hide();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Stress test des fonctions PURES de dimensionnement de police -- aucun
    // accès disque/réseau/fenêtre, seulement des appels GDI en mémoire.
    // Bornes réalistes (jusqu'à un écran 8K, une bordure large mais
    // plausible) plutôt qu'adversariales : le clamp de ui::theme (voir
    // rejette_des_reglages_extremes_sans_planter) rend les valeurs extrêmes
    // inatteignables depuis ce code.

    #[test]
    fn windowed_font_sizes_reste_coherent_sur_une_large_plage_de_tailles() {
        for window_height_px in (300..=7680).step_by(137) {
            for border_width_px in [0, 1, 3, 10, 50] {
                for rows in [1, VISIBLE_ROWS, 100] {
                    let f = windowed_font_sizes("Segoe UI", window_height_px, border_width_px, rows);
                    assert!(f.row_height_px >= 1, "h={window_height_px} b={border_width_px} rows={rows}: row_height_px={}", f.row_height_px);
                    assert!(
                        f.item_font_px >= MIN_ITEM_FONT_PX,
                        "h={window_height_px} b={border_width_px} rows={rows}: item_font_px={} < plancher {MIN_ITEM_FONT_PX}",
                        f.item_font_px
                    );
                    assert!(f.title_font_px >= 8, "h={window_height_px} b={border_width_px} rows={rows}: title_font_px={}", f.title_font_px);
                    assert_eq!(f.search_bar_height_px, f.row_height_px * SEARCH_BAR_UNITS);
                    assert_eq!(f.title_bar_height_px, f.row_height_px * TITLE_BAR_UNITS);
                    // Régression : sans MIN_UNIT_H_PX, une fenêtre réduite
                    // fait descendre title_bar_height_px sous
                    // title-vertical-padding*2 (20px physiques, voir
                    // app-window.slint), title-button-size devient négative
                    // et les icônes de la barre de titre disparaissent.
                    assert!(
                        f.row_height_px >= MIN_UNIT_H_PX,
                        "h={window_height_px} b={border_width_px} rows={rows}: row_height_px={} < plancher {MIN_UNIT_H_PX}",
                        f.row_height_px
                    );
                    // Ne doit jamais dépasser `rows` lignes pleines
                    // (débordement de la fenêtre) ni tomber à zéro ou en
                    // négatif (au moins une ligne visible).
                    assert!(
                        f.content_body_height_px > 0 && f.content_body_height_px <= f.row_height_px * rows,
                        "h={window_height_px} b={border_width_px} rows={rows}: content_body_height_px={} hors bornes (row_height_px={})",
                        f.content_body_height_px,
                        f.row_height_px
                    );
                }
            }
        }
    }

    #[test]
    fn resolve_font_sizes_reste_coherent_sur_une_large_plage_de_tailles() {
        for base_height_px in (100..=4320).step_by(97) {
            for search_padding_px in [0, 8, 16, 50] {
                for min_item_font_px in [0, MIN_ITEM_FONT_PX, 40] {
                    let f = resolve_font_sizes("Segoe UI", base_height_px, search_padding_px, min_item_font_px);
                    assert!(f.item_font_px >= min_item_font_px.max(1), "base={base_height_px}: item_font_px={} < {min_item_font_px}", f.item_font_px);
                    assert!(f.row_height_px >= 1, "base={base_height_px}: row_height_px={}", f.row_height_px);
                    assert!(f.search_bar_height_px >= 1, "base={base_height_px}: search_bar_height_px={}", f.search_bar_height_px);
                    assert!(f.title_bar_height_px >= 1, "base={base_height_px}: title_bar_height_px={}", f.title_bar_height_px);
                }
            }
        }
    }
}
