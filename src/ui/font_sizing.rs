//! Police et hauteurs dérivées de la fenêtre principale, pour les deux
//! modes (fenêtré et plein écran). Mesure GDI réelle (voir `font_metrics`)
//! poussée telle quelle vers le `.slint` (voir `apply_font_sizes`) plutôt
//! que recalculée en fraction arbitraire à l'intérieur du `.slint`
//! lui-même.

use crate::AppWindow;
use slint::ComponentHandle;

/// Cible du linespace du texte, fraction de la hauteur d'ÉCRAN -- calibrée
/// empiriquement, utilisée uniquement par le mode PLEIN ÉCRAN (voir
/// `resolve_font_sizes` ; le mode fenêtré dérive géométriquement, voir
/// `windowed_font_sizes`).
///
/// Ne pas la réduire pour compenser un problème d'affichage sans d'abord
/// écarter un bug de mise à l'échelle DPI ailleurs dans le pipeline
/// (`Theme.scale-factor` vs le facteur utilisé pour la taille de fenêtre,
/// voir `scale` dans `main()`) : un tel bug rend TOUT le contenu ~2x trop
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
/// DOIT rester synchronisé avec Theme.title-button-padding (shared.slint) --
/// vérifié automatiquement, voir `tests::slint_sync`.
const TITLE_BAR_VERTICAL_PADDING_PX: i32 = 10;
/// Padding vertical de la barre de recherche EN PLEIN ÉCRAN uniquement --
/// côté fenêtré, `search_bar_height_px` est géométrique (`unit_h_px *
/// SEARCH_BAR_UNITS`, voir `windowed_font_sizes`).
const SEARCH_BAR_VERTICAL_PADDING_BIG_PX: i32 = 16;
const TITLE_FONT_RATIO: f32 = 18.0 / 20.0;
/// Même ratio que search-font-px côté app-window.slint ET
/// dialogs/picker.slint (SearchListDialog) -- DOIT rester synchronisé avec
/// ces deux valeurs-là, vérifié automatiquement (voir `tests::slint_sync`).
const SEARCH_FONT_RATIO: f32 = 1.2;
/// Cible physique -- DOIT rester synchronisée avec content-margin dans
/// app-window.slint (`12px / Theme.scale-factor` côté .slint, donc 12
/// physique quel que soit le DPI, même principe que `title_bar_height_px`
/// ci-dessous) : utilisée pour reconstruire EXACTEMENT le même espace
/// disponible que le rendu .slint produira, voir `windowed_font_sizes`.
/// Vérifié automatiquement, voir `tests::slint_sync`.
const CONTENT_MARGIN_PX: i32 = 12;
/// Nombre de lignes de la liste fenêtrée visibles SANS scroll,
/// géométriquement garanti quelle que soit la valeur choisie ici. La
/// fenêtre garde exactement la taille de `compute_window_size_for` (la
/// redimensionner casserait le ratio voulu) : c'est la POLICE de la liste
/// qui s'ajuste pour que ces lignes tiennent -- voir `windowed_font_sizes`.
pub const VISIBLE_ROWS: i32 = 25;
/// Plancher physique de `unit_h_px` (voir `windowed_font_sizes`) -- sans
/// lui, une fenêtre réduite jusqu'à `WINDOW_MIN_WIDTH` fait descendre la
/// hauteur de la barre de titre sous title-vertical-padding*2 (20px, voir
/// app-window.slint), rendant title-button-size NÉGATIVE : les icônes de la
/// barre de titre et les boutons de ligne disparaissent au lieu de
/// rapetisser. Une fois ce plancher atteint, `VISIBLE_ROWS` lignes ne
/// tiennent plus forcément toutes -- voir `content_body_height_px`, qui
/// bascule alors sur un sous-ensemble scrollable.
const MIN_UNIT_H_PX: i32 = 32;

/// Tailles de police/hauteurs dérivées, résolues une fois par mesure GDI
/// réelle (voir `font_metrics`), poussées telles quelles vers le `.slint`
/// (voir `apply_font_sizes`) plutôt que recalculées en fraction arbitraire à
/// l'intérieur du `.slint` lui-même.
#[derive(Clone, Copy)]
pub struct FontSizes {
    pub item_font_px: i32,
    pub title_font_px: i32,
    pub row_height_px: i32,
    pub search_bar_height_px: i32,
    pub title_bar_height_px: i32,
    /// Hauteur RÉELLE de content-body en mode fenêtré (voir
    /// `windowed_font_sizes`) -- sans objet en plein écran, où CardGrid n'a
    /// pas l'invariant "N lignes tiennent pile" et où le .slint ne lit
    /// jamais ce champ (voir content-body dans app-window.slint).
    pub content_body_height_px: i32,
}

/// Dérive titre/recherche/hauteurs à partir d'un `item_font_px` déjà résolu
/// par l'appelant (`resolve_font_sizes` via `ROW_HEIGHT_FRACTION`,
/// `windowed_font_sizes` via la géométrie) -- une seule formule
/// titre/recherche partagée par les deux.
fn font_sizes_from_item_font(family: &str, item_font_px: i32, item_linespace: i32, search_padding_px: i32) -> FontSizes {
    let title_font_px = ((item_font_px as f32) * TITLE_FONT_RATIO).round().max(8.0) as i32;
    let title_linespace = super::font_metrics::linespace_for_size(family, title_font_px);
    // Hauteur de la barre de recherche mesurée sur SA PROPRE police
    // (search-font-px = item-font-px * 1.2 côté .slint), PAS sur celle de la
    // liste -- sinon la barre serait trop petite pour son propre texte.
    let search_font_px = ((item_font_px as f32) * SEARCH_FONT_RATIO).round().max(8.0) as i32;
    let search_linespace = super::font_metrics::linespace_for_size(family, search_font_px);

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
/// (voir `MIN_ITEM_FONT_PX`). Utilisée pour le mode PLEIN ÉCRAN : la grille
/// de cartes n'a pas de "nombre de lignes cible" comme la liste fenêtrée.
pub fn resolve_font_sizes(family: &str, base_height_px: i32, search_padding_px: i32, min_item_font_px: i32) -> FontSizes {
    let target = ((base_height_px as f32) * ROW_HEIGHT_FRACTION).round() as i32;
    let (mut item_font_px, mut item_linespace) = super::font_metrics::solve_font_for_height(family, target);
    if item_font_px < min_item_font_px {
        item_font_px = min_item_font_px;
        item_linespace = super::font_metrics::linespace_for_size(family, item_font_px);
    }
    font_sizes_from_item_font(family, item_font_px, item_linespace, search_padding_px)
}

/// L'espace disponible est divisé en tranches ÉGALES, la barre de recherche
/// valant volontairement le double d'une ligne -- `VISIBLE_ROWS` tranches
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
pub fn windowed_font_sizes(family: &str, window_height_px: i32, border_width_px: i32, rows: i32) -> FontSizes {
    let total_units = rows + SEARCH_BAR_UNITS + TITLE_BAR_UNITS + FOOTER_UNITS;
    // border*2 (haut+bas) + 4×content-margin (padding-top/bottom du
    // VerticalLayout + son spacing entre CHACUNE des 3 rangées barre de
    // recherche/contenu/pied de page, voir app-window.slint) -- tout ce qui
    // n'est PAS une des `total_units` tranches. footer-body consomme une
    // unité PLEINE (row-height, voir sa déclaration dans app-window.slint --
    // délibérément plus grand que action-button-height pour GitHub/Update
    // et Discord), donc aucun terme de compensation supplémentaire n'est
    // nécessaire ici.
    let available_px = (window_height_px - border_width_px * 2 - CONTENT_MARGIN_PX * 4).max(total_units);
    // MIN_UNIT_H_PX plutôt que .max(1) -- voir son commentaire.
    let unit_h_px = (available_px / total_units).max(MIN_UNIT_H_PX);

    // La mesure GDI ne sert qu'à choisir item_font_px -- row_height_px reste
    // unit_h_px, purement géométrique.
    let target_linespace = (unit_h_px - 2 * ROW_VERTICAL_PADDING_PX).max(1);
    let (mut item_font_px, _) = super::font_metrics::solve_font_for_height(family, target_linespace);
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
pub fn apply_font_sizes(window: &AppWindow, f: &FontSizes) {
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
pub struct ModeGeometry {
    /// Pixels LOGIQUES -- poussés tels quels vers `initial-width/height`
    /// (voir leur commentaire dans app-window.slint).
    pub logical_width: f32,
    pub logical_height: f32,
    /// Pixels PHYSIQUES -- poussés tels quels vers `WindowPosition::Physical`.
    pub pos_x: i32,
    pub pos_y: i32,
    pub fonts: FontSizes,
}

/// `scale` = facteur d'échelle DPI fourni par l'appelant (voir
/// `windows_chrome::scale_factor_under_cursor`), jamais remesuré ici : cette
/// fonction produit une géométrie de mode figée.
pub fn compute_mode_geometry(
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
        let (win_width, win_height) = super::geometry::compute_window_size_for(screen_w, screen_h, width_fraction);
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

pub fn apply_mode_geometry(window: &AppWindow, mode: &ModeGeometry) {
    window.set_initial_width(mode.logical_width);
    window.set_initial_height(mode.logical_height);
    window.window().set_position(slint::WindowPosition::Physical(slint::PhysicalPosition { x: mode.pos_x, y: mode.pos_y }));
    apply_font_sizes(window, &mode.fonts);
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

    // Garde-fou : la police de la liste ne doit jamais RÉTRÉCIR quand la
    // fenêtre s'agrandit (monotonie garantie par construction, voir
    // solve_font_for_height). Couvre tout le pipeline largeur -> hauteur ->
    // police, pas seulement solve_font_for_height en isolation.
    #[test]
    fn item_font_px_ne_regresse_jamais_quand_la_fenetre_grandit() {
        for (screen_w, screen_h) in [(1920, 1080), (2560, 1440), (3840, 2160), (1366, 768)] {
            for border_width_px in [1i32, 3] {
                let mut prev: Option<(i32, i32, i32)> = None;
                for pct in 5..=90 {
                    let frac = pct as f64 / 100.0;
                    let (_w, h) = super::super::geometry::compute_window_size_for(screen_w, screen_h, frac);
                    let f = windowed_font_sizes("Segoe UI", h, border_width_px, VISIBLE_ROWS);
                    if let Some((prev_pct, prev_h, prev_font)) = prev {
                        assert!(
                            f.item_font_px >= prev_font,
                            "REGRESSION screen={screen_w}x{screen_h} border={border_width_px}: {prev_pct}% (h={prev_h}) -> font={prev_font}, puis {pct}% (h={h}) -> font={} (plus PETIT alors que la fenêtre est plus GRANDE)",
                            f.item_font_px
                        );
                    }
                    prev = Some((pct, h, f.item_font_px));
                }
            }
        }
    }

    /// Vérification textuelle que les constantes ci-dessus (dupliquées côté
    /// `.slint` car Slint ne peut pas importer une constante Rust) ont
    /// toujours la même valeur littérale que leur source de vérité `.slint`.
    /// Même principe que `slint_layout_lint` : extraction par sous-chaîne,
    /// pas un vrai parseur -- suffisant tant que la syntaxe autour de
    /// l'ancre (`title-button-padding: `, etc.) ne change pas de forme.
    mod slint_sync {
        const SHARED_SLINT: &str = include_str!("../../ui/shared.slint");
        const APP_WINDOW_SLINT: &str = include_str!("../../ui/app-window.slint");
        const PICKER_SLINT: &str = include_str!("../../ui/dialogs/picker.slint");

        /// Extrait le premier nombre (entier ou décimal) qui suit `anchor`
        /// dans `source`. Panique avec un message explicite si `anchor` est
        /// introuvable, pour distinguer clairement "le fichier .slint a
        /// changé de forme" d'un simple échec d'assertion de valeur.
        fn number_after(source: &str, anchor: &str) -> f64 {
            let after = source.split(anchor).nth(1).unwrap_or_else(|| panic!("ancre `{anchor}` introuvable -- le .slint a changé de forme, mettre à jour ce test"));
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            digits.parse().unwrap_or_else(|_| panic!("valeur numérique introuvable juste après `{anchor}`"))
        }

        #[test]
        fn title_bar_vertical_padding_px_synchronise_avec_shared_slint() {
            let slint_value = number_after(SHARED_SLINT, "title-button-padding: ");
            assert_eq!(slint_value, super::super::TITLE_BAR_VERTICAL_PADDING_PX as f64);
        }

        #[test]
        fn content_margin_px_synchronise_avec_app_window_slint() {
            let slint_value = number_after(APP_WINDOW_SLINT, "content-margin: ");
            assert_eq!(slint_value, super::super::CONTENT_MARGIN_PX as f64);
        }

        #[test]
        fn search_font_ratio_synchronise_avec_app_window_et_picker_slint() {
            // Comparaison en f32 (comme SEARCH_FONT_RATIO) : `1.2` parsé en
            // f64 puis élargi depuis un f32 ne retombe pas bit-à-bit sur le
            // même f64 (1.2 n'a pas de représentation binaire exacte).
            let app_window_value = number_after(APP_WINDOW_SLINT, "item-font-px * ") as f32;
            let picker_value = number_after(PICKER_SLINT, "item-font-px * ") as f32;
            assert_eq!(app_window_value, super::super::SEARCH_FONT_RATIO);
            assert_eq!(picker_value, super::super::SEARCH_FONT_RATIO);
        }
    }
}
