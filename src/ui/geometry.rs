//! Géométrie de la fenêtre principale -- calculée en fraction de l'écran
//! (voir `theme::ThemeConfig::window_width_fraction`), jamais une taille
//! fixe codée en dur. Logique PURE uniquement : la lecture native du
//! moniteur/DPI sous le curseur (`work_area_under_cursor`/
//! `scale_factor_under_cursor`) vit dans `windows_chrome`, seul point
//! d'entrée Win32 pour tout ce qui touche à une fenêtre.

/// Format portrait de la fenêtre (largeur/hauteur).
pub const WINDOW_ASPECT_RATIO: f64 = 3.0 / 4.0;
pub const WINDOW_MIN_WIDTH: i32 = 380;

/// `width_fraction` pilote la largeur (`screen_w * width_fraction`, hauteur
/// dérivée du ratio) tant que le résultat tient dans l'écran. Au-delà, la
/// fenêtre a atteint la plus grande taille respectant à la fois le ratio et
/// les limites de l'écran : on PLAFONNE à cette taille (hauteur = plein
/// écran, largeur redérivée), sans repasser par une autre formule. Un repli
/// en `screen_h * width_fraction` briserait la monotonie (0.50 donnerait
/// une fenêtre plus petite que 0.30) ; le plateau, lui, est correct.
pub fn compute_window_size_for(screen_w: i32, screen_h: i32, width_fraction: f64) -> (i32, i32) {
    let mut width = ((screen_w as f64 * width_fraction).round() as i32).max(WINDOW_MIN_WIDTH);
    let mut height = (width as f64 / WINDOW_ASPECT_RATIO).round() as i32;
    if height > screen_h {
        height = screen_h;
        width = ((height as f64 * WINDOW_ASPECT_RATIO).round() as i32).max(WINDOW_MIN_WIDTH);
    }
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largeur_normale_1920x1080() {
        assert_eq!(compute_window_size_for(1920, 1080, 0.30), (576, 768));
    }

    #[test]
    fn respecte_la_largeur_minimale() {
        let (w, _) = compute_window_size_for(1920, 1080, 0.01);
        assert_eq!(w, WINDOW_MIN_WIDTH);
    }

    #[test]
    fn replie_sur_la_hauteur_si_debordement() {
        // Ultrawide : le candidat piloté par la largeur déborderait en
        // hauteur, on plafonne et on redérive la largeur de cette hauteur.
        let (w, h) = compute_window_size_for(3440, 1000, 0.80);
        assert_eq!(h, 1000);
        assert_eq!(w, (1000.0 * WINDOW_ASPECT_RATIO).round() as i32);
    }

    #[test]
    fn plafonne_a_la_meme_taille_maximale_au_dela_du_seuil() {
        // Taille maximale atteinte (hauteur + ratio) : aucune fraction plus
        // grande ne peut agrandir la fenêtre, le plateau est attendu.
        let a = compute_window_size_for(3440, 1000, 0.80);
        let b = compute_window_size_for(3440, 1000, 0.90);
        assert_eq!(a, b);
    }

    #[test]
    fn jamais_plus_petite_quand_la_fraction_augmente() {
        // La taille doit rester monotone non-décroissante avec la fraction :
        // un repli en `screen_h * width_fraction` la ferait diminuer passé
        // un certain seuil.
        let mut previous = compute_window_size_for(1920, 1080, 0.05);
        let mut fraction = 0.10;
        while fraction <= 1.0 {
            let current = compute_window_size_for(1920, 1080, fraction);
            assert!(current.0 >= previous.0, "largeur a diminué à fraction={fraction} : {previous:?} -> {current:?}");
            assert!(current.1 >= previous.1, "hauteur a diminué à fraction={fraction} : {previous:?} -> {current:?}");
            previous = current;
            fraction += 0.05;
        }
    }
}
