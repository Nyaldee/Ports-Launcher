
pub const WINDOW_ASPECT_RATIO: f64 = 3.0 / 4.0;
pub const WINDOW_MIN_WIDTH: i32 = 380;

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
        let (w, h) = compute_window_size_for(3440, 1000, 0.80);
        assert_eq!(h, 1000);
        assert_eq!(w, (1000.0 * WINDOW_ASPECT_RATIO).round() as i32);
    }

    #[test]
    fn plafonne_a_la_meme_taille_maximale_au_dela_du_seuil() {
        let a = compute_window_size_for(3440, 1000, 0.80);
        let b = compute_window_size_for(3440, 1000, 0.90);
        assert_eq!(a, b);
    }

    #[test]
    fn jamais_plus_petite_quand_la_fraction_augmente() {
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
