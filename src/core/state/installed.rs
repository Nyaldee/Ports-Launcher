
use super::{now_timestamp, StateManager};
use crate::core::models::InstalledInfo;

impl StateManager {
    pub fn get(&self, key: &str) -> Option<&InstalledInfo> {
        self.installed.get(key)
    }

    fn entry(&mut self, key: &str) -> &mut InstalledInfo {
        self.installed.entry(key.to_string()).or_insert_with(|| InstalledInfo { installed_at: now_timestamp(), ..Default::default() })
    }

    pub fn mark_installed(&mut self, key: &str, tag: Option<String>) {
        let info = self.entry(key);
        info.installed_tag = tag;
        info.installed_at = now_timestamp();
        self.save();
    }

    pub fn mark_removed(&mut self, key: &str) {
        self.installed.remove(key);
        self.save();
    }

    pub fn mark_played(&mut self, key: &str) {
        self.entry(key).last_played_at = now_timestamp();
        self.save();
    }

    pub fn add_playtime(&mut self, key: &str, seconds: u64) {
        if let Some(info) = self.installed.get_mut(key) {
            info.playtime_seconds += seconds;
            self.save();
        }
    }

    pub fn reset_playtime(&mut self, key: &str) {
        if let Some(info) = self.installed.get_mut(key) {
            info.playtime_seconds = 0;
            self.save();
        }
    }

    pub fn set_port_update(&mut self, key: &str, value: bool) {
        self.entry(key).update = value;
        self.save();
    }

    pub fn set_favorite_exe(&mut self, key: &str, exe: Option<String>) {
        self.entry(key).favorite_exe = exe;
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_state_path;
    use super::*;
    use std::fs;

    #[test]
    fn mark_installed_puis_removed_persiste() {
        let path = test_state_path("roundtrip");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        let reloaded = StateManager::load(&path);
        assert_eq!(reloaded.get("owner/repo").unwrap().installed_tag.as_deref(), Some("v1.0"));

        state.mark_removed("owner/repo");
        assert!(StateManager::load(&path).get("owner/repo").is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_favorite_exe_persiste() {
        let path = test_state_path("favorite_exe");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_favorite_exe("owner/repo", Some("bin/game.exe".to_string()));
        assert_eq!(StateManager::load(&path).get("owner/repo").unwrap().favorite_exe.as_deref(), Some("bin/game.exe"));

        state.set_favorite_exe("owner/repo", None);
        assert!(StateManager::load(&path).get("owner/repo").unwrap().favorite_exe.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_installed_preserve_favori_update_et_temps_de_jeu() {
        let path = test_state_path("reinstall_preserves");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_favorite_exe("owner/repo", Some("bin/game.exe".to_string()));
        state.set_port_update("owner/repo", false);
        state.add_playtime("owner/repo", 150);

        state.mark_installed("owner/repo", Some("v2.0".to_string()));
        let info = state.get("owner/repo").unwrap();
        assert_eq!(info.installed_tag.as_deref(), Some("v2.0"));
        assert_eq!(info.favorite_exe.as_deref(), Some("bin/game.exe"));
        assert!(!info.update);
        assert_eq!(info.playtime_seconds, 150);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn update_vrai_par_defaut_apres_install() {
        let path = test_state_path("update_default_true");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        assert!(state.get("owner/repo").unwrap().update);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_port_update_persiste() {
        let path = test_state_path("port_update_toggle");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_port_update("owner/repo", false);
        assert!(!StateManager::load(&path).get("owner/repo").unwrap().update);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn add_playtime_cumule() {
        let path = test_state_path("playtime_accumulates");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.add_playtime("owner/repo", 120);
        state.add_playtime("owner/repo", 30);
        assert_eq!(state.get("owner/repo").unwrap().playtime_seconds, 150);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn add_playtime_sans_effet_si_port_non_suivi() {
        let path = test_state_path("playtime_noop_missing");
        let mut state = StateManager::load(&path);
        state.add_playtime("owner/repo", 120);
        assert!(state.get("owner/repo").is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reset_playtime_persiste() {
        let path = test_state_path("playtime_reset");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.add_playtime("owner/repo", 3600);
        state.reset_playtime("owner/repo");
        assert_eq!(StateManager::load(&path).get("owner/repo").unwrap().playtime_seconds, 0);
        let _ = fs::remove_file(&path);
    }
}
