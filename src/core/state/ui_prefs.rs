//! Préférences d'affichage persistées (thème actif, police, taille/bordure
//! de fenêtre, horloge, placeholder, langue) -- voir le commentaire de champ
//! de `StateManager::fullscreen` pour pourquoi elles sont séparées de
//! `themes.json`.

use super::StateManager;

impl StateManager {
    pub fn set_fullscreen(&mut self, value: bool) {
        self.fullscreen = value;
        self.save();
    }

    /// Bouton "Themes" du menu Settings (voir app::dialogs::open_theme_picker)
    /// -- persiste le NOM du thème actif ; les couleurs elles-mêmes restent
    /// dans `themes.json` (voir `ui::theme::ThemeConfig`).
    pub fn set_active_theme(&mut self, name: String) {
        self.active_theme = name;
        self.save();
    }

    /// Ctrl+1..9/0 (voir app-window.slint) -- `percent` est un pourcentage
    /// 0-100, converti et persisté en fraction (voir `window_width_fraction`).
    pub fn set_window_size(&mut self, percent: i32) {
        self.window_width_fraction = (percent as f64 / 100.0).clamp(0.05, 1.0);
        self.save();
    }

    /// Ctrl+-/Ctrl+= (voir app-window.slint).
    pub fn set_border(&mut self, px: i32) {
        self.border_width = px.clamp(0, 100);
        self.save();
    }

    /// Appelée UNE SEULE fois, juste après la création du tout premier
    /// `state.json` (voir `main()`) : seed initial traduit selon la langue
    /// système, jamais retouché ensuite par un changement de langue -- voir
    /// le commentaire du champ `placeholder_text`.
    pub fn set_placeholder_text(&mut self, value: String) {
        self.placeholder_text = value;
        self.save();
    }

    pub fn set_language(&mut self, value: String) {
        self.language = value;
        self.save();
    }
}
