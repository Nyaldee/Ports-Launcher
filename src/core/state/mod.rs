
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

fn installed_at_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
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
    pub last_themes_check: String,
    pub last_themes_etag: String,
    pub release_sync: bool,
    pub last_launcher_update_check: String,
    pub launcher_update_available: bool,
    pub catalog_sync: bool,
    pub last_catalog_check: String,
    pub last_catalog_etag: String,
    pub language: String,
    pub discord_rpc_enabled: bool,
}

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
    let Ok(data) = serde_json::from_str::<Value>(super::config::strip_bom(&text)) else { return empty };
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
            discord_rpc_enabled: false,
        };

        let Ok(text) = fs::read_to_string(&state.path) else {
            state.save();
            return state;
        };
        let Ok(data) = serde_json::from_str::<Value>(super::config::strip_bom(&text)) else {
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
            state.window_width_fraction = ui
                .get("window_size")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .map(|n| (n / 100.0).clamp(0.05, 1.0))
                .unwrap_or(0.30);
            state.border_width =
                ui.get("border").and_then(Value::as_f64).filter(|n| n.is_finite()).map(|n| (n as i32).clamp(0, 100)).unwrap_or(1);
        } else {
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
        state.discord_rpc_enabled = obj.get("discord_rpc_enabled").and_then(Value::as_bool).unwrap_or(false);

        if let Some(installed) = obj.get("installed").and_then(Value::as_object) {
            for (key, info) in installed {
                let Some(info) = info.as_object() else { continue };
                let installed_tag = info.get("installed_tag").and_then(Value::as_str).map(str::to_string);
                let installed_at = info.get("installed_at").and_then(Value::as_str).unwrap_or("").to_string();
                let favorite_exe = info.get("favorite_exe").and_then(Value::as_str).map(str::to_string);
                let update = info.get("update").and_then(Value::as_bool).unwrap_or(true);
                let playtime_seconds = info.get("playtime_seconds").and_then(Value::as_u64).unwrap_or(0);
                let last_played_at = info.get("last_played_at").and_then(Value::as_str).unwrap_or("").to_string();
                state.installed.insert(
                    key.clone(),
                    InstalledInfo { installed_tag, installed_at, favorite_exe, update, playtime_seconds, last_played_at },
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
                        "last_played_at": info.last_played_at,
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
        assert!(path.exists());
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
