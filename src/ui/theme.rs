//! Chargement, aperçu et écriture des thèmes (`themes.json`), avec bornage
//! des réglages numériques à la lecture, plus les couleurs sémantiques
//! fixes de l'application (boutons d'action, liens GitHub/Discord).

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
    /// Nom du thème persisté dans `themes.json` -- distinct de `current`,
    /// qui diverge temporairement pendant un aperçu (voir `preview_theme`).
    pub active_theme: String,
    pub font_family: Option<String>,
    pub placeholder_text: String,
    pub show_clock: bool,
    /// Fraction 0.0-1.0 de la taille d'écran, forme attendue par
    /// `ui::geometry`. Persisté dans `themes.json` sous `window_size`, un
    /// entier 0-100 (%) plus lisible à l'édition manuelle : seuls `load` et
    /// `commit_window_size` connaissent ce facteur 100.
    pub window_width_fraction: f64,
    pub border_width: i32,
    pub themes: HashMap<String, Theme>,
    pub current: Theme,
    pub semantic: SemanticColors,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        let mut themes = HashMap::new();
        themes.insert("arc-dark".to_string(), Theme::fallback());
        ThemeConfig {
            active_theme: "arc-dark".to_string(),
            font_family: None,
            placeholder_text: "Type to search...".to_string(),
            show_clock: true,
            window_width_fraction: 0.30,
            border_width: 3,
            current: Theme::fallback(),
            themes,
            semantic: SemanticColors::default(),
        }
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

/// Charge `themes.json` dans `cfg`. Silencieux sur toute erreur (fichier
/// absent, JSON invalide, aucun thème exploitable) : `cfg` garde son état
/// précédent plutôt que de faire échouer le démarrage.
pub fn load(path: &Path, cfg: &mut ThemeConfig) {
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

    let active = obj.get("theme").and_then(Value::as_str).unwrap_or(&cfg.active_theme).to_string();
    let applied = themes.get(&active).copied().unwrap_or_else(|| *themes.values().next().unwrap());

    cfg.font_family = obj.get("font_family").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    cfg.placeholder_text =
        obj.get("placeholder_text").and_then(Value::as_str).unwrap_or("Type to search...").to_string();
    cfg.show_clock = obj.get("show_clock").and_then(Value::as_bool).unwrap_or(true);
    // Bornage obligatoire : une valeur JSON syntaxiquement valide mais
    // absurde (`window_size: 1e300`, `border: 2147483647`) fait déborder le
    // calcul de géométrie en aval. "window_size" est un pourcentage 0-100,
    // converti en fraction ici (voir le champ window_width_fraction).
    cfg.window_width_fraction = obj
        .get("window_size")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .map(|n| (n / 100.0).clamp(0.05, 1.0))
        .unwrap_or(0.30);
    cfg.border_width = obj
        .get("border")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .map(|n| (n as i32).clamp(0, 100))
        .unwrap_or(3);
    cfg.active_theme = active;
    cfg.current = applied;
    cfg.themes = themes;
}

/// Applique les couleurs d'un thème sans toucher au disque ni à
/// `active_theme` -- aperçu en direct du sélecteur. Sans effet si le nom
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

/// Écrit le nouveau thème actif dans `themes.json` par remplacement ciblé
/// de la valeur de la clé "theme" plutôt que par resérialisation complète :
/// préserve le formatage du fichier édité à la main.
pub fn commit_theme(path: &Path, new_name: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let key_pos = text.find("\"theme\"").ok_or("'theme' key not found in themes.json")?;
    let after_key = &text[key_pos + 7..];
    let colon_rel = after_key.find(':').ok_or("':' missing after 'theme'")?;
    let after_colon = &after_key[colon_rel + 1..];
    let quote_start_rel = after_colon.find('"').ok_or("'theme' value not found")?;
    let value_start = key_pos + 7 + colon_rel + 1 + quote_start_rel + 1;
    let value_end = text[value_start..].find('"').map(|i| value_start + i).ok_or("'theme' value not terminated")?;

    let mut new_text = String::with_capacity(text.len());
    new_text.push_str(&text[..value_start]);
    new_text.push_str(new_name);
    new_text.push_str(&text[value_end..]);
    fs::write(path, new_text).map_err(|e| e.to_string())
}

/// Équivalent de `commit_theme` pour une valeur NUMÉRIQUE (non quotée) à la
/// racine de `themes.json`. Une telle valeur n'ayant pas de délimiteur
/// fermant, sa fin est le premier caractère hors littéral JSON nombre
/// (chiffre, signe, point, exposant).
fn commit_number(path: &Path, key: &str, value: i64) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let quoted_key = format!("\"{}\"", key);
    let key_pos = text.find(&quoted_key).ok_or_else(|| format!("key '{}' not found in themes.json", key))?;
    let after_key = &text[key_pos + quoted_key.len()..];
    let colon_rel = after_key.find(':').ok_or_else(|| format!("':' missing after '{}'", key))?;
    let after_colon = &after_key[colon_rel + 1..];
    let value_start_rel =
        after_colon.find(|c: char| !c.is_whitespace()).ok_or_else(|| format!("'{}' value not found", key))?;
    let value_start = key_pos + quoted_key.len() + colon_rel + 1 + value_start_rel;
    let value_len = text[value_start..]
        .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E'))
        .unwrap_or(text.len() - value_start);
    let value_end = value_start + value_len;

    let mut new_text = String::with_capacity(text.len());
    new_text.push_str(&text[..value_start]);
    new_text.push_str(&value.to_string());
    new_text.push_str(&text[value_end..]);
    fs::write(path, new_text).map_err(|e| e.to_string())
}

/// Persiste la taille de fenêtre (Ctrl+1..9/0 dans `app-window.slint`) --
/// `size_percent` est un pourcentage 0-100, la forme JSON de "window_size".
pub fn commit_window_size(path: &Path, size_percent: i32) -> Result<(), String> {
    commit_number(path, "window_size", size_percent as i64)
}

/// Persiste l'épaisseur de bordure (Ctrl+-/Ctrl+= dans `app-window.slint`).
pub fn commit_border(path: &Path, border_px: i32) -> Result<(), String> {
    commit_number(path, "border", border_px as i64)
}

/// Police de `themes.json` si réglée, sinon Segoe UI (police système par
/// défaut de Windows).
pub fn resolve_font_family(cfg: &ThemeConfig) -> String {
    cfg.font_family.clone().unwrap_or_else(|| "Segoe UI".to_string())
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
  "theme": "night",
  "font_family": "Segoe UI",
  "placeholder_text": "Type to search...",
  "show_clock": true,
  "window_size": 30,
  "border": 3,
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
    fn charge_le_theme_actif_et_les_reglages_racine() {
        let path = temp_path("load_ok");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg);
        assert_eq!(cfg.active_theme, "night");
        assert_eq!(cfg.current.search_background, rgb(0x40, 0x45, 0x52));
        assert_eq!(cfg.font_family.as_deref(), Some("Segoe UI"));
        assert_eq!(cfg.border_width, 3);
        assert_eq!(cfg.window_width_fraction, 0.30);
        assert_eq!(cfg.themes.len(), 2);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn fichier_absent_garde_l_etat_precedent() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        let mut cfg = ThemeConfig::default();
        let before = cfg.current;
        load(&path, &mut cfg);
        assert_eq!(cfg.current, before);
    }

    #[test]
    fn preview_change_current_sans_toucher_active_theme() {
        let path = temp_path("preview");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg);
        preview_theme(&mut cfg, "day");
        assert_eq!(cfg.current.search_background, rgb(0xff, 0xff, 0xff));
        assert_eq!(cfg.active_theme, "night");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn list_theme_names_est_triee() {
        let path = temp_path("list_names");
        fs::write(&path, SAMPLE).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg);
        assert_eq!(list_theme_names(&cfg), vec!["day".to_string(), "night".to_string()]);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn rejette_des_reglages_extremes_sans_planter() {
        let path = temp_path("extreme");
        let text = SAMPLE.replace("\"window_size\": 30", "\"window_size\": 1e300").replace("\"border\": 3", "\"border\": 2147483647");
        fs::write(&path, text).unwrap();
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg);
        assert_eq!(cfg.window_width_fraction, 1.0);
        assert_eq!(cfg.border_width, 100);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn commit_theme_remplace_seulement_la_valeur_theme() {
        let path = temp_path("commit");
        fs::write(&path, SAMPLE).unwrap();
        commit_theme(&path, "day").unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("\"theme\": \"day\""));
        assert!(after.contains("\"font_family\": \"Segoe UI\""));
        assert!(after.contains("\"night\": {"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn commit_window_size_remplace_seulement_cette_valeur() {
        let path = temp_path("commit_window_size");
        fs::write(&path, SAMPLE).unwrap();
        commit_window_size(&path, 90).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("\"window_size\": 90"));
        assert!(after.contains("\"border\": 3")); // reste intact
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg);
        assert_eq!(cfg.window_width_fraction, 0.90);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn commit_border_remplace_seulement_cette_valeur() {
        let path = temp_path("commit_border");
        fs::write(&path, SAMPLE).unwrap();
        commit_border(&path, 7).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("\"border\": 7"));
        assert!(after.contains("\"window_size\": 30")); // reste intact
        let mut cfg = ThemeConfig::default();
        load(&path, &mut cfg);
        assert_eq!(cfg.border_width, 7);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn commit_number_gere_les_valeurs_negatives() {
        let path = temp_path("commit_negative");
        fs::write(&path, SAMPLE).unwrap();
        commit_border(&path, -1).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("\"border\": -1"));
        let _ = fs::remove_file(&path);
    }
}
