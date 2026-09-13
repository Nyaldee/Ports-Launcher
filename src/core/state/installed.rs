//! État par-port installé : tag/date d'installation, exécutable favori,
//! préférence d'auto-MAJ, temps de jeu cumulé.

use super::{installed_at_now, StateManager};
use crate::core::models::InstalledInfo;

impl StateManager {
    pub fn mark_installed(&mut self, key: &str, tag: Option<String>) {
        // Préserve favorite_exe, update (préférence d'auto-MAJ) ET
        // playtime_seconds de l'entrée existante -- une (ré)install/MAJ ne
        // doit pas effacer un choix explicite de l'utilisateur ni remettre
        // son temps de jeu à zéro (voir set_favorite_exe/set_port_update/
        // add_playtime). Un chemin devenu invalide après un changement de
        // version est de toute façon revalidé (existence sur disque) au
        // moment du Play, pas ici.
        let existing = self.installed.get(key);
        let favorite_exe = existing.and_then(|i| i.favorite_exe.clone());
        let update = existing.map(|i| i.update).unwrap_or(true);
        let playtime_seconds = existing.map(|i| i.playtime_seconds).unwrap_or(0);
        self.installed.insert(
            key.to_string(),
            InstalledInfo { installed_tag: tag, installed_at: installed_at_now(), favorite_exe, update, playtime_seconds },
        );
        self.save();
    }

    /// Appelée UNIQUEMENT au moment où un process lancé pour `key` est
    /// détecté terminé (voir app::playtime::record_playtime) -- `seconds`
    /// est la durée de CETTE session, ajoutée au cumul déjà connu. No-op
    /// silencieux si l'entrée a disparu entre-temps (désinstallation
    /// pendant que le jeu tournait) -- pas de or_insert_with, contrairement
    /// à set_port_update : rien à cumuler pour un port qui n'est plus suivi.
    pub fn add_playtime(&mut self, key: &str, seconds: u64) {
        if let Some(info) = self.installed.get_mut(key) {
            info.playtime_seconds += seconds;
            self.save();
        }
    }

    /// Bouton "Reset Game Time" d'InfoDialog -- remet le compteur à zéro
    /// pour CE port précisément, sans toucher au reste de l'entrée.
    pub fn reset_playtime(&mut self, key: &str) {
        if let Some(info) = self.installed.get_mut(key) {
            info.playtime_seconds = 0;
            self.save();
        }
    }

    /// Bouton "Update" d'InfoDialog / badge barré de la ligne (voir
    /// app::dialogs::open_update_toggle_dialog) -- active/désactive
    /// l'auto-MAJ de CE port précisément. Même convention que
    /// set_favorite_exe : crée une entrée minimale si besoin plutôt que
    /// d'ignorer silencieusement l'appel (le bouton n'est de toute façon
    /// accessible que pour un port installé).
    pub fn set_port_update(&mut self, key: &str, value: bool) {
        self.installed.entry(key.to_string()).or_insert_with(|| InstalledInfo { installed_at: installed_at_now(), ..Default::default() }).update = value;
        self.save();
    }

    pub fn mark_removed(&mut self, key: &str) {
        self.installed.remove(key);
        self.save();
    }

    /// Fixe/efface (`None`) l'exécutable favori de `key` -- voir le bouton
    /// sous "Select version" dans InfoDialog et son usage dans `launch_flow`.
    /// Mute l'entrée existante en place pour ne pas écraser ses autres
    /// champs ; si elle n'existe pas encore (le bouton n'est accessible que
    /// pour un port installé), en crée une minimale plutôt que d'ignorer
    /// silencieusement l'appel.
    pub fn set_favorite_exe(&mut self, key: &str, exe: Option<String>) {
        self.installed.entry(key.to_string()).or_insert_with(|| InstalledInfo { installed_at: installed_at_now(), ..Default::default() }).favorite_exe = exe;
        self.save();
    }

    pub fn get(&self, key: &str) -> Option<&InstalledInfo> {
        self.installed.get(key)
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
    fn mark_installed_puis_removed_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("roundtrip");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        let info = reloaded.get("owner/repo").unwrap();
        assert_eq!(info.installed_tag.as_deref(), Some("v1.0"));

        state.mark_removed("owner/repo");
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(reloaded.get("owner/repo").is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_favorite_exe_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("favorite_exe");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_favorite_exe("owner/repo", Some("C:\\Games\\repo\\game.exe".to_string()));
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert_eq!(reloaded.get("owner/repo").unwrap().favorite_exe.as_deref(), Some("C:\\Games\\repo\\game.exe"));

        state.set_favorite_exe("owner/repo", None);
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(reloaded.get("owner/repo").unwrap().favorite_exe.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_installed_preserve_favorite_exe_existant() {
        let path = temp_path("favorite_exe_survives_update");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
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

    #[test]
    fn update_vrai_par_defaut_apres_install() {
        let path = temp_path("update_default_true");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        assert!(state.get("owner/repo").unwrap().update);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_port_update_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("port_update_toggle");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_port_update("owner/repo", false);
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(!reloaded.get("owner/repo").unwrap().update);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_port_update_avant_toute_install_puis_mark_installed_preserve_false() {
        // Scénario signalé par l'utilisateur : désactiver l'auto-MAJ sur un
        // port PAS ENCORE installé (le bouton "Update" d'InfoDialog est
        // accessible même non installé, voir update_toggle_enabled dans
        // app::dialogs), avant tout appel à mark_installed.
        let path = temp_path("update_false_before_first_install");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.set_port_update("owner/repo", false);
        assert!(!state.get("owner/repo").unwrap().update);

        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        assert!(!state.get("owner/repo").unwrap().update, "update repassé à true après le premier mark_installed");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_installed_preserve_update_desactive() {
        let path = temp_path("update_survives_reinstall");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.set_port_update("owner/repo", false);

        // Select version (voir app::install_launch::open_version_picker)
        // réinstalle en pinnant explicitement update=false APRÈS
        // mark_installed -- ce test couvre juste mark_installed seule, qui
        // ne doit dans tous les cas jamais réactiver un auto-update déjà
        // désactivé par l'utilisateur.
        state.mark_installed("owner/repo", Some("v2.0".to_string()));
        assert!(!state.get("owner/repo").unwrap().update);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn add_playtime_cumule_et_survit_a_mark_installed() {
        let path = temp_path("playtime_accumulates");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.add_playtime("owner/repo", 120);
        state.add_playtime("owner/repo", 30);
        assert_eq!(state.get("owner/repo").unwrap().playtime_seconds, 150);

        // Une MAJ ne doit jamais remettre le compteur à zéro.
        state.mark_installed("owner/repo", Some("v2.0".to_string()));
        assert_eq!(state.get("owner/repo").unwrap().playtime_seconds, 150);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn add_playtime_no_op_si_port_pas_installe() {
        let path = temp_path("playtime_noop_missing");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.add_playtime("owner/repo", 120);
        assert!(state.get("owner/repo").is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reset_playtime_remet_a_zero_puis_sauvegarde_recharge_correctement() {
        let path = temp_path("playtime_reset");
        let mut state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        state.mark_installed("owner/repo", Some("v1.0".to_string()));
        state.add_playtime("owner/repo", 3600);
        state.reset_playtime("owner/repo");
        let reloaded = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert_eq!(reloaded.get("owner/repo").unwrap().playtime_seconds, 0);
        let _ = fs::remove_file(&path);
    }
}
