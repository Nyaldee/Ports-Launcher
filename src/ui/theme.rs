//! Chargement et aperçu des thèmes (`themes.json`) -- le catalogue de
//! couleurs nommées lui-même, synchronisé depuis GitHub (voir
//! `core::catalog_sync`/`main.rs::start_themes_sync`), plus les couleurs
//! sémantiques fixes de l'application (boutons d'action, liens
//! GitHub/Discord). Les préférences personnelles (thème ACTIF, police,
//! taille de fenêtre...) vivent dans `state.json` (voir `core::state`), pas
//! ici : `themes.json` peut être réécrit en entier par une synchronisation,
//! il ne doit jamais porter un réglage propre à l'utilisateur.

use serde_json::Value;
use slint::Color;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_argb_encoded(0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub search_background: Color,
    pub search_text: Color,
    pub list_background: Color,
    pub list_text: Color,
    pub selected_background: Color,
    pub selected_text: Color,
    pub border: Color,
}

impl Theme {
    /// Thème de repli si `themes.json` est absent ou entièrement invalide --
    /// jamais écrit sur disque.
    fn fallback() -> Theme {
        Theme {
            search_background: rgb(0x40, 0x45, 0x52),
            search_text: rgb(0x7c, 0x81, 0x8c),
            list_background: rgb(0x38, 0x3c, 0x4a),
            list_text: rgb(0xd3, 0xda, 0xe3),
            selected_background: rgb(0x52, 0x94, 0xe2),
            selected_text: rgb(0xff, 0xff, 0xff),
            border: rgb(0x4b, 0x51, 0x62),
        }
    }
}

/// Couleurs de marque et de sémantique d'action, non éditables via
/// `themes.json` : identiques quel que soit le thème actif.
#[derive(Clone, Copy, Debug)]
pub struct SemanticColors {
    pub brand_github: Color,
    pub brand_github_hover: Color,
    pub brand_discord: Color,
    pub brand_discord_hover: Color,
    pub success: Color,
    pub success_hover: Color,
    pub warning: Color,
    pub warning_hover: Color,
    pub danger: Color,
    pub danger_hover: Color,
    pub info: Color,
    pub info_hover: Color,
    /// Bordure de sélection des cartes en grille plein écran -- hors thème
    /// pour rester nettement visible sur n'importe quel fond.
    pub border_strong: Color,
    /// Texte posé sur les boutons d'action colorés : blanc fixe, jamais
    /// `Theme.list_text` (pas garanti lisible sur vert/orange/rouge/bleu).
    pub text_on_accent: Color,
}

impl Default for SemanticColors {
    fn default() -> Self {
        SemanticColors {
            brand_github: rgb(0x24, 0x29, 0x2f),
            brand_github_hover: rgb(0x32, 0x38, 0x3f),
            brand_discord: rgb(0x58, 0x65, 0xf2),
            brand_discord_hover: rgb(0x6b, 0x76, 0xf5),
            success: rgb(0x78, 0xb1, 0x59),
            success_hover: rgb(0x93, 0xc1, 0x7a),
            warning: rgb(0xf4, 0x90, 0x0c),
            warning_hover: rgb(0xf6, 0xa6, 0x3d),
            danger: rgb(0xdd, 0x2e, 0x44),
            danger_hover: rgb(0xe4, 0x58, 0x69),
            info: rgb(0x55, 0xac, 0xee),
            info_hover: rgb(0x77, 0xbd, 0xf1),
            border_strong: rgb(0xff, 0xff, 0xff),
            text_on_accent: rgb(0xff, 0xff, 0xff),
        }
    }
}

pub struct ThemeConfig {
    pub themes: HashMap<String, Theme>,
    /// Couleurs actuellement APPLIQUÉES -- diverge temporairement du thème
    /// réellement actif (voir `core::state::StateManager::active_theme`)
    /// pendant un aperçu (voir `preview_theme`).
    pub current: Theme,
    pub semantic: SemanticColors,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        let mut themes = HashMap::new();
        themes.insert("arc-dark".to_string(), Theme::fallback());
        ThemeConfig { current: Theme::fallback(), themes, semantic: SemanticColors::default() }
    }
}

/// "#rrggbb" ou "#rgb" -> `Color`, `None` sinon. Seul format accepté par
/// `themes.json` (voir le README) : les noms de couleurs CSS sont rejetés.
pub fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    let (r, g, b) = match s.len() {
        6 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        ),
        3 => {
            let mut chars = s.chars();
            let expand = |c: char| -> Option<u8> {
                let d = c.to_digit(16)? as u8;
                Some(d * 16 + d)
            };
            (expand(chars.next()?)?, expand(chars.next()?)?, expand(chars.next()?)?)
        }
        _ => return None,
    };
    Some(rgb(r, g, b))
}

fn parse_theme_entry(v: &Value) -> Option<Theme> {
    let color = |key: &str| v.get(key).and_then(Value::as_str).and_then(parse_hex_color);
    Some(Theme {
        search_background: color("search_background")?,
        search_text: color("search_text")?,
        list_background: color("list_background")?,
        list_text: color("list_text")?,
        selected_background: color("selected_background")?,
        selected_text: color("selected_text")?,
        border: color("border")?,
    })
}

/// Charge le catalogue `"themes"` de `themes.json` dans `cfg` et résout
/// `cfg.current` sur `active_theme` (voir
/// `core::state::StateManager::active_theme`, propriétaire du NOM du thème
/// actif -- ce fichier ne connaît plus que les couleurs elles-mêmes).
/// Silencieux sur toute erreur (fichier absent, JSON invalide, aucun thème
/// exploitable) : `cfg` garde son état précédent plutôt que de faire
/// échouer le démarrage.
pub fn load(path: &Path, cfg: &mut ThemeConfig, active_theme: &str) {
    let Ok(text) = fs::read_to_string(path) else { return };
    let Ok(data) = serde_json::from_str::<Value>(&text) else { return };
    let Some(obj) = data.as_object() else { return };

    let mut themes = HashMap::new();
    if let Some(theme_obj) = obj.get("themes").and_then(Value::as_object) {
        for (name, v) in theme_obj {
            if let Some(t) = parse_theme_entry(v) {
                themes.insert(name.clone(), t);
            }
        }
    }
    if themes.is_empty() {
        return;
    }

    cfg.current = themes.get(active_theme).copied().unwrap_or_else(|| *themes.values().next().unwrap());
    cfg.themes = themes;
}

/// Applique les couleurs d'un thème sans toucher au disque ni au thème
/// RÉELLEMENT actif -- aperçu en direct du sélecteur. Sans effet si le nom
/// est inconnu.
pub fn preview_theme(cfg: &mut ThemeConfig, name: &str) {
    if let Some(t) = cfg.themes.get(name) {
        cfg.current = *t;
    }
}

/// Noms de thèmes en ordre alphabétique, pour peupler le sélecteur --
/// l'ordre d'itération d'une `HashMap` n'est pas stable.
pub fn list_theme_names(cfg: &ThemeConfig) -> Vec<String> {
    let mut names: Vec<String> = cfg.themes.keys().cloned().collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_theme_test_{}_{}.json", std::process::id(), name));
        p
    }

    const SAMPLE: &str = r##"{
  "themes": {
    "night": {
      "search_background": "#404552",
      "search_text": "#7c818c",
      "list_background": "#383c4a",
      "list_text": "#d3dae3",
      "selected_background": "#5294e2",
      "selected_text": "#ffffff",
      "border": "#4b5162"
    },
    "day": {
      "search_background": "#ffffff",
      "search_text": "#000000",
      "list_background": "#eeeeee",
      "list_text": "#111111",
      "selected_background": "#3366cc",
      "selected_text": "#ffffff",
      "border": "#cccccc"
    }
  }
}"##;

    #[test]
    fn parse_hex_color_couvre_3_et_6_chiffres() {
        assert_eq!(parse_hex_color("#fff"), Some(rgb(255, 255, 255)));
        assert_eq!(parse_hex_color("#3a8ea0"), Some(rgb(0x3a, 0x8e, 0xa0)));
        assert_eq!(parse_hex_color("bogus"), None);
        assert_eq!(parse_hex_color("#12"), None);
    }

    #[test]
    fn charge_le_theme_actif_demande_et_le_catalogue() {
        let path = temp_path("load_ok");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg, "night");
        assert_eq!(cfg.current.search_background, rgb(0x40, 0x45, 0x52));
        assert_eq!(cfg.themes.len(), 2);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn theme_actif_inconnu_replie_sur_le_premier_du_catalogue() {
        let path = temp_path("load_unknown_active");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg, "does-not-exist");
        assert!(cfg.themes.contains_key("night") && cfg.themes.contains_key("day"));
        // Reste résolu à UN des deux thèmes du catalogue plutôt qu'au repli
        // par défaut, silencieusement.
        assert!(cfg.current == *cfg.themes.get("night").unwrap() || cfg.current == *cfg.themes.get("day").unwrap());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn fichier_absent_garde_l_etat_precedent() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        let mut cfg = ThemeConfig::default();
        let before = cfg.current;
        load(&path, &mut cfg, "arc-dark");
        assert_eq!(cfg.current, before);
    }

    #[test]
    fn preview_change_current_sans_toucher_le_catalogue() {
        let path = temp_path("preview");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg, "night");
        preview_theme(&mut cfg, "day");
        assert_eq!(cfg.current.search_background, rgb(0xff, 0xff, 0xff));
        assert_eq!(cfg.themes.len(), 2);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn list_theme_names_est_triee() {
        let path = temp_path("list_names");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg, "night");
        assert_eq!(list_theme_names(&cfg), vec!["day".to_string(), "night".to_string()]);
        let _ = fs::remove_file(&path);
    }
}
