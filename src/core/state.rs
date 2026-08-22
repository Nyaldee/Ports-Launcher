//! `state.json` -- bookkeeping interne de l'appli (jamais édité à la main,
//! contrairement à `ports.json`/`themes.json`) : jetons GitHub/GitLab,
//! dernier mode plein écran, throttle des vérifications de mise à jour,
//! catalogue des ports installés. Un fichier corrompu (crash en plein
//! milieu d'une sauvegarde, disque plein...) ne doit jamais empêcher le
//! lanceur de démarrer -- chargement tolérant, repli silencieux sur un état
//! vide plutôt qu'une erreur fatale (à l'opposé de `ports.json`, voir
//! `config.rs`).

use super::models::InstalledInfo;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Une seule vérification des releases par tranche de 12h -- persisté dans
/// state.json, donc plusieurs redémarrages de l'appli n'en redéclenchent
/// qu'une. Couvre à la fois l'auto-MAJ du launcher ET la vérif de MAJ des
/// ports installés (voir `should_check_releases`) : les deux tapent la même
/// API GitHub/GitLab, soumise au même quota non authentifié (60 req/h) --
/// un seul throttle pour ce budget réseau commun, pas deux qui pourraient
/// se cumuler. Les releases des ports restent rares, pas besoin d'un
/// intervalle plus court.
const RELEASE_CHECK_INTERVAL_HOURS: i64 = 12;

/// Même valeur que RELEASE_CHECK_INTERVAL_HOURS par simplicité -- mais un
/// champ ENTIÈREMENT séparé, à dessein : `catalog_sync.rs` tape
/// `raw.githubusercontent.com` (un CDN de fichiers), pas l'API, donc
/// n'est soumis à AUCUN quota partagé avec les releases -- rien ne
/// justifierait de le bloquer si l'autre throttle venait d'être déclenché.
const CATALOG_CHECK_INTERVAL_HOURS: i64 = 12;

/// Horodatage de `InstalledInfo::installed_at` -- format `%Y-%m-%dT%H:%M:%SZ`
/// (sans fraction de seconde, suffixe `Z`) plutôt que `DateTime::to_rfc3339`
/// (`+00:00` et microsecondes) : cette valeur est comparée TEXTUELLEMENT
/// (`>`) aux dates de release des API GitHub/GitLab (voir
/// `github_api::update_decision`), elles-mêmes dans ce format à la seconde
/// près. Coller au même format écarte tout risque de comparaison faussée.
fn installed_at_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub struct StateManager {
    path: PathBuf,
    pub github_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub installed: HashMap<String, InstalledInfo>,
    pub fullscreen: bool,
    /// Bascule pour désactiver l'auto-MAJ du launcher ET la vérif de MAJ des
    /// ports installés (voir `should_check_releases`) -- true par défaut.
    /// Un seul interrupteur pour les deux : ils partagent déjà le même
    /// throttle/quota API, pas de raison de les activer/désactiver
    /// indépendamment.
    pub release_sync: bool,
    pub last_release_check: String,
    /// Bascule pour désactiver la vérification distante du catalogue (voir
    /// catalog_sync) -- true par défaut : le fichier local/embarqué reste
    /// TOUJOURS utilisable hors-ligne quel que soit l'état de cette
    /// bascule, elle ne contrôle que le rafraîchissement en tâche de fond.
    /// Un interrupteur distinct de `release_sync` : rafraîchir le catalogue
    /// (CDN, aucun quota partagé) et vérifier une nouvelle release (API,
    /// quota partagé) sont deux besoins sans rapport technique, chacun le
    /// sien.
    pub catalog_sync: bool,
    pub last_catalog_check: String,
    /// Vide -- jamais vérifié, ou dernière réponse sans ETag (improbable
    /// pour raw.githubusercontent.com, mais pas garanti) : voir
    /// fetch_if_changed, envoyé sans `If-None-Match` dans ce cas, se
    /// comporte alors comme un simple GET.
    pub last_catalog_etag: String,
}

impl StateManager {
    pub fn load(path: &Path) -> StateManager {
        let mut state = StateManager {
            path: path.to_path_buf(),
            github_token: None,
            gitlab_token: None,
            installed: HashMap::new(),
            fullscreen: false,
            release_sync: true,
            last_release_check: String::new(),
            catalog_sync: true,
            last_catalog_check: String::new(),
            last_catalog_etag: String::new(),
        };

        let Ok(text) = fs::read_to_string(&state.path) else {
            state.save();
            return state;
        };
        let Ok(data) = serde_json::from_str::<Value>(&text) else {
            return state;
        };
        let Some(obj) = data.as_object() else {
            return state;
        };

        state.github_token = obj.get("github_token").and_then(Value::as_str).map(str::to_string);
        state.gitlab_token = obj.get("gitlab_token").and_then(Value::as_str).map(str::to_string);
        state.fullscreen = obj.get("fullscreen").and_then(Value::as_bool).unwrap_or(false);
        state.release_sync = obj.get("release_sync").and_then(Value::as_bool).unwrap_or(true);
        state.last_release_check = obj.get("last_release_check").and_then(Value::as_str).unwrap_or("").to_string();
        state.catalog_sync = obj.get("catalog_sync").and_then(Value::as_bool).unwrap_or(true);
        state.last_catalog_check = obj.get("last_catalog_check").and_then(Value::as_str).unwrap_or("").to_string();
        state.last_catalog_etag = obj.get("last_catalog_etag").and_then(Value::as_str).unwrap_or("").to_string();

        if let Some(installed) = obj.get("installed").and_then(Value::as_object) {
            for (key, info) in installed {
                let Some(info) = info.as_object() else { continue };
                // Champ absent/mal typé : repli sur sa valeur par défaut,
                // jamais un échec du chargement entier.
                let installed_tag = info.get("installed_tag").and_then(Value::as_str).map(str::to_string);
                let installed_at = info.get("installed_at").and_then(Value::as_str).unwrap_or("").to_string();
                let favorite_exe = info.get("favorite_exe").and_then(Value::as_str).map(str::to_string);
                state.installed.insert(key.clone(), InstalledInfo { installed_tag, installed_at, favorite_exe });
            }
        }

        state
    }

    fn save(&self) {
        let installed: Value = self
            .installed
            .iter()
            .map(|(key, info)| {
                (
                    key.clone(),
                    json!({
                        "installed_tag": info.installed_tag,
                        "installed_at": info.installed_at,
                        "favorite_exe": info.favorite_exe,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into();

        let data = json!({
            "fullscreen": self.fullscreen,
            "github_token": self.github_token,
            "gitlab_token": self.gitlab_token,
            "release_sync": self.release_sync,
            "catalog_sync": self.catalog_sync,
            "last_release_check": self.last_release_check,
            "last_catalog_check": self.last_catalog_check,
            "last_catalog_etag": self.last_catalog_etag,
            "installed": installed,
        });
        // Écrit au mieux -- une écriture ratée (disque plein, dossier
        // supprimé entre-temps) n'a pas besoin de faire planter l'appli
        // pour un fichier de bookkeeping non critique.
        let _ = fs::write(&self.path, serde_json::to_string_pretty(&data).unwrap_or_default());
    }

    pub fn should_check_releases(&self) -> bool {
        if !self.release_sync {
            return false;
        }
        if self.last_release_check.is_empty() {
            return true;
        }
        let Ok(last) = DateTime::parse_from_rfc3339(&self.last_release_check) else {
            return true;
        };
        Utc::now().signed_duration_since(last) >= Duration::hours(RELEASE_CHECK_INTERVAL_HOURS)
    }

    pub fn mark_release_check(&mut self) {
        // Format compact d'installed_at_now() -- relu par
        // should_check_releases via parse_from_rfc3339, qui accepte le
        // suffixe "Z" (RFC 3339 valide) aussi bien que "+00:00".
        self.last_release_check = installed_at_now();
        self.save();
    }

    // RÉSERVÉ : pas encore de case à cocher dans l'UI pour ces deux champs,
    // donc pas d'appelant hors tests. Les champs sont bien lus/écrits dans
    // state.json et respectés par should_check_releases/start_update_checks/
    // start_self_update_check -- à conserver pour le futur interrupteur d'UI.
    #[allow(dead_code)]
    pub fn set_release_sync(&mut self, value: bool) {
        self.release_sync = value;
        self.save();
    }

    /// Même principe que should_check_releases -- une horloge LOCALE
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
    /// mark_release_check. `etag` vide si la réponse n'en portait pas
    /// (voir son commentaire de champ).
    pub fn mark_catalog_check(&mut self, etag: String) {
        self.last_catalog_check = installed_at_now();
        self.last_catalog_etag = etag;
        self.save();
    }

    #[allow(dead_code)]
    pub fn set_catalog_sync(&mut self, value: bool) {
        self.catalog_sync = value;
        self.save();
    }

    pub fn set_fullscreen(&mut self, value: bool) {
        self.fullscreen = value;
        self.save();
    }

    pub fn mark_installed(&mut self, key: &str, tag: Option<String>) {
        // Préserve favorite_exe de l'entrée existante -- une (ré)install/MAJ
        // ne doit pas effacer le choix explicite de l'utilisateur (voir
        // set_favorite_exe). Un chemin devenu invalide après un changement
        // de version est de toute façon revalidé (existence sur disque) au
        // moment du Play, pas ici.
        let favorite_exe = self.installed.get(key).and_then(|i| i.favorite_exe.clone());
        self.installed.insert(key.to_string(), InstalledInfo { installed_tag: tag, installed_at: installed_at_now(), favorite_exe });
        self.save();
    }

    pub fn mark_removed(&mut self, key: &str) {
        self.installed.remove(key);
        self.save();
    }

    /// Fixe/efface (`None`) l'exécutable favori de `key` -- voir le bouton
    /// sous "Change version" dans InfoDialog et son usage dans `launch_flow`.
    /// Mute l'entrée existante en place pour ne pas écraser ses autres
    /// champs ; si elle n'existe pas encore (le bouton n'est accessible que
    /// pour un port installé), en crée une minimale plutôt que d'ignorer
    /// silencieusement l'appel.
    pub fn set_favorite_exe(&mut self, key: &str, exe: Option<String>) {
        self.installed.entry(key.to_string()).or_insert_with(|| InstalledInfo {
            installed_tag: None,
            installed_at: installed_at_now(),
            favorite_exe: None,
        }).favorite_exe = exe;
        self.save();
    }

    pub fn get(&self, key: &str) -> Option<&InstalledInfo> {
        self.installed.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_state_test_{}_{}.json", std::process::id(), name));
        let _ = fs::remove_file(&p);
        p
    }

    #[test]
    fn fichier_absent_cree_un_etat_vide() {
        let path = temp_path("missing");
        let state = StateManager::load(&path);
        assert!(state.installed.is_empty());
        assert!(!state.fullscreen);
        assert!(path.exists()); // sauvegardé une première fois
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn fichier_corrompu_repart_sur_un_etat_vide_sans_paniquer() {
        let path = temp_path("corrupt");
        fs::write(&path, "{ceci n'est pas du json valide").unwrap();
        let state = StateManager::load(&path);
        assert!(state.installed.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_releases_jamais_verifie() {
        let path = temp_path("throttle_never");
        let state = StateManager::load(&path);
        assert!(state.should_check_releases());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_releases_recent_est_false() {
        let path = temp_path("throttle_recent");
        let mut state = StateManager::load(&path);
        state.mark_release_check();
        assert!(!state.should_check_releases());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_releases_ancien_est_true() {
        let path = temp_path("throttle_old");
        let mut state = StateManager::load(&path);
        // Relatif à la constante plutôt qu'un nombre en dur.
        state.last_release_check = (Utc::now() - Duration::hours(RELEASE_CHECK_INTERVAL_HOURS + 1)).to_rfc3339();
        assert!(state.should_check_releases());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_releases_desactive_est_toujours_false() {
        let path = temp_path("release_throttle_disabled");
        let mut state = StateManager::load(&path);
        state.set_release_sync(false);
        assert!(!state.should_check_releases());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_jamais_verifie() {
        let path = temp_path("catalog_throttle_never");
        let state = StateManager::load(&path);
        assert!(state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_recent_est_false() {
        let path = temp_path("catalog_throttle_recent");
        let mut state = StateManager::load(&path);
        state.mark_catalog_check("\"abc123\"".to_string());
        assert!(!state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_ancien_est_true() {
        let path = temp_path("catalog_throttle_old");
        let mut state = StateManager::load(&path);
        state.last_catalog_check = (Utc::now() - Duration::hours(25)).to_rfc3339();
        assert!(state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn should_check_catalog_desactive_est_toujours_false() {
        let path = temp_path("catalog_throttle_disabled");
        let mut state = StateManager::load(&path);
        state.set_catalog_sync(false);
        assert!(!state.should_check_catalog());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn release_sync_vrai_par_defaut_et_persiste() {
        let path = temp_path("release_sync_default");
        let mut state = StateManager::load(&path);
        assert!(state.release_sync);
        state.set_release_sync(false);
        let reloaded = StateManager::load(&path);
        assert!(!reloaded.release_sync);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn catalog_sync_vrai_par_defaut() {
        let path = temp_path("catalog_sync_default");
        let state = StateManager::load(&path);
        assert!(state.catalog_sync);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_catalog_check_puis_sauvegarde_recharge_l_etag() {
        let path = temp_path("catalog_etag_roundtrip");
        let mut state = StateManager::load(&path);
        state.mark_catalog_check("\"abc123\"".to_string());
        let reloaded = StateManager::load(&path);
        assert_eq!(reloaded.last_catalog_etag, "\"abc123\"");
        assert!(!reloaded.last_catalog_check.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_installed_puis_removed_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("roundtrip");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        let reloaded = StateManager::load(&path);
        let info = reloaded.get("owner/repo").unwrap();
        assert_eq!(info.installed_tag.as_deref(), Some("v1.0"));

        state.mark_removed("owner/repo");
        let reloaded = StateManager::load(&path);
        assert!(reloaded.get("owner/repo").is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_favorite_exe_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("favorite_exe");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_favorite_exe("owner/repo", Some("C:\\Games\\repo\\game.exe".to_string()));
        let reloaded = StateManager::load(&path);
        assert_eq!(reloaded.get("owner/repo").unwrap().favorite_exe.as_deref(), Some("C:\\Games\\repo\\game.exe"));

        state.set_favorite_exe("owner/repo", None);
        let reloaded = StateManager::load(&path);
        assert!(reloaded.get("owner/repo").unwrap().favorite_exe.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_installed_preserve_favorite_exe_existant() {
        let path = temp_path("favorite_exe_survives_update");
        let mut state = StateManager::load(&path);
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_favorite_exe("owner/repo", Some("C:\\Games\\repo\\game.exe".to_string()));

        // Mise à jour vers une nouvelle version : mark_installed est appelée
        // à nouveau et ne doit PAS effacer le favori déjà choisi.
        state.mark_installed("owner/repo", Some("v2.0".to_string()));
        let info = state.get("owner/repo").unwrap();
        assert_eq!(info.installed_tag.as_deref(), Some("v2.0"));
        assert_eq!(info.favorite_exe.as_deref(), Some("C:\\Games\\repo\\game.exe"));
        let _ = fs::remove_file(&path);
    }
}
