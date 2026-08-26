//! Mesures de texte par GDI. Il n'existe pas de formule fermée entre une
//! taille de police demandée et le linespace réellement rendu (ça dépend de
//! la police) : on mesure et on ajuste. GDI faute d'API de mesure Slint
//! exposée au code hôte Rust, alors que `windows` est déjà une dépendance.

use windows::core::PCWSTR;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetTextMetricsW, SelectObject, HDC,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CALCRECT, DT_NOPREFIX, DT_SINGLELINE,
    DT_WORDBREAK, FF_DONTCARE, FW_NORMAL, OUT_DEFAULT_PRECIS, TEXTMETRICW,
};

/// Appelle `f` avec un HDC dont la police `family`/`px` est déjà
/// sélectionnée, puis libère police et HDC. `None` si l'un des deux n'a pas
/// pu être créé : chaque appelant replie alors sur sa valeur par défaut,
/// une police invalide venant de `themes.json` ne devant jamais planter.
unsafe fn with_font<T>(family: &str, px: i32, f: impl FnOnce(HDC) -> T) -> Option<T> {
    let hdc = CreateCompatibleDC(None);
    if hdc.is_invalid() {
        return None;
    }
    let wide: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let hfont = CreateFontW(
        -px,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
        PCWSTR(wide.as_ptr()),
    );
    if hfont.is_invalid() {
        let _ = DeleteDC(hdc);
        return None;
    }
    let old = SelectObject(hdc, hfont.into());
    let result = f(hdc);
    SelectObject(hdc, old);
    let _ = DeleteObject(hfont.into());
    let _ = DeleteDC(hdc);
    Some(result)
}

/// Linespace (`tmHeight`) rendu par `family` à la taille `px`, en pixels
/// physiques comme toute la géométrie poussée depuis Rust. Repli sur `px`
/// si la police ne peut pas être créée ou mesurée.
pub fn linespace_for_size(family: &str, px: i32) -> i32 {
    unsafe {
        with_font(family, px, |hdc| {
            let mut tm = TEXTMETRICW::default();
            let ok = GetTextMetricsW(hdc, &mut tm).as_bool();
            if ok && tm.tmHeight > 0 {
                tm.tmHeight
            } else {
                px
            }
        })
        .unwrap_or(px)
    }
}

/// Cherche la plus GRANDE taille de police (px) dont le linespace mesuré ne
/// dépasse pas `target_linespace` -- une définition purement fonction de
/// `target_linespace`, donc non-décroissante en `target_linespace` tant que
/// `linespace_for_size` l'est elle-même en taille (vrai pour toute police
/// réelle : demander une taille plus grande ne rend jamais une ligne plus
/// courte). Cette garantie de monotonie est nécessaire : la fenêtre
/// s'agrandissant ne doit jamais faire apparaître une police plus petite
/// (voir le test de régression
/// `ui::font_sizing::tests::item_font_px_ne_regresse_jamais_quand_la_fenetre_grandit`).
/// Une estimation proportionnelle sert de point de départ pour limiter le
/// nombre de mesures GDI, mais seule la marche qui suit (jamais bornée à un
/// nombre fixe de pas) décide du résultat. Retourne `(taille_px, linespace
/// mesuré à cette taille)` ; le second sert à dimensionner les lignes/
/// barres qui doivent contenir ce texte.
pub fn solve_font_for_height(family: &str, target_linespace: i32) -> (i32, i32) {
    let target = target_linespace.max(1);
    // MIN_SIZE_PX/MAX_SIZE_PX -- bornes de sécurité contre une boucle qui
    // marcherait indéfiniment si `linespace_for_size` échouait à mesurer
    // quoi que ce soit de cohérent (police introuvable, par exemple) ;
    // jamais atteintes en pratique pour une taille de fenêtre réaliste.
    const MIN_SIZE_PX: i32 = 8;
    const MAX_SIZE_PX: i32 = 500;

    let mut size = ((target as f32 * 0.75).round() as i32).clamp(MIN_SIZE_PX, MAX_SIZE_PX);
    let mut linespace = linespace_for_size(family, size);

    if linespace > target {
        // L'estimation dépasse déjà la cible -- recule jusqu'à la plus
        // grande taille qui tient dedans.
        while size > MIN_SIZE_PX {
            let smaller = size - 1;
            let smaller_linespace = linespace_for_size(family, smaller);
            if smaller_linespace <= target {
                return (smaller, smaller_linespace);
            }
            size = smaller;
            linespace = smaller_linespace;
        }
        return (size, linespace);
    }

    // L'estimation tient dans la cible (ou l'égale) -- avance tant que la
    // taille suivante tient ENCORE, pour ne jamais s'arrêter en-deçà de la
    // plus grande taille possible.
    while size < MAX_SIZE_PX {
        let next = size + 1;
        let next_linespace = linespace_for_size(family, next);
        if next_linespace > target {
            break;
        }
        size = next;
        linespace = next_linespace;
    }
    (size, linespace)
}

/// Hauteur (pixels physiques) de `text` replié sur `max_width_px` à la
/// police `family`/`px`, mesurée par `DrawTextW` + `DT_CALCRECT` (calcul
/// seul, aucun rendu) : une estimation au nombre de caractères ne suffit
/// pas, la largeur des glyphes variant trop en police proportionnelle. Sert
/// à dimensionner un dialogue d'après son texte (voir dialog_geometry.rs).
/// Repli sur `px`, soit une ligne, si la mesure échoue.
pub fn wrapped_text_height(family: &str, px: i32, text: &str, max_width_px: i32) -> i32 {
    unsafe {
        with_font(family, px, |hdc| {
            let mut wide_text: Vec<u16> = text.encode_utf16().collect();
            let mut rect = RECT { left: 0, top: 0, right: max_width_px.max(1), bottom: 0 };
            DrawTextW(hdc, &mut wide_text, &mut rect, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
            let height = rect.bottom - rect.top;
            if height > 0 {
                height
            } else {
                px
            }
        })
        .unwrap_or(px)
    }
}

/// Largeur naturelle de `text` sur une seule ligne, HDC déjà préparé par
/// `with_font`. `DT_SINGLELINE` (et non `DT_WORDBREAK`) : on mesure la
/// largeur du texte au lieu de le replier dans une largeur donnée.
unsafe fn measure_single_line(hdc: HDC, text: &str) -> i32 {
    let mut wide_text: Vec<u16> = text.encode_utf16().collect();
    // Borne large plutôt que 0 : DT_CALCRECT + DT_SINGLELINE calcule la
    // largeur indépendamment de cette valeur de départ, mais une borne
    // large lève toute ambiguïté.
    let mut rect = RECT { left: 0, top: 0, right: 32767, bottom: 0 };
    DrawTextW(hdc, &mut wide_text, &mut rect, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    (rect.right - rect.left).max(0)
}

/// Largeur du plus large de `texts`. Un seul HDC/police GDI pour tout
/// l'itérateur : les appelants (`longest_word_width`,
/// `dialog_geometry::list_picker_dialog_size`) passent potentiellement des
/// dizaines de libellés, en recréer un par mesure serait gratuit.
pub fn max_text_width<'a>(family: &str, px: i32, texts: impl Iterator<Item = &'a str>) -> i32 {
    unsafe { with_font(family, px, |hdc| texts.map(|t| measure_single_line(hdc, t)).max().unwrap_or(0)).unwrap_or(0) }
}

/// Largeur du plus long segment sans espace de `text`. `DT_WORDBREAK` ne
/// coupe jamais À L'INTÉRIEUR d'un mot, seulement aux espaces : un nom de
/// fichier sans espace déborde donc du cadre quelle que soit la largeur
/// disponible, même avec une hauteur de dialogue correctement mesurée. Sert
/// à élargir le dialogue à ce mot AVANT de mesurer le repli (voir
/// dialog_geometry.rs), plutôt qu'à couper le mot.
pub fn longest_word_width(family: &str, px: i32, text: &str) -> i32 {
    max_text_width(family, px, text.split_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `max_text_width` restreint à un seul texte : la production mesure
    /// toujours plusieurs libellés à la fois, ce cas n'existe qu'ici.
    fn text_width(family: &str, px: i32, text: &str) -> i32 {
        max_text_width(family, px, std::iter::once(text))
    }

    #[test]
    fn converge_vers_un_linespace_proche_de_la_cible() {
        let (size, linespace) = solve_font_for_height("Segoe UI", 22);
        assert!(size >= 8);
        // Le linespace ne varie pas à chaque taille de police : quelques
        // pixels de tolérance absorbent l'arrondi.
        assert!((linespace - 22).abs() <= 3, "linespace={linespace}");
    }

    #[test]
    fn police_invalide_ne_plante_pas() {
        let (size, _) = solve_font_for_height("Cette Police N'Existe Sûrement Pas XYZ", 20);
        assert!(size >= 8);
    }

    #[test]
    fn respecte_le_plancher_de_8px() {
        let (size, _) = solve_font_for_height("Segoe UI", 1);
        assert!(size >= 8);
    }

    #[test]
    fn wrapped_text_height_grandit_avec_un_texte_plus_long() {
        let short = wrapped_text_height("Segoe UI", 14, "Short message.", 300);
        let long = wrapped_text_height(
            "Segoe UI",
            14,
            "This is a much longer message that should wrap across several lines once constrained to a narrow width, taking up noticeably more vertical space than the short one above.",
            300,
        );
        assert!(long > short, "short={short} long={long}");
    }

    #[test]
    fn wrapped_text_height_police_invalide_ne_plante_pas() {
        let h = wrapped_text_height("Cette Police N'Existe Sûrement Pas XYZ", 14, "some text", 300);
        assert!(h > 0);
    }

    #[test]
    fn text_width_grandit_avec_un_texte_plus_long() {
        let short = text_width("Segoe UI", 14, "short.exe");
        let long = text_width("Segoe UI", 14, "a_much_longer_executable_name_indeed.exe");
        assert!(long > short, "short={short} long={long}");
    }

    #[test]
    fn text_width_police_invalide_ne_plante_pas() {
        let w = text_width("Cette Police N'Existe Sûrement Pas XYZ", 14, "some text");
        assert!(w > 0);
    }

    #[test]
    fn longest_word_width_ignore_les_mots_courts_autour() {
        let long_word = "ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne.zip";
        let whole = format!("word1 word2 {long_word} word3");
        assert_eq!(longest_word_width("Segoe UI", 14, &whole), text_width("Segoe UI", 14, long_word));
    }

    #[test]
    fn longest_word_width_texte_vide_est_zero() {
        assert_eq!(longest_word_width("Segoe UI", 14, ""), 0);
        assert_eq!(longest_word_width("Segoe UI", 14, "   "), 0);
    }
}
