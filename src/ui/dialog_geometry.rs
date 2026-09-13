//! Taille et position des fenêtres de dialogue : largeur à fraction d'écran
//! fixe, hauteur dérivée du contenu réel (texte mesuré, nombre d'options)
//! pour ne jamais le couper. En pixels PHYSIQUES comme le reste de
//! `ui::geometry` -- la compensation DPI se fait à l'affichage, côté .slint
//! (voir `Theme.scale-factor`).

/// `dimension * frac`, borné à `[min_v, max_v]`.
fn clamp_to_screen(frac: f64, min_v: i32, max_v: i32, dimension: i32) -> i32 {
    ((dimension as f64 * frac).round() as i32).clamp(min_v, max_v)
}

/// Marge de sécurité sur une hauteur mesurée via GDI. Slint replie le texte
/// avec son propre moteur de shaping (parley/harfrust/skrifa), qui peut
/// couper à des points légèrement différents pour la même police et la même
/// largeur : la mesure GDI est la meilleure approximation disponible, pas
/// une garantie pixel-exacte. 15% évitent une coupure sur ce désaccord.
fn with_wrap_margin(measured_height: i32) -> i32 {
    (measured_height as f64 * 1.15).ceil() as i32
}

/// Élargit `width` si le mot le plus long de `text` n'y tiendrait pas : un
/// segment sans espace ne peut jamais être replié (voir
/// `font_metrics::longest_word_width`) et déborderait du cadre. Le dialogue
/// s'élargit plutôt que de couper, dans la limite de `max_width`.
fn widen_for_longest_word(width: i32, max_width: i32, family: &str, item_font_px: i32, text: &str, gutter: i32, border_width_px: i32) -> i32 {
    let longest_word = super::font_metrics::longest_word_width(family, item_font_px, text);
    let needed = longest_word + gutter * 2 + border_width_px * 2;
    width.max(needed).min(max_width)
}

/// Largeur à fraction fixe (élargie au besoin, voir
/// `widen_for_longest_word`) et hauteur du texte replié à cette largeur --
/// base commune aux trois dialogues texte ci-dessous, qui ne diffèrent que
/// par le chrome fixe ajouté autour. `gutter` est la marge fixe de 12px
/// physiques de app-window.slint/dialogs.slint, pas une fraction de la
/// largeur de ce dialogue : les marges restent cohérentes avec la fenêtre
/// principale.
fn text_dialog_width_and_height(work_w: i32, family: &str, item_font_px: i32, border_width_px: i32, text: &str) -> (i32, i32, i32) {
    let gutter = 12;
    let mut width = clamp_to_screen(0.28, 380, 480, work_w);
    width = widen_for_longest_word(width, clamp_to_screen(0.9, 380, i32::MAX, work_w), family, item_font_px, text, gutter, border_width_px);
    let logical_w = width - border_width_px * 2;
    let text_width = (logical_w - gutter * 2).max(1);
    let text_height = with_wrap_margin(super::font_metrics::wrapped_text_height(family, item_font_px, text, text_width));
    (width, gutter, text_height)
}

/// Hauteur dérivée de la mesure réelle de `message`, pour que le texte ne
/// soit jamais coupé. Reproduit la mise en page de MessageDialog dans
/// dialogs.slint (titre + gutter*2 autour du texte + bordures) afin que la
/// hauteur calculée ici couvre celle dont Slint a besoin -- jamais moins,
/// quitte à dépasser légèrement le strict minimum.
pub fn message_dialog_size(work_w: i32, work_h: i32, family: &str, item_font_px: i32, title_bar_height_px: i32, border_width_px: i32, message: &str) -> (i32, i32) {
    let (width, gutter, text_height) = text_dialog_width_and_height(work_w, family, item_font_px, border_width_px, message);
    let height = title_bar_height_px + gutter * 2 + text_height + border_width_px * 2;
    (width, height.clamp(160, clamp_to_screen(0.85, 160, i32::MAX, work_h)))
}

/// Comme `message_dialog_size`, plus la place fixe des deux boutons
/// (Reinstall/Open Info) sous le texte : `bar-height*2 + gutter*2` de
/// chrome, en plus du titre et de ses propres gutter*2 (ErrorDialog dans
/// dialogs.slint).
pub fn error_dialog_size(work_w: i32, work_h: i32, family: &str, item_font_px: i32, title_bar_height_px: i32, border_width_px: i32, message: &str) -> (i32, i32) {
    let (width, gutter, text_height) = text_dialog_width_and_height(work_w, family, item_font_px, border_width_px, message);
    let buttons_chrome = title_bar_height_px * 2 + gutter * 2;
    let height = title_bar_height_px + gutter * 2 + text_height + buttons_chrome + border_width_px * 2;
    (width, height.clamp(240, clamp_to_screen(0.85, 240, i32::MAX, work_h)))
}

/// Comme `message_dialog_size`, sur le texte de statut -- qui contient un
/// nom de port potentiellement long ("Installing {name}...") -- plus la
/// place de la barre de progression (`gutter*4` de chrome dans
/// dialogs.slint, texte compris).
pub fn progress_dialog_size(work_w: i32, work_h: i32, family: &str, item_font_px: i32, title_bar_height_px: i32, border_width_px: i32, status: &str) -> (i32, i32) {
    let (width, gutter, text_height) = text_dialog_width_and_height(work_w, family, item_font_px, border_width_px, status);
    let height = title_bar_height_px + gutter * 4 + text_height + border_width_px * 2;
    (width, height.clamp(130, clamp_to_screen(0.85, 130, i32::MAX, work_h)))
}

/// Hauteur d'une option de `ListPickerDialog` -- fraction de la hauteur de
/// travail, jamais un pixel fixe. `big_mode` la grossit pour rester lisible
/// en plein écran.
pub fn list_picker_item_height(work_h: i32, big_mode: bool) -> i32 {
    let frac = if big_mode { 0.06 } else { 0.045 };
    clamp_to_screen(frac, 32, 64, work_h)
}

/// Taille de `ListPickerDialog` -- largeur à fraction fixe, élargie pour
/// que le libellé le plus long tienne en entier (voir
/// `font_metrics::max_text_width`) plutôt que d'être coupé par une ellipse.
/// Hauteur dérivée du nombre d'options, plafonnée à 0.65 de la hauteur de
/// travail. `title_bar_height_px` doit être la hauteur réelle de
/// DialogTitleBar, pas `item_height` réutilisé comme approximation : les
/// deux valeurs ne coïncident pas, et réserver le mauvais espace rogne le
/// dernier bouton ou laisse un vide en bas.
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
    let scale = if big_mode { 1.4 } else { 1.0 };
    let mut width = clamp_to_screen(0.3 * scale, 360, (560.0 * scale) as i32, work_w);

    let longest_label_width = super::font_metrics::max_text_width(family, item_font_px, labels.iter().map(String::as_str));
    // Marge fixe, comme les autres dialogues : indépendante de `width`,
    // donc une seule passe suffit à les calculer tous les deux.
    let gutter = 12;
    // 24px = marge horizontale du label dans DialogButton (12px de chaque
    // côté, voir dialogs.slint), + 20px de sécurité pour la même raison que
    // `with_wrap_margin` : une mesure GDI approxime le rendu de Slint.
    let needed_width = longest_label_width + border_width_px * 2 + gutter * 2 + 24 + 20;
    if needed_width > width {
        width = needed_width;
    }
    width = width.min(clamp_to_screen(0.9, 360, i32::MAX, work_w));

    let item_height = list_picker_item_height(work_h, big_mode);
    let spacing = (gutter as f64 * 0.5).round() as i32;
    let item_count = (labels.len().max(1)) as i32;
    // Bordure haut+bas comptée comme dans les trois autres dialogues : sans
    // elle la fenêtre est deux fois border_width_px trop courte pour son
    // contenu, et la dernière option se retrouve rognée.
    let content_height =
        title_bar_height_px + gutter * 2 + item_count * item_height + (item_count - 1) * spacing + border_width_px * 2;
    let max_height = clamp_to_screen(0.65, 300, 820, work_h);

    (width, content_height.min(max_height))
}

/// Coin haut-gauche d'un dialogue centré sur son parent.
pub fn center_over_parent(parent_x: i32, parent_y: i32, parent_w: i32, parent_h: i32, dialog_w: i32, dialog_h: i32) -> (i32, i32) {
    (parent_x + (parent_w - dialog_w) / 2, parent_y + (parent_h - dialog_h) / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_dialog_respecte_la_largeur_minimale_et_grandit_avec_le_texte() {
        let (w, h_short) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, "Short.");
        assert_eq!(w, 380); // 500*0.28=140 < min 380
        assert!(h_short >= 160);

        // Répété pour dépasser franchement le plancher de 160px quelles que
        // soient les métriques GDI de la machine : sous ce plancher, les
        // deux hauteurs seraient clampées à la même valeur et le test ne
        // vérifierait plus rien.
        let long_message = "This is a much longer message that should wrap across several lines once constrained to a narrow dialog width, taking up noticeably more vertical space than a short one-liner. ".repeat(4);
        let (_, h_long) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, &long_message);
        assert!(h_long > h_short, "h_short={h_short} h_long={h_long}");
    }

    #[test]
    fn message_dialog_plafonne_la_largeur() {
        let (w, _) = message_dialog_size(4000, 4000, "Segoe UI", 14, 34, 3, "x");
        assert_eq!(w, 480); // 4000*0.28=1120 > max 480
    }

    // DT_WORDBREAK ne coupe qu'aux espaces : un segment sans espace plus
    // large que le dialogue déborde du cadre au lieu de se replier, le
    // dialogue doit donc s'élargir pour lui (voir widen_for_longest_word).
    #[test]
    fn message_dialog_s_elargit_pour_un_mot_sans_espace_plus_large_que_le_cadre() {
        let (w_normal, _) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, "ligne ligne ligne");
        let long_word_message = "ligne ligne ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne.zip";
        let (w_wide, _) = message_dialog_size(500, 500, "Segoe UI", 14, 34, 3, long_word_message);
        assert!(w_wide > w_normal, "w_normal={w_normal} w_wide={w_wide}");
        // Pas juste "un peu plus grande" : assez large pour contenir le mot.
        let longest_word = super::super::font_metrics::longest_word_width("Segoe UI", 14, long_word_message);
        assert!(w_wide >= longest_word, "w_wide={w_wide} longest_word={longest_word}");
    }

    #[test]
    fn message_dialog_valeur_normale() {
        // Dimensions choisies pour que la fraction tombe dans les bornes :
        // 1500*0.28 = 420, dans [380, 480].
        let (w, h) = message_dialog_size(1500, 800, "Segoe UI", 14, 34, 3, "A normal-length message.");
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
        // Deux épaisseurs de bordure doivent écarter la hauteur d'exactement
        // 2x leur différence, pas de 0 (bordure ignorée).
        let (_, h_thin) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(2), false, 34, 1);
        let (_, h_thick) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(2), false, 34, 5);
        assert_eq!(h_thick - h_thin, (5 - 1) * 2);
    }

    #[test]
    fn list_picker_plafonne_avec_beaucoup_d_options() {
        let (_, h) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(500), false, 34, 3);
        let max_height = clamp_to_screen(0.65, 300, 820, 1080);
        assert_eq!(h, max_height);
    }

    #[test]
    fn list_picker_big_mode_est_plus_large() {
        let (w_normal, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(3), false, 34, 3);
        let (w_big, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &labels(3), true, 34, 3);
        assert!(w_big > w_normal);
    }

    #[test]
    fn list_picker_s_elargit_pour_un_libelle_long_plutot_que_de_le_couper() {
        let (w_short, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &["short.exe".to_string()], false, 34, 3);
        // Répété pour dépasser franchement le plafond de largeur (560px)
        // quelles que soient les métriques GDI de la machine : en dessous,
        // les deux largeurs seraient clampées à la même valeur.
        let long_name = "a_very_long_executable_name_that_would_never_fit_in_the_default_width".repeat(4) + ".exe";
        let (w_long, _) = list_picker_dialog_size(1920, 1080, "Segoe UI", 14, &[long_name], false, 34, 3);
        assert!(w_long > w_short, "w_short={w_short} w_long={w_long}");
    }

    #[test]
    fn centrage_simple() {
        let (x, y) = center_over_parent(100, 200, 800, 600, 400, 300);
        assert_eq!((x, y), (100 + 200, 200 + 150));
    }
}
