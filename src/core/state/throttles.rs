//! Les trois throttles indépendants de `StateManager` : MAJ du launcher
//! lui-même, sync du catalogue de ports, sync du catalogue de thèmes.

use super::{installed_at_now, StateManager};
use chrono::{DateTime, Duration, Utc};

/// Délai entre deux vérifications de mise à jour du launcher LUI-MÊME (voir
/// `should_check_launcher_update`, `start_self_update_check` dans
/// `app::sync`) -- ignoré tant que `launcher_update_available` est vrai
/// (voir son commentaire de champ) : une fois une MAJ détectée, plus la
/// peine de retaper l'API tant qu'elle n'a pas été appliquée. Les ports
/// installés N'utilisent PAS ce throttle : chacun a sa propre règle, basée
/// sur son propre `installed_at` plutôt que sur un timer global (voir
/// `is_stale_for_update_check`/`launch_with_update_check`) -- un timer
/// unique ne scalait pas avec le nombre de ports installés (un redémarrage
/// après le délai réveillait TOUS les ports d'un coup, en rafale).
/// `release_sync` reste néanmoins l'interrupteur commun aux deux mécanismes.
const LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS: i64 = 24;

/// Délai de fraîcheur de la vérification PAR PORT au Play (voir
/// `launch_with_update_check`, seul point d'entrée réseau par port -- un
/// port en auto-MAJ désactivée n'est lui-même JAMAIS vérifié) : en dessous,
/// une release vient de toute façon d'être posée il y a moins d'une
/// journée, pas la peine de retaper l'API pour le savoir.
const PORT_UPDATE_STALE_HOURS: i64 = 24;

/// Même valeur que LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS par simplicité --
/// mais un champ ENTIÈREMENT séparé, à dessein : `catalog_sync.rs` tape
/// `raw.githubusercontent.com` (un CDN de fichiers), pas l'API, donc
/// n'est soumis à AUCUN quota partagé avec les releases -- rien ne
/// justifierait de le bloquer si l'autre throttle venait d'être déclenché.
const CATALOG_CHECK_INTERVAL_HOURS: i64 = 12;

/// Délai de synchronisation de `themes.json` (voir
/// `should_check_themes`/`app::sync::start_themes_sync`) -- champ séparé de
/// CATALOG_CHECK_INTERVAL_HOURS bien que basé sur le même mécanisme (ETag
/// conditionnel sur `raw.githubusercontent.com`, voir
/// `catalog_sync::fetch_if_changed`) : rien n'oblige les deux fichiers à se
/// rafraîchir au même rythme, gardés comme deux constantes distinctes pour
/// pouvoir un jour diverger -- MÊME valeur que CATALOG_CHECK_INTERVAL_HOURS
/// pour l'instant, aucune raison de la garder plus longue : un check
/// conditionnel sur un CDN ne coûte quasiment rien de plus tous les 12h que
/// tous les 24h (voir le commentaire de module de catalog_sync.rs).
const THEMES_CHECK_INTERVAL_HOURS: i64 = 12;

/// Vrai si `installed_at` est vide (jamais posé, ne devrait pas arriver pour
/// un port installé mais traité par prudence comme "à vérifier") ou vieux de
/// plus de `PORT_UPDATE_STALE_HOURS` -- unique garde-fou de la vérification
/// par port désormais (voir le commentaire de
/// LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS), à la place d'un throttle global :
/// chaque port est ainsi revérifié au rythme de SA propre installation,
/// jamais en rafale avec tous les autres.
pub fn is_stale_for_update_check(installed_at: &str) -> bool {
    if installed_at.is_empty() {
        return true;
    }
    let Ok(last) = DateTime::parse_from_rfc3339(installed_at) else { return true };
    Utc::now().signed_duration_since(last) >= Duration::hours(PORT_UPDATE_STALE_HOURS)
}

impl StateManager {
    /// Faux si `release_sync` est désactivé, OU si `launcher_update_available`
    /// est déjà vrai (voir son commentaire de champ -- inutile de reconfirmer
    /// une MAJ déjà connue), OU si le dernier check date de moins de
    /// `LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS`. `last_launcher_update_check`
    /// vide -> FAUX, jamais vrai : pas de requête tant qu'on n'a pas la
    /// preuve que 24h se sont écoulées depuis une date connue -- `main()`
    /// pose cette date dès le chargement (voir son commentaire), avant même
    /// le premier appel à cette fonction, donc ce cas ne se présente en
    /// pratique qu'une fraction de seconde au tout premier lancement.
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
        // Format compact d'installed_at_now() -- relu par
        // should_check_launcher_update via parse_from_rfc3339, qui accepte
        // le suffixe "Z" (RFC 3339 valide) aussi bien que "+00:00".
        self.last_launcher_update_check = installed_at_now();
        self.save();
    }

    /// Voir le commentaire de `launcher_update_available`. Appelé par le
    /// handler d'`AppEvent::SelfUpdateAvailable` (true) et par
    /// `launch_self_update` juste avant de relancer (false).
    pub fn set_launcher_update_available(&mut self, value: bool) {
        self.launcher_update_available = value;
        self.save();
    }

    /// Interrupteur global de Settings (voir open_settings_dialog) --
    /// coupe/rétablit self-update ET les vérifications par port d'un coup,
    /// voir le commentaire du champ.
    pub fn set_release_sync(&mut self, value: bool) {
        self.release_sync = value;
        self.save();
    }

    /// Même principe que should_check_launcher_update -- une horloge LOCALE
    /// (jamais de réseau pour cette décision elle-même, voir
    /// catalog_sync::fetch_if_changed pour la partie qui en fait vraiment
    /// un), juste un throttle ENTIÈREMENT séparé (voir
    /// CATALOG_CHECK_INTERVAL_HOURS et le commentaire de `catalog_sync`).
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

    /// Appelée après CHAQUE tentative de vérification (304 comme 200) --
    /// remet le compteur à zéro dans les deux cas, exactement comme
    /// mark_launcher_update_check. `etag` vide si la réponse n'en portait pas
    /// (voir son commentaire de champ).
    pub fn mark_catalog_check(&mut self, etag: String) {
        self.last_catalog_check = installed_at_now();
        self.last_catalog_etag = etag;
        self.save();
    }

    /// Même principe que `should_check_catalog` -- throttle ENTIÈREMENT
    /// séparé (voir THEMES_CHECK_INTERVAL_HOURS), gardé par `catalog_sync`
    /// (même interrupteur que `ports.json` : les deux ne sont que des
    /// fichiers de données synchronisés depuis GitHub, aucun quota API en
    /// jeu contrairement à `release_sync`).
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

    /// Voir le commentaire de `mark_catalog_check` -- même convention.
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
        // Pas de requête tant qu'aucune date de référence n'est connue --
        // c'est `main()` qui pose cette date au chargement (voir le
        // commentaire de la fonction), jamais cette fonction elle-même.
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
        // Relatif à la constante plutôt qu'un nombre en dur.
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
