
const GUTTER: i32 = 12;
const BIG_MODE_SCALE: f64 = 1.6;
const MAX_SCREEN_FRACTION: f64 = 0.9;

fn mode_scale(big_mode: bool) -> f64 {
    if big_mode { BIG_MODE_SCALE } else { 1.0 }
}

fn scaled(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
}

fn clamp_to_screen(frac: f64, min_v: i32, max_v: i32, dimension: i32) -> i32 {
    ((dimension as f64 * frac).round() as i32).clamp(min_v, max_v)
}

fn clamp_to_screen_scaled(frac: f64, min_v: i32, max_v: i32, dimension: i32, scale: f64) -> i32 {
    let max_v = if max_v == i32::MAX { max_v } else { scaled(max_v, scale) };
    clamp_to_screen(frac * scale, scaled(min_v, scale), max_v, dimension)
}

fn screen_cap(dimension: i32) -> i32 {
    (dimension as f64 * MAX_SCREEN_FRACTION).round() as i32
}

pub fn main_sized_dialog_size(windowed_w: i32, windowed_h: i32, work_w: i32, work_h: i32, big_mode: bool) -> (i32, i32) {
    if !big_mode {
        return (windowed_w, windowed_h);
    }
    (scaled(windowed_w, BIG_MODE_SCALE).min(screen_cap(work_w)), scaled(windowed_h, BIG_MODE_SCALE).min(screen_cap(work_h)))
}

fn with_wrap_margin(measured_height: i32) -> i32 {
    (measured_height as f64 * 1.15).ceil() as i32
}

fn widen_for_longest_word(width: i32, max_width: i32, family: &str, item_font_px: i32, text: &str, border_width_px: i32) -> i32 {
    let needed = super::font_metrics::longest_word_width(family, item_font_px, text) + GUTTER * 2 + border_width_px * 2;
    width.max(needed).min(max_width)
}

fn text_dialog_width_and_height(work_w: i32, family: &str, item_font_px: i32, border_width_px: i32, text: &str, big_mode: bool) -> (i32, i32) {
    let scale = mode_scale(big_mode);
    let base_width = clamp_to_screen_scaled(0.28, 380, 480, work_w, scale).min(screen_cap(work_w));
    let width = widen_for_longest_word(base_width, clamp_to_screen(0.9, 380, i32::MAX, work_w), family, item_font_px, text, border_width_px);
    let text_width = (width - border_width_px * 2 - GUTTER * 2).max(1);
    let text_height = with_wrap_margin(super::font_metrics::wrapped_text_height(family, item_font_px, text, text_width));
    (width, text_height)
}

fn clamp_text_dialog_height(height: i32, min_h: i32, work_h: i32, big_mode: bool) -> i32 {
    let min_h = scaled(min_h, mode_scale(big_mode)).min(screen_cap(work_h));
    height.clamp(min_h, clamp_to_screen(0.85, min_h, i32::MAX, work_h))
}

#[allow(clippy::too_many_arguments)]
pub fn message_dialog_size(
    work_w: i32,
    work_h: i32,
    family: &str,
    item_font_px: i32,
    title_bar_height_px: i32,
    border_width_px: i32,
    message: &str,
    big_mode: bool,
) -> (i32, i32) {
    let (width, text_height) = text_dialog_width_and_height(work_w, family, item_font_px, border_width_px, message, big_mode);
    let height = title_bar_height_px + GUTTER * 2 + text_height + border_width_px * 2;
    (width, clamp_text_dialog_height(height, 160, work_h, big_mode))
}

#[allow(clippy::too_many_arguments)]
pub fn error_dialog_size(
    work_w: i32,
    work_h: i32,
    family: &str,
    item_font_px: i32,
    title_bar_height_px: i32,
    border_width_px: i32,
    message: &str,
    big_mode: bool,
) -> (i32, i32) {
    let (width, text_height) = text_dialog_width_and_height(work_w, family, item_font_px, border_width_px, message, big_mode);
    let buttons = title_bar_height_px * 2 + GUTTER * 2;
    let height = title_bar_height_px + GUTTER * 2 + text_height + buttons + border_width_px * 2;
    (width, clamp_text_dialog_height(height, 240, work_h, big_mode))
}

#[allow(clippy::too_many_arguments)]
pub fn progress_dialog_size(
    work_w: i32,
    work_h: i32,
    family: &str,
    item_font_px: i32,
    title_bar_height_px: i32,
    border_width_px: i32,
    status: &str,
    big_mode: bool,
) -> (i32, i32) {
    let (width, text_height) = text_dialog_width_and_height(work_w, family, item_font_px, border_width_px, status, big_mode);
    let height = title_bar_height_px + GUTTER * 4 + text_height + border_width_px * 2;
    (width, clamp_text_dialog_height(height, 130, work_h, big_mode))
}

pub fn list_picker_item_height(work_h: i32, big_mode: bool) -> i32 {
    clamp_to_screen_scaled(0.045, 32, 64, work_h, mode_scale(big_mode))
}

#[allow(clippy::too_many_arguments)]
pub fn list_picker_dialog_size(
    work_w: i32,
    work_h: i32,
    family: &str,
    item_font_px: i32,
    labels: &[String],
    big_mode: bool,
    title_bar_height_px: i32,
    border_width_px: i32,
) -> (i32, i32) {
    let scale = mode_scale(big_mode);
    let base_width = clamp_to_screen_scaled(0.3, 360, 560, work_w, scale).min(screen_cap(work_w));
    let longest_label_width = super::font_metrics::max_text_width(family, item_font_px, labels.iter().map(String::as_str));
    let needed_width = longest_label_width + border_width_px * 2 + GUTTER * 2 + 24 + 20;
    let width = base_width.max(needed_width).min(clamp_to_screen(0.9, 360, i32::MAX, work_w));

    let item_height = list_picker_item_height(work_h, big_mode);
    let spacing = GUTTER / 2;
    let item_count = labels.len().max(1) as i32;
    let content_height = title_bar_height_px + GUTTER * 2 + item_count * item_height + (item_count - 1) * spacing + border_width_px * 2;
    (width, content_height.min(clamp_to_screen_scaled(0.65, 300, 820, work_h, scale).min(screen_cap(work_h))))
}

pub fn center_over_parent(parent_x: i32, parent_y: i32, parent_w: i32, parent_h: i32, dialog_w: i32, dialog_h: i32) -> (i32, i32) {
    (parent_x + (parent_w - dialog_w) / 2, parent_y + (parent_h - dialog_h) / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_dialog_respecte_la_largeur_minimale_et_grandit_avec_le_texte() {
        let (w, h_short) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, "Short.", false);
        assert_eq!(w, 380);
        assert!(h_short >= 160);

        let long_message = "This is a much longer message that should wrap across several lines once constrained to a narrow dialog width, taking up noticeably more vertical space than a short one-liner. ".repeat(4);
        let (_, h_long) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, &long_message, false);
        assert!(h_long > h_short, "h_short={h_short} h_long={h_long}");
    }

    #[test]
    fn message_dialog_plafonne_la_largeur() {
        let (w, _) = message_dialog_size(4000, 4000, "Segoe UI", 14, 34, 3, "x", false);
        assert_eq!(w, 480);
    }

    #[test]
    fn message_dialog_s_elargit_pour_un_mot_sans_espace() {
        let (w_normal, _) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, "ligne ligne ligne", false);
        let long_word_message = "ligne ligne ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne.zip";
        let (w_wide, _) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, long_word_message, false);
        assert!(w_wide > w_normal, "w_normal={w_normal} w_wide={w_wide}");
        let longest_word = super::super::font_metrics::longest_word_width("Segoe UI", 14, long_word_message);
        assert!(w_wide >= longest_word, "w_wide={w_wide} longest_word={longest_word}");
    }

    #[test]
    fn message_dialog_valeur_normale() {
        let (w, h) = message_dialog_size(1500, 800, "Segoe UI", 14, 34, 3, "A normal-length message.", false);
        assert_eq!(w, (1500.0_f64 * 0.28).round() as i32);
        assert!(h >= 160);
    }

    fn labels(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("item {i}")).collect()
    }

    #[test]
    fn list_picker_grandit_avec_le_nombre_d_options() {
        let (_, h1) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(2), false, 34, 3);
        let (_, h5) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(5), false, 34, 3);
        assert!(h5 > h1);
    }

    #[test]
    fn list_picker_compte_la_bordure_dans_la_hauteur() {
        let (_, h_thin) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(2), false, 34, 1);
        let (_, h_thick) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(2), false, 34, 5);
        assert_eq!(h_thick - h_thin, (5 - 1) * 2);
    }

    #[test]
    fn list_picker_plafonne_avec_beaucoup_d_options() {
        let (_, h) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(500), false, 34, 3);
        assert_eq!(h, clamp_to_screen(0.65, 300, 820, 1080));
    }

    #[test]
    fn list_picker_big_mode_est_plus_large() {
        let (w_normal, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(3), false, 34, 3);
        let (w_big, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(3), true, 34, 3);
        assert!(w_big > w_normal);
    }

    #[test]
    fn list_picker_s_elargit_pour_un_libelle_long() {
        let (w_short, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &["short.exe".to_string()], false, 34, 3);
        let long_name = "a_very_long_executable_name_that_would_never_fit_in_the_default_width".repeat(4) + ".exe";
        let (w_long, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &[long_name], false, 34, 3);
        assert!(w_long > w_short, "w_short={w_short} w_long={w_long}");
    }

    #[test]
    fn plein_ecran_agrandit_tous_les_dialogues_du_meme_facteur() {
        let (w_normal, _) = message_dialog_size(1920, 1080, "Segoe UI", 14, 34, 3, "x", false);
        let (w_big, _) = message_dialog_size(1920, 1080, "Segoe UI", 14, 34, 3, "x", true);
        assert_eq!(w_big, scaled(w_normal, BIG_MODE_SCALE));
        assert_eq!(list_picker_item_height(1080, true), scaled(list_picker_item_height(1080, false), BIG_MODE_SCALE));
        assert_eq!(main_sized_dialog_size(600, 800, 1920, 1080, true), (960, 972));
        assert_eq!(main_sized_dialog_size(600, 800, 1920, 1080, false), (600, 800));
    }

    #[test]
    fn plein_ecran_ne_depasse_jamais_l_ecran() {
        for (work_w, work_h) in [(800, 600), (1366, 768), (1920, 1080), (3840, 2160)] {
            let cap_w = screen_cap(work_w);
            let cap_h = screen_cap(work_h);
            let (w, h) = message_dialog_size(work_w, work_h, "Segoe UI", 20, 40, 3, "x", true);
            assert!(w <= cap_w && h <= cap_h, "{work_w}x{work_h}: message {w}x{h}");
            let (w, h) = list_picker_dialog_size(work_w, work_h, "Segoe UI", 20, &labels(50), true, 40, 3);
            assert!(w <= cap_w && h <= cap_h, "{work_w}x{work_h}: picker {w}x{h}");
            let (w, h) = main_sized_dialog_size(work_w, work_h, work_w, work_h, true);
            assert!(w <= cap_w && h <= cap_h, "{work_w}x{work_h}: info {w}x{h}");
        }
    }

    #[test]
    fn centrage_simple() {
        assert_eq!(center_over_parent(100, 200, 800, 600, 400, 300), (300, 350));
    }
}
