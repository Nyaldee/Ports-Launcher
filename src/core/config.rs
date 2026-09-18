
use super::models::{port_from_value, Port};
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Json(String),
    NotAnObject,
    NoPorts,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "{e}"),
            ConfigError::Json(msg) => write!(f, "{msg}"),
            ConfigError::NotAnObject => write!(f, "la racine de ports.json n'est pas un objet"),
            ConfigError::NoPorts => write!(f, "ports.json ne contient aucun port"),
        }
    }
}

pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

pub fn load_config(path: &Path) -> Result<Vec<Port>, ConfigError> {
    let text = fs::read_to_string(path).map_err(ConfigError::Io)?;
    parse_catalog(&text)
}

pub fn parse_catalog(text: &str) -> Result<Vec<Port>, ConfigError> {
    let data: serde_json::Value = serde_json::from_str(strip_bom(text)).map_err(|e| ConfigError::Json(e.to_string()))?;
    let obj = data.as_object().ok_or(ConfigError::NotAnObject)?;

    let mut ports = Vec::new();
    if let Some(raw_ports) = obj.get("ports").and_then(serde_json::Value::as_array) {
        for p in raw_ports {
            if let Ok(port) = port_from_value(p) {
                ports.push(port);
            }
        }
    }

    if ports.is_empty() {
        return Err(ConfigError::NoPorts);
    }
    Ok(ports)
}

pub fn load_local_config(path: &Path) -> Vec<Port> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let Ok(data) = serde_json::from_str::<serde_json::Value>(strip_bom(&text)) else { return Vec::new() };
    let Some(obj) = data.as_object() else { return Vec::new() };
    let Some(raw_ports) = obj.get("ports").and_then(serde_json::Value::as_array) else { return Vec::new() };
    raw_ports
        .iter()
        .filter_map(|p| port_from_value(p).ok())
        .map(|mut port| {
            port.user_managed = true;
            port
        })
        .collect()
}

pub fn merge_local_catalog(mut main: Vec<Port>, local: Vec<Port>) -> Vec<Port> {
    for port in local {
        main.retain(|p| p.folder != port.folder);
        main.push(port);
    }
    main
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("ports_launcher_config_test_{}_{}.json", std::process::id(), name));
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn charge_des_ports_valides() {
        let path = write_temp(
            "ok",
            r#"{"ports":[{"name":"A","folder":"a","source":"https://example.com/a.zip"}]}"#,
        );
        let ports = load_config(&path).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "A");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn entree_sans_name_est_ignoree_pas_fatale() {
        let path = write_temp(
            "missing_name",
            r#"{"ports":[{"folder":"a","source":"s"},{"name":"B","folder":"b","source":"s"}]}"#,
        );
        let ports = load_config(&path).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "B");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn champ_mal_type_est_ignore_silencieusement() {
        let path = write_temp(
            "bad_type",
            r#"{"ports":[{"name":5,"folder":"a","source":"s"},{"name":"B","folder":"b","source":"s"}]}"#,
        );
        let ports = load_config(&path).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "B");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn catalogue_vide_est_fatal() {
        let path = write_temp("empty", r#"{"ports":[]}"#);
        assert!(matches!(load_config(&path), Err(ConfigError::NoPorts)));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn racine_non_objet_est_fatale() {
        let path = write_temp("array_root", r#"[1,2,3]"#);
        assert!(matches!(load_config(&path), Err(ConfigError::NotAnObject)));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn fichier_absent_est_fatal() {
        let mut path = std::env::temp_dir();
        path.push("ports_launcher_config_test_does_not_exist.json");
        let _ = fs::remove_file(&path);
        assert!(matches!(load_config(&path), Err(ConfigError::Io(_))));
    }

    #[test]
    fn load_local_config_fichier_absent_est_vide_pas_fatal() {
        let mut path = std::env::temp_dir();
        path.push("ports_launcher_local_config_test_does_not_exist.json");
        let _ = fs::remove_file(&path);
        assert!(load_local_config(&path).is_empty());
    }

    #[test]
    fn load_local_config_json_mal_forme_est_vide_pas_fatal() {
        let path = write_temp("local_bad_json", "not json at all");
        assert!(load_local_config(&path).is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_local_config_accepte_une_entree_sans_source() {
        let path = write_temp("local_ok", r#"{"ports":[{"name":"My Game","folder":"my-game"}]}"#);
        let ports = load_local_config(&path);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].source_type, super::super::models::SourceType::Local);
        assert!(ports[0].user_managed);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_config_entree_sans_source_nest_pas_user_managed() {
        let path = write_temp("main_no_source", r#"{"ports":[{"name":"Website Game","folder":"website-game","website":"https://example.com"}]}"#);
        let ports = load_config(&path).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].source_type, super::super::models::SourceType::Local);
        assert!(!ports[0].user_managed);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn merge_local_catalog_ajoute_les_nouvelles_entrees() {
        let main = vec![port_from_value(&serde_json::json!({"name": "A", "folder": "a", "source": "s"})).unwrap()];
        let local = vec![port_from_value(&serde_json::json!({"name": "B", "folder": "b"})).unwrap()];
        let merged = merge_local_catalog(main, local);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_local_catalog_une_entree_locale_remplace_le_principal_sur_le_meme_folder() {
        let main = vec![port_from_value(&serde_json::json!({"name": "Official", "folder": "a", "source": "s"})).unwrap()];
        let local = vec![port_from_value(&serde_json::json!({"name": "Customized", "folder": "a"})).unwrap()];
        let merged = merge_local_catalog(main, local);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Customized");
    }
}
