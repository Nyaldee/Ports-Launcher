
use super::{now_timestamp, StateManager};
use chrono::{DateTime, Duration, Utc};

const LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS: i64 = 24;
const PORT_UPDATE_STALE_HOURS: i64 = 24;
const CATALOG_CHECK_INTERVAL_HOURS: i64 = 12;
const THEMES_CHECK_INTERVAL_HOURS: i64 = 12;

fn is_older_than(timestamp: &str, hours: i64) -> bool {
    DateTime::parse_from_rfc3339(timestamp).map_or(true, |last| Utc::now().signed_duration_since(last) >= Duration::hours(hours))
}

pub fn is_stale_for_update_check(installed_at: &str) -> bool {
    is_older_than(installed_at, PORT_UPDATE_STALE_HOURS)
}

impl StateManager {
    pub fn should_check_launcher_update(&self) -> bool {
        self.release_sync
            && !self.launcher_update_available
            && !self.last_launcher_update_check.is_empty()
            && is_older_than(&self.last_launcher_update_check, LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_launcher_update_check(&mut self) {
        self.last_launcher_update_check = now_timestamp();
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
        self.catalog_sync && is_older_than(&self.last_catalog_check, CATALOG_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_catalog_check(&mut self, etag: String) {
        self.last_catalog_check = now_timestamp();
        self.last_catalog_etag = etag;
        self.save();
    }

    pub fn should_check_themes(&self) -> bool {
        self.catalog_sync && is_older_than(&self.last_themes_check, THEMES_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_themes_check(&mut self, etag: String) {
        self.last_themes_check = now_timestamp();
        self.last_themes_etag = etag;
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_state_path;
    use super::*;
    use std::fs;

    #[test]
    fn launcher_jamais_verifie_ne_declenche_pas() {
        let path = test_state_path("throttle_never");
        assert!(!StateManager::load(&path).should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn launcher_verifie_recemment_ne_declenche_pas() {
        let path = test_state_path("throttle_recent");
        let mut state = StateManager::load(&path);
        state.mark_launcher_update_check();
        assert!(!state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn launcher_verifie_il_y_a_longtemps_declenche() {
        let path = test_state_path("throttle_old");
        let mut state = StateManager::load(&path);
        state.last_launcher_update_check = (Utc::now() - Duration::hours(LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS + 1)).to_rfc3339();
        assert!(state.should_check_launcher_update());

        state.set_release_sync(false);
        assert!(!state.should_check_launcher_update());
        state.set_release_sync(true);
        state.set_launcher_update_available(true);
        assert!(!state.should_check_launcher_update());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn launcher_update_available_persiste() {
        let path = test_state_path("launcher_update_available_roundtrip");
        StateManager::load(&path).set_launcher_update_available(true);
        assert!(StateManager::load(&path).launcher_update_available);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn catalogue_jamais_verifie_declenche() {
        let path = test_state_path("catalog_throttle_never");
        assert!(StateManager::load(&path).should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn catalogue_verifie_recemment_ne_declenche_pas() {
        let path = test_state_path("catalog_throttle_recent");
        let mut state = StateManager::load(&path);
        state.mark_catalog_check("\"abc123\"".to_string());
        assert!(!state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn catalogue_ancien_declenche_sauf_si_desactive() {
        let path = test_state_path("catalog_throttle_old");
        let mut state = StateManager::load(&path);
        state.last_catalog_check = (Utc::now() - Duration::hours(25)).to_rfc3339();
        assert!(state.should_check_catalog());
        state.catalog_sync = false;
        assert!(!state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn release_sync_vrai_par_defaut_et_persiste() {
        let path = test_state_path("release_sync_default");
        let mut state = StateManager::load(&path);
        assert!(state.release_sync);
        state.set_release_sync(false);
        assert!(!StateManager::load(&path).release_sync);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_catalog_check_persiste_l_etag() {
        let path = test_state_path("catalog_etag_roundtrip");
        StateManager::load(&path).mark_catalog_check("\"abc123\"".to_string());
        let reloaded = StateManager::load(&path);
        assert_eq!(reloaded.last_catalog_etag, "\"abc123\"");
        assert!(!reloaded.last_catalog_check.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn is_stale_for_update_check_vide_ou_ancien() {
        assert!(is_stale_for_update_check(""));
        assert!(is_stale_for_update_check(&(Utc::now() - Duration::hours(25)).to_rfc3339()));
        assert!(!is_stale_for_update_check(&(Utc::now() - Duration::hours(1)).to_rfc3339()));
    }
}
