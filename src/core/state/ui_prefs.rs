
use super::StateManager;

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
        self.window_width_fraction = (percent as f64 / 100.0).clamp(0.05, 1.0);
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
}
