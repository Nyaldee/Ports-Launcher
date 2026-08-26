//! `state.json` -- bookkeeping interne de l'appli (jamais édité à la main,
//! contrairement à `ports.json`/`themes.json`) : jetons GitHub/GitLab,
//! dernier mode plein écran, throttle des vérifications de mise à jour,
//! catalogue des ports installés. Un fichier corrompu (crash en plein
//! milieu d'une sauvegarde, disque plein...) ne doit jamais empêcher le
//! lanceur de démarrer -- chargement tolérant, repli silencieux sur un état
//! vide plutôt qu'une erreur fatale (à l'opposé de `ports.json`, voir
//! `config.rs`).
//!
//! Découpé par domaine de persistance : ce fichier ne porte que la struct et
//! le (dé)sérialisation JSON elle-même ; `throttles`/`installed`/`ui_prefs`
//! ajoutent chacun leur propre `impl StateManager` pour leur groupe de
//! méthodes (Rust autorise plusieurs blocs `impl` dans des fichiers
//! différents), sans changer le format JSON ni l'API publique de
//! `StateManager`.

mod installed;
mod throttles;
mod ui_prefs;

pub use throttles::is_stale_for_update_check;

use super::models::InstalledInfo;
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Horodatage de `InstalledInfo::installed_at` -- format `%Y-%m-%dT%H:%M:%SZ`
/// (sans fraction de seconde, suffixe `Z`) plutôt que `DateTime::to_rfc3339`
/// (`+00:00` et microsecondes) : cette valeur est comparée TEXTUELLEMENT
/// (`>`) aux dates de release des API GitHub/GitLab (voir
/// `github_api::update_decision`), elles-mêmes dans ce format à la seconde
/// près. Coller au même format écarte tout risque de comparaison faussée.
/// Partagée par `throttles`/`installed` (visible d'eux via `super::`, ce
/// module étant leur parent).
fn installed_at_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub struct StateManager {
    path: PathBuf,
    pub github_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub installed: HashMap<String, InstalledInfo>,
    /// Regroupés sous la clé JSON "ui" (voir `load`/`save`) -- préférences
    /// personnelles d'affichage, à dessein SÉPARÉES de `themes.json` (voir
    /// son commentaire de module) : `themes.json` peut être réécrit en
    /// entier par une synchronisation GitHub (voir `should_check_themes`),
    /// ces champs-ci ne doivent JAMAIS l'être.
    pub fullscreen: bool,
    /// Nom du thème actif (voir `ui::theme::ThemeConfig.themes`, le
    /// catalogue de couleurs lui-même reste dans `themes.json`).
    pub active_theme: String,
    pub font_family: Option<String>,
    // Volontairement PAS de @tr()/traduction pour ce champ après le premier
    // lancement -- un utilisateur qui veut ce texte dans sa langue peut déjà
    // l'éditer lui-même dans state.json, pas besoin d'un second mécanisme de
    // traduction pour un champ dont c'est justement la raison d'être. SEUL
    // le tout premier `state.json` jamais créé reçoit une valeur traduite
    // selon la langue système (voir `main()`, `set_placeholder_text`) --
    // ensuite, un changement de langue ne le retouche plus jamais.
    pub placeholder_text: String,
    pub show_clock: bool,
    /// Fraction 0.0-1.0 de la taille d'écran, forme attendue par
    /// `ui::geometry`. Persisté sous `window_size`, un entier 0-100 (%) plus
    /// lisible à l'édition manuelle : seuls `load` et `set_window_size`
    /// connaissent ce facteur 100.
    pub window_width_fraction: f64,
    pub border_width: i32,
    pub last_themes_check: String,
    /// Même convention que `last_catalog_etag`.
    pub last_themes_etag: String,
    /// Bascule pour désactiver TOUTE vérification de mise à jour -- celle du
    /// launcher lui-même (voir `should_check_launcher_update`) ET celle par
    /// port (voir `is_stale_for_update_check`/`launch_with_update_check`
    /// dans main.rs) -- true par défaut. Réglable depuis Settings (voir
    /// `open_settings_dialog`).
    pub release_sync: bool,
    pub last_launcher_update_check: String,
    /// Vrai dès qu'une MAJ du launcher lui-même a été détectée (voir
    /// `start_self_update_check`/`AppEvent::SelfUpdateAvailable`) -- reste
    /// vrai tant qu'elle n'a pas été appliquée, ce qui suspend
    /// `should_check_launcher_update` (voir son commentaire) : inutile de
    /// retaper l'API pour reconfirmer ce qu'on sait déjà. Persisté pour que
    /// le bouton "Update" (voir `AppWindow.self-update-available`) survive à
    /// un redémarrage sans nouveau check. Remis à false par
    /// `launch_self_update` juste avant de lancer le `.bat` -- le tout
    /// nouveau build qui redémarre ensuite n'a plus de raison de croire
    /// qu'une MAJ l'attend encore.
    pub launcher_update_available: bool,
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
    /// Vide -- suit la langue du système (voir
    /// slint::select_bundled_translation, appelée uniquement si non vide).
    /// Une valeur explicite ("fr", "ja"...) force cette langue peu importe
    /// celle de Windows, choisie depuis Settings.
    pub language: String,
}

/// Lit les anciennes clés racine de `themes.json` (`theme`/`font_family`/
/// `placeholder_text`/`show_clock`/`window_size`/`border`, avant leur
/// déménagement vers `state.json`."ui") -- UNIQUEMENT utilisé comme migration
/// une fois, quand `state.json` n'a pas encore de bloc `"ui"` (voir `load`).
/// Silencieux sur toute erreur : une migration ratée retombe sur les
/// défauts codés en dur, jamais un échec de démarrage.
struct LegacyThemeRoot {
    theme: Option<String>,
    font_family: Option<String>,
    placeholder_text: Option<String>,
    show_clock: Option<bool>,
    window_size: Option<f64>,
    border: Option<f64>,
}

fn read_legacy_theme_root(themes_path: &Path) -> LegacyThemeRoot {
    let empty = LegacyThemeRoot { theme: None, font_family: None, placeholder_text: None, show_clock: None, window_size: None, border: None };
    let Ok(text) = fs::read_to_string(themes_path) else { return empty };
    let Ok(data) = serde_json::from_str::<Value>(&text) else { return empty };
    let Some(obj) = data.as_object() else { return empty };
    LegacyThemeRoot {
        theme: obj.get("theme").and_then(Value::as_str).map(str::to_string),
        font_family: obj.get("font_family").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
        placeholder_text: obj.get("placeholder_text").and_then(Value::as_str).map(str::to_string),
        show_clock: obj.get("show_clock").and_then(Value::as_bool),
        window_size: obj.get("window_size").and_then(Value::as_f64).filter(|n| n.is_finite()),
        border: obj.get("border").and_then(Value::as_f64).filter(|n| n.is_finite()),
    }
}

impl StateManager {
    /// `legacy_themes_path` -- UNIQUEMENT lu pour migrer un `state.json`
    /// écrit par une version antérieure au déménagement des préférences
    /// d'affichage vers `state.json`."ui" (voir `read_legacy_theme_root`) :
    /// ignoré dès que `state.json` a déjà son propre bloc `"ui"`.
    pub fn load(path: &Path, legacy_themes_path: &Path) -> StateManager {
        let mut state = StateManager {
            path: path.to_path_buf(),
            github_token: None,
            gitlab_token: None,
            installed: HashMap::new(),
            fullscreen: false,
            active_theme: "arc-dark".to_string(),
            font_family: None,
            placeholder_text: "Type to search...".to_string(),
            show_clock: true,
            window_width_fraction: 0.30,
            border_width: 1,
            last_themes_check: String::new(),
            last_themes_etag: String::new(),
            release_sync: true,
            last_launcher_update_check: String::new(),
            launcher_update_available: false,
            catalog_sync: true,
            last_catalog_check: String::new(),
            last_catalog_etag: String::new(),
            language: String::new(),
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
        state.release_sync = obj.get("release_sync").and_then(Value::as_bool).unwrap_or(true);
        state.last_launcher_update_check = obj.get("last_launcher_update_check").and_then(Value::as_str).unwrap_or("").to_string();
        state.launcher_update_available = obj.get("launcher_update_available").and_then(Value::as_bool).unwrap_or(false);

        if let Some(ui) = obj.get("ui").and_then(Value::as_object) {
            state.fullscreen = ui.get("fullscreen").and_then(Value::as_bool).unwrap_or(false);
            state.active_theme = ui.get("theme").and_then(Value::as_str).unwrap_or("arc-dark").to_string();
            state.font_family = ui.get("font_family").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
            state.placeholder_text =
                ui.get("placeholder_text").and_then(Value::as_str).unwrap_or("Type to search...").to_string();
            state.show_clock = ui.get("show_clock").and_then(Value::as_bool).unwrap_or(true);
            // Bornage obligatoire : une valeur JSON syntaxiquement valide
            // mais absurde (`window_size: 1e300`, `border: 2147483647`) fait
            // déborder le calcul de géométrie en aval. "window_size" est un
            // pourcentage 0-100, converti en fraction ici (voir
            // window_width_fraction).
            state.window_width_fraction = ui
                .get("window_size")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .map(|n| (n / 100.0).clamp(0.05, 1.0))
                .unwrap_or(0.30);
            state.border_width =
                ui.get("border").and_then(Value::as_f64).filter(|n| n.is_finite()).map(|n| (n as i32).clamp(0, 100)).unwrap_or(1);
        } else {
            // Migration UNIQUE : ce state.json vient d'une version
            // antérieure au déménagement des préférences d'affichage --
            // `fullscreen` vivait alors à la racine de CE fichier,
            // theme/font_family/placeholder_text/show_clock/window_size/
            // border à la racine de `themes.json` (voir
            // read_legacy_theme_root). Le prochain `save()` (déclenché par
            // n'importe quel setter) écrit définitivement le nouveau format,
            // cette branche ne sert donc qu'une fois par installation.
            state.fullscreen = obj.get("fullscreen").and_then(Value::as_bool).unwrap_or(false);
            let legacy = read_legacy_theme_root(legacy_themes_path);
            if let Some(theme) = legacy.theme {
                state.active_theme = theme;
            }
            if legacy.font_family.is_some() {
                state.font_family = legacy.font_family;
            }
            if let Some(text) = legacy.placeholder_text {
                state.placeholder_text = text;
            }
            if let Some(show_clock) = legacy.show_clock {
                state.show_clock = show_clock;
            }
            if let Some(percent) = legacy.window_size {
                state.window_width_fraction = (percent / 100.0).clamp(0.05, 1.0);
            }
            if let Some(border) = legacy.border {
                state.border_width = (border as i32).clamp(0, 100);
            }
        }
        state.last_themes_check = obj.get("last_themes_check").and_then(Value::as_str).unwrap_or("").to_string();
        state.last_themes_etag = obj.get("last_themes_etag").and_then(Value::as_str).unwrap_or("").to_string();
        state.catalog_sync = obj.get("catalog_sync").and_then(Value::as_bool).unwrap_or(true);
        state.last_catalog_check = obj.get("last_catalog_check").and_then(Value::as_str).unwrap_or("").to_string();
        state.last_catalog_etag = obj.get("last_catalog_etag").and_then(Value::as_str).unwrap_or("").to_string();
        state.language = obj.get("language").and_then(Value::as_str).unwrap_or("").to_string();

        if let Some(installed) = obj.get("installed").and_then(Value::as_object) {
            for (key, info) in installed {
                let Some(info) = info.as_object() else { continue };
                // Champ absent/mal typé : repli sur sa valeur par défaut,
                // jamais un échec du chargement entier.
                let installed_tag = info.get("installed_tag").and_then(Value::as_str).map(str::to_string);
                let installed_at = info.get("installed_at").and_then(Value::as_str).unwrap_or("").to_string();
                let favorite_exe = info.get("favorite_exe").and_then(Value::as_str).map(str::to_string);
                let update = info.get("update").and_then(Value::as_bool).unwrap_or(true);
                let playtime_seconds = info.get("playtime_seconds").and_then(Value::as_u64).unwrap_or(0);
                state.installed.insert(
                    key.clone(),
                    InstalledInfo { installed_tag, installed_at, favorite_exe, update, playtime_seconds },
                );
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
                        "update": info.update,
                        "playtime_seconds": info.playtime_seconds,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into();

        let data = json!({
            "ui": {
                "fullscreen": self.fullscreen,
                "theme": self.active_theme,
                "font_family": self.font_family,
                "placeholder_text": self.placeholder_text,
                "show_clock": self.show_clock,
                "window_size": (self.window_width_fraction * 100.0).round() as i64,
                "border": self.border_width,
            },
            "language": self.language,
            "github_token": self.github_token,
            "gitlab_token": self.gitlab_token,
            "release_sync": self.release_sync,
            "catalog_sync": self.catalog_sync,
            "last_launcher_update_check": self.last_launcher_update_check,
            "launcher_update_available": self.launcher_update_available,
            "last_catalog_check": self.last_catalog_check,
            "last_catalog_etag": self.last_catalog_etag,
            "last_themes_check": self.last_themes_check,
            "last_themes_etag": self.last_themes_etag,
            "installed": installed,
        });
        // Écrit au mieux -- une écriture ratée (disque plein, dossier
        // supprimé entre-temps) n'a pas besoin de faire planter l'appli
        // pour un fichier de bookkeeping non critique.
        let _ = fs::write(&self.path, serde_json::to_string_pretty(&data).unwrap_or_default());
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
        let state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(state.installed.is_empty());
        assert!(!state.fullscreen);
        assert!(path.exists()); // sauvegardé une première fois
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn fichier_corrompu_repart_sur_un_etat_vide_sans_paniquer() {
        let path = temp_path("corrupt");
        fs::write(&path, "{ceci n'est pas du json valide").unwrap();
        let state = StateManager::load(&path, Path::new("ports_launcher_test_no_legacy_themes.json"));
        assert!(state.installed.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn migre_les_anciennes_cles_racine_de_state_json_et_themes_json() {
        let path = temp_path("legacy_migration");
        // Ancien format de state.json : "fullscreen" à la racine, pas de
        // bloc "ui".
        fs::write(&path, r#"{"fullscreen": true}"#).unwrap();

        let mut themes_path = std::env::temp_dir();
        themes_path.push(format!("ports_launcher_state_test_{}_legacy_themes.json", std::process::id()));
        fs::write(&themes_path, r#"{"theme": "cappuccino", "font_family": "Segoe UI", "show_clock": false, "window_size": 45, "border": 1, "themes": {}}"#).unwrap();

        let state = StateManager::load(&path, &themes_path);
        assert!(state.fullscreen);
        assert_eq!(state.active_theme, "cappuccino");
        assert_eq!(state.font_family.as_deref(), Some("Segoe UI"));
        assert!(!state.show_clock);
        assert_eq!(state.window_width_fraction, 0.45);
        assert_eq!(state.border_width, 1);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&themes_path);
    }

    #[test]
    fn state_json_avec_bloc_ui_ignore_la_migration_legacy() {
        let path = temp_path("no_migration_needed");
        fs::write(&path, r#"{"ui": {"theme": "night"}}"#).unwrap();

        let mut themes_path = std::env::temp_dir();
        themes_path.push(format!("ports_launcher_state_test_{}_should_be_ignored.json", std::process::id()));
        fs::write(&themes_path, r#"{"theme": "cappuccino"}"#).unwrap();

        let state = StateManager::load(&path, &themes_path);
        assert_eq!(state.active_theme, "night");

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&themes_path);
    }

}
