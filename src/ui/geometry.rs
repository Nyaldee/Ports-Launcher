
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
    fn plafonne_la_hauteur_si_debordement() {
        let (w, h) = compute_window_size_for(3440, 1000, 0.80);
        assert_eq!(h, 1000);
        assert_eq!(w, (1000.0 * WINDOW_ASPECT_RATIO).round() as i32);
        assert_eq!(compute_window_size_for(3440, 1000, 0.90), (w, h));
    }

    #[test]
    fn jamais_plus_petite_quand_la_fraction_augmente() {
        let mut previous = compute_window_size_for(1920, 1080, 0.05);
        for percent in (10..=100).step_by(5) {
            let current = compute_window_size_for(1920, 1080, percent as f64 / 100.0);
            assert!(current.0 >= previous.0 && current.1 >= previous.1, "{percent} % : {previous:?} -> {current:?}");
            previous = current;
        }
    }
}
