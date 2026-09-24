
mod installed;
mod throttles;
mod ui_prefs;

pub use throttles::is_stale_for_update_check;

use super::models::InstalledInfo;
use chrono::Utc;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_THEME: &str = "arc-dark";
const DEFAULT_PLACEHOLDER: &str = "Type to search...";
const DEFAULT_WINDOW_FRACTION: f64 = 0.30;
const DEFAULT_BORDER: i32 = 1;

fn now_timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn window_fraction_from_percent(percent: f64) -> f64 {
    (percent / 100.0).clamp(0.05, 1.0)
}

pub struct StateManager {
    path: PathBuf,
    pub github_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub installed: HashMap<String, InstalledInfo>,
    pub fullscreen: bool,
    pub active_theme: String,
    pub font_family: Option<String>,
    pub placeholder_text: String,
    pub show_clock: bool,
    pub window_width_fraction: f64,
    pub border_width: i32,
    pub language: String,
    pub discord_rpc_enabled: bool,
    pub release_sync: bool,
    pub last_launcher_update_check: String,
    pub launcher_update_available: bool,
    pub catalog_sync: bool,
    pub last_catalog_check: String,
    pub last_catalog_etag: String,
    pub last_themes_check: String,
    pub last_themes_etag: String,
}

impl StateManager {
    pub fn load(path: &Path) -> StateManager {
        let mut state = StateManager {
            path: path.to_path_buf(),
            github_token: None,
            gitlab_token: None,
            installed: HashMap::new(),
            fullscreen: false,
            active_theme: DEFAULT_THEME.to_string(),
            font_family: None,
            placeholder_text: DEFAULT_PLACEHOLDER.to_string(),
            show_clock: true,
            window_width_fraction: DEFAULT_WINDOW_FRACTION,
            border_width: DEFAULT_BORDER,
            language: String::new(),
            discord_rpc_enabled: false,
            release_sync: true,
            last_launcher_update_check: String::new(),
            launcher_update_available: false,
            catalog_sync: true,
            last_catalog_check: String::new(),
            last_catalog_etag: String::new(),
            last_themes_check: String::new(),
            last_themes_etag: String::new(),
        };

        let Ok(text) = fs::read_to_string(&state.path) else {
            state.save();
            return state;
        };
        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(super::config::strip_bom(&text)) else {
            return state;
        };

        let string = |o: &Map<String, Value>, key: &str| o.get(key).and_then(Value::as_str).map(str::to_string);
        let flag = |o: &Map<String, Value>, key: &str, default: bool| o.get(key).and_then(Value::as_bool).unwrap_or(default);
        let number = |o: &Map<String, Value>, key: &str| o.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());

        if let Some(ui) = obj.get("ui").and_then(Value::as_object) {
            state.fullscreen = flag(ui, "fullscreen", false);
            state.active_theme = string(ui, "theme").unwrap_or(state.active_theme);
            state.font_family = string(ui, "font_family").filter(|s| !s.is_empty());
            state.placeholder_text = string(ui, "placeholder_text").unwrap_or(state.placeholder_text);
            state.show_clock = flag(ui, "show_clock", true);
            state.window_width_fraction = number(ui, "window_size").map(window_fraction_from_percent).unwrap_or(DEFAULT_WINDOW_FRACTION);
            state.border_width = number(ui, "border").map(|n| (n as i32).clamp(0, 100)).unwrap_or(DEFAULT_BORDER);
        }
        state.language = string(&obj, "language").unwrap_or_default();
        state.discord_rpc_enabled = flag(&obj, "discord_rpc_enabled", false);
        state.github_token = string(&obj, "github_token");
        state.gitlab_token = string(&obj, "gitlab_token");
        state.release_sync = flag(&obj, "release_sync", true);
        state.last_launcher_update_check = string(&obj, "last_launcher_update_check").unwrap_or_default();
        state.launcher_update_available = flag(&obj, "launcher_update_available", false);
        state.catalog_sync = flag(&obj, "catalog_sync", true);
        state.last_catalog_check = string(&obj, "last_catalog_check").unwrap_or_default();
        state.last_catalog_etag = string(&obj, "last_catalog_etag").unwrap_or_default();
        state.last_themes_check = string(&obj, "last_themes_check").unwrap_or_default();
        state.last_themes_etag = string(&obj, "last_themes_etag").unwrap_or_default();

        if let Some(installed) = obj.get("installed").and_then(Value::as_object) {
            for (key, info) in installed {
                let Some(info) = info.as_object() else { continue };
                state.installed.insert(
                    key.clone(),
                    InstalledInfo {
                        installed_tag: string(info, "installed_tag"),
                        installed_at: string(info, "installed_at").unwrap_or_default(),
                        favorite_exe: string(info, "favorite_exe"),
                        update: flag(info, "update", true),
                        playtime_seconds: info.get("playtime_seconds").and_then(Value::as_u64).unwrap_or(0),
                        last_played_at: string(info, "last_played_at").unwrap_or_default(),
                    },
                );
            }
        }
        state
    }

    fn save(&self) {
        let installed: Map<String, Value> = self
            .installed
            .iter()
            .map(|(key, info)| {
                let entry = json!({
                    "installed_tag": info.installed_tag,
                    "installed_at": info.installed_at,
                    "favorite_exe": info.favorite_exe,
                    "update": info.update,
                    "playtime_seconds": info.playtime_seconds,
                    "last_played_at": info.last_played_at,
                });
                (key.clone(), entry)
            })
            .collect();

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
            "discord_rpc_enabled": self.discord_rpc_enabled,
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
        if let Ok(text) = serde_json::to_string_pretty(&data) {
            let _ = super::files::write_atomic(&self.path, text.as_bytes());
        }
    }
}

#[cfg(test)]
pub(super) fn test_state_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("ports_launcher_state_test_{}_{}.json", std::process::id(), name));
    let _ = fs::remove_file(&p);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichier_absent_cree_un_etat_par_defaut() {
        let path = test_state_path("missing");
        let state = StateManager::load(&path);
        assert!(state.installed.is_empty());
        assert!(!state.fullscreen);
        assert!(path.exists());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn fichier_corrompu_repart_sur_un_etat_par_defaut() {
        let path = test_state_path("corrupt");
        fs::write(&path, "{ceci n'est pas du json valide").unwrap();
        let state = StateManager::load(&path);
        assert!(state.installed.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reglages_ui_aller_retour() {
        let path = test_state_path("ui_roundtrip");
        fs::write(&path, r#"{"ui": {"theme": "night", "window_size": 45, "border": 3, "show_clock": false}}"#).unwrap();
        let mut state = StateManager::load(&path);
        assert_eq!(state.active_theme, "night");
        assert_eq!(state.window_width_fraction, 0.45);
        assert_eq!(state.border_width, 3);
        assert!(!state.show_clock);

        state.set_fullscreen(true);
        let reloaded = StateManager::load(&path);
        assert!(reloaded.fullscreen);
        assert_eq!(reloaded.active_theme, "night");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn valeurs_numeriques_absurdes_sont_bornees() {
        let path = test_state_path("clamped");
        fs::write(&path, r#"{"ui": {"window_size": 1e300, "border": -40}}"#).unwrap();
        let state = StateManager::load(&path);
        assert_eq!(state.window_width_fraction, 1.0);
        assert_eq!(state.border_width, 0);
        let _ = fs::remove_file(&path);
    }
}
