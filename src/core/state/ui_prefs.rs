use super::{window_fraction_from_percent, StateManager};

impl StateManager {
    pub fn set_fullscreen(&mut self, value: bool) {
        self.fullscreen = value;
        self.save();
    }

    pub fn set_active_theme(&mut self, name: String) {
        self.active_theme = name;
        self.save();
    }

    pub fn set_window_size(&mut self, percent: i32) {
        self.window_width_fraction = window_fraction_from_percent(percent as f64);
        self.save();
    }

    pub fn set_border(&mut self, px: i32) {
        self.border_width = px.clamp(0, 100);
        self.save();
    }

    pub fn set_placeholder_text(&mut self, value: String) {
        self.placeholder_text = value;
        self.save();
    }

    pub fn set_language(&mut self, value: String) {
        self.language = value;
        self.save();
    }

    pub fn set_discord_rpc_enabled(&mut self, value: bool) {
        self.discord_rpc_enabled = value;
        self.save();
    }

    pub fn set_custom_presence_enabled(&mut self, value: bool) {
        self.custom_presence_enabled = value;
        self.save();
    }

    pub fn custom_presence_active(&self) -> bool {
        self.discord_rpc_enabled && self.custom_presence_enabled && !self.custom_presence_text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_state_path;
    use super::*;
    use std::fs;

    #[test]
    fn custom_presence_persiste_et_depend_de_discord_rpc() {
        let path = test_state_path("custom_presence");
        let mut state = StateManager::load(&path);
        state.custom_presence_text = "Hello".to_string();
        state.set_custom_presence_enabled(true);
        assert!(!state.custom_presence_active());
        state.set_discord_rpc_enabled(true);
        assert!(state.custom_presence_active());

        let reloaded = StateManager::load(&path);
        assert!(reloaded.custom_presence_enabled);
        assert_eq!(reloaded.custom_presence_text, "Hello");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn custom_presence_sans_texte_reste_inactif() {
        let path = test_state_path("custom_presence_empty");
        let mut state = StateManager::load(&path);
        state.set_discord_rpc_enabled(true);
        state.set_custom_presence_enabled(true);
        state.custom_presence_text = "   ".to_string();
        assert!(!state.custom_presence_active());
        let _ = fs::remove_file(&path);
    }
}
