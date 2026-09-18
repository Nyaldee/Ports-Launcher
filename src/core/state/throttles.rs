
use super::{installed_at_now, StateManager};
use chrono::{DateTime, Duration, Utc};

const LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS: i64 = 24;

const PORT_UPDATE_STALE_HOURS: i64 = 24;

const CATALOG_CHECK_INTERVAL_HOURS: i64 = 12;

const THEMES_CHECK_INTERVAL_HOURS: i64 = 12;

pub fn is_stale_for_update_check(installed_at: &str) -> bool {
    if installed_at.is_empty() {
        return true;
    }
    let Ok(last) = DateTime::parse_from_rfc3339(installed_at) else { return true };
    Utc::now().signed_duration_since(last) >= Duration::hours(PORT_UPDATE_STALE_HOURS)
}

impl StateManager {
    pub fn should_check_launcher_update(&self) -> bool {
        if !self.release_sync || self.launcher_update_available || self.last_launcher_update_check.is_empty() {
            return false;
        }
        let Ok(last) = DateTime::parse_from_rfc3339(&self.last_launcher_update_check) else {
            return true;
        };
        Utc::now().signed_duration_since(last) >= Duration::hours(LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_launcher_update_check(&mut self) {
        self.last_launcher_update_check = installed_at_now();
        self.save();
    }

    pub fn set_launcher_update_available(&mut self, value: bool) {
        self.launcher_update_available = value;
        self.save();
    }

    pub fn set_release_sync(&mut self, value: bool) {
        self.release_sync = value;
        self.save();
    }

    pub fn should_check_catalog(&self) -> bool {
        if !self.catalog_sync {
            return false;
        }
        if self.last_catalog_check.is_empty() {
            return true;
        }
        let Ok(last) = DateTime::parse_from_rfc3339(&self.last_catalog_check) else {
            return true;
        };
        Utc::now().signed_duration_since(last) >= Duration::hours(CATALOG_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_catalog_check(&mut self, etag: String) {
        self.last_catalog_check = installed_at_now();
        self.last_catalog_etag = etag;
        self.save();
    }

    pub fn should_check_themes(&self) -> bool {
        if !self.catalog_sync {
            return false;
        }
        if self.last_themes_check.is_empty() {
            return true;
        }
        let Ok(last) = DateTime::parse_from_rfc3339(&self.last_themes_check) else {
            return true;
        };
        Utc::now().signed_duration_since(last) >= Duration::hours(THEMES_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_themes_check(&mut self, etag: String) {
        self.last_themes_check = installed_at_now();
        self.last_themes_etag = etag;
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_state_test_{}_{}.json", std::process::id(), name));
        let _ = fs::remove_file(&p);
        p
    }

    #[test]
    fn should_check_launcher_update_jamais_verifie_est_false() {
        let path = temp_path("throttle_never");
        let state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(!state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_launcher_update_recent_est_false() {
        let path = temp_path("throttle_recent");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_launcher_update_check();
        assert!(!state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_launcher_update_ancien_est_true() {
        let path = temp_path("throttle_old");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.last_launcher_update_check = (Utc::now() - Duration::hours(LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS + 1)).to_rfc3339();
        assert!(state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_launcher_update_desactive_est_toujours_false() {
        let path = temp_path("release_throttle_disabled");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.set_release_sync(false);
        assert!(!state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_launcher_update_available_est_toujours_false() {
        let path = temp_path("launcher_update_available_suspends_check");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.set_launcher_update_available(true);
        assert!(!state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn launcher_update_available_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("launcher_update_available_roundtrip");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.set_launcher_update_available(true);
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(reloaded.launcher_update_available);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_jamais_verifie() {
        let path = temp_path("catalog_throttle_never");
        let state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_recent_est_false() {
        let path = temp_path("catalog_throttle_recent");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_catalog_check("\"abc123\"".to_string());
        assert!(!state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_ancien_est_true() {
        let path = temp_path("catalog_throttle_old");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.last_catalog_check = (Utc::now() - Duration::hours(25)).to_rfc3339();
        assert!(state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_desactive_est_toujours_false() {
        let path = temp_path("catalog_throttle_disabled");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.catalog_sync = false;
        assert!(!state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn release_sync_vrai_par_defaut_et_persiste() {
        let path = temp_path("release_sync_default");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(state.release_sync);
        state.set_release_sync(false);
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(!reloaded.release_sync);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn catalog_sync_vrai_par_defaut() {
        let path = temp_path("catalog_sync_default");
        let state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(state.catalog_sync);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_catalog_check_puis_sauvegarde_recharge_l_etag() {
        let path = temp_path("catalog_etag_roundtrip");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_catalog_check("\"abc123\"".to_string());
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert_eq!(reloaded.last_catalog_etag, "\"abc123\"");
        assert!(!reloaded.last_catalog_check.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn is_stale_for_update_check_vide_ou_ancien_est_true() {
        assert!(is_stale_for_update_check(""));
        assert!(is_stale_for_update_check(&(Utc::now() - Duration::hours(25)).to_rfc3339()));
    }

    #[test]
    fn is_stale_for_update_check_recent_est_false() {
        assert!(!is_stale_for_update_check(&(Utc::now() - Duration::hours(1)).to_rfc3339()));
    }
}
