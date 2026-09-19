
use crate::AppWindow;
use slint::ComponentHandle;

const ROW_HEIGHT_FRACTION: f32 = 0.0255;
const MIN_ITEM_FONT_PX: i32 = 11;
const ROW_VERTICAL_PADDING_PX: i32 = 6;
const TITLE_BAR_VERTICAL_PADDING_PX: i32 = 10;
const SEARCH_BAR_VERTICAL_PADDING_BIG_PX: i32 = 16;
const TITLE_FONT_RATIO: f32 = 18.0 / 20.0;
const SEARCH_FONT_RATIO: f32 = 1.2;
const CONTENT_MARGIN_PX: i32 = 12;
pub const VISIBLE_ROWS: i32 = 25;
const MIN_UNIT_H_PX: i32 = 32;

#[derive(Clone, Copy)]
pub struct FontSizes {
    pub item_font_px: i32,
    pub title_font_px: i32,
    pub row_height_px: i32,
    pub search_bar_height_px: i32,
    pub title_bar_height_px: i32,
    pub content_body_height_px: i32,
}

fn title_font_px_for(item_font_px: i32) -> i32 {
    ((item_font_px as f32) * TITLE_FONT_RATIO).round().max(8.0) as i32
}

fn font_sizes_from_item_font(family: &str, item_font_px: i32, item_linespace: i32, search_padding_px: i32) -> FontSizes {
    let title_font_px = title_font_px_for(item_font_px);
    let title_linespace = super::font_metrics::linespace_for_size(family, title_font_px);
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

pub fn resolve_font_sizes(family: &str, base_height_px: i32, search_padding_px: i32, min_item_font_px: i32) -> FontSizes {
    let target = ((base_height_px as f32) * ROW_HEIGHT_FRACTION).round() as i32;
    let (mut item_font_px, mut item_linespace) = super::font_metrics::solve_font_for_height(family, target);
    if item_font_px < min_item_font_px {
        item_font_px = min_item_font_px;
        item_linespace = super::font_metrics::linespace_for_size(family, item_font_px);
    }
    font_sizes_from_item_font(family, item_font_px, item_linespace, search_padding_px)
}

const SEARCH_BAR_UNITS: i32 = 2;
const TITLE_BAR_UNITS: i32 = 1;
const FOOTER_UNITS: i32 = 1;

pub fn windowed_font_sizes(family: &str, window_height_px: i32, border_width_px: i32, rows: i32) -> FontSizes {
    let total_units = rows + SEARCH_BAR_UNITS + TITLE_BAR_UNITS + FOOTER_UNITS;
    let available_px = (window_height_px - border_width_px * 2 - CONTENT_MARGIN_PX * 4).max(total_units);
    let unit_h_px = (available_px / total_units).max(MIN_UNIT_H_PX);

    let target_linespace = (unit_h_px - 2 * ROW_VERTICAL_PADDING_PX).max(1);
    let (mut item_font_px, _) = super::font_metrics::solve_font_for_height(family, target_linespace);
    if item_font_px < MIN_ITEM_FONT_PX {
        item_font_px = MIN_ITEM_FONT_PX;
    }
    let title_font_px = title_font_px_for(item_font_px);

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

pub fn apply_font_sizes(window: &AppWindow, f: &FontSizes) {
    window.set_item_font_px_physical(f.item_font_px as f32);
    window.set_title_font_px_physical(f.title_font_px as f32);
    window.set_row_height_physical(f.row_height_px as f32);
    window.set_search_bar_height_physical(f.search_bar_height_px as f32);
    window.set_title_bar_height_physical(f.title_bar_height_px as f32);
    window.set_content_body_height_physical(f.content_body_height_px as f32);
}

pub struct ModeGeometry {
    pub logical_width: f32,
    pub logical_height: f32,
    pub pos_x: i32,
    pub pos_y: i32,
    pub fonts: FontSizes,
}

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
                    assert!(
                        f.row_height_px >= MIN_UNIT_H_PX,
                        "h={window_height_px} b={border_width_px} rows={rows}: row_height_px={} < plancher {MIN_UNIT_H_PX}",
                        f.row_height_px
                    );
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

    mod slint_sync {
        const SHARED_SLINT: &str = include_str!("../../ui/shared.slint");
        const APP_WINDOW_SLINT: &str = include_str!("../../ui/app-window.slint");
        const PICKER_SLINT: &str = include_str!("../../ui/dialogs/picker.slint");

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
            let app_window_value = number_after(APP_WINDOW_SLINT, "item-font-px * ") as f32;
            let picker_value = number_after(PICKER_SLINT, "item-font-px * ") as f32;
            assert_eq!(app_window_value, super::super::SEARCH_FONT_RATIO);
            assert_eq!(picker_value, super::super::SEARCH_FONT_RATIO);
        }
    }
}
