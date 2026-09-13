//! Lecture de `ports.json`, le catalogue statique des ports disponibles.
//!
//! Une erreur ici (fichier absent, JSON mal formé, racine qui n'est pas un
//! objet, aucun port utilisable...) est TOUJOURS fatale -- `ports.json` est
//! le catalogue même du lanceur, potentiellement édité à la main, pas de
//! raison de démarrer quand même avec un catalogue vide plutôt que de
//! signaler clairement le problème (voir `state.rs`, qui lui repart
//! silencieusement sur des valeurs par défaut sur un fichier jamais édité à
//! la main).

use super::models::{port_from_value, Port};
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Json(String),
    /// JSON valide mais racine qui n'est pas un objet (un tableau, un
    /// simple nombre...).
    NotAnObject,
    /// JSON valide, forme d'objet valide, mais aucun port utilisable dedans.
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

pub fn load_config(path: &Path) -> Result<Vec<Port>, ConfigError> {
    let text = fs::read_to_string(path).map_err(ConfigError::Io)?;
    parse_catalog(&text)
}

/// Cœur de `load_config`, séparé pour être réutilisable sur du texte déjà en
/// mémoire : `catalog_sync` valide un `ports.json` téléchargé AVANT
/// d'écraser la copie locale, jamais après -- un JSON cassé côté distant ne
/// doit pas rendre le catalogue local inutilisable au prochain lancement.
pub fn parse_catalog(text: &str) -> Result<Vec<Port>, ConfigError> {
    let data: serde_json::Value = serde_json::from_str(text).map_err(|e| ConfigError::Json(e.to_string()))?;
    let obj = data.as_object().ok_or(ConfigError::NotAnObject)?;

    let mut ports = Vec::new();
    if let Some(raw_ports) = obj.get("ports").and_then(serde_json::Value::as_array) {
        for p in raw_ports {
            // Entrée individuelle malformée (champ requis manquant/mal
            // typé) -- ignorée plutôt que de faire échouer tout le
            // catalogue pour une seule entrée invalide.
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

/// Catalogue LOCAL de l'utilisateur (`ports.local.json`, à côté de
/// `ports.json`) -- ses ports n'ont pas besoin de "source" (voir
/// `SourceType::Local`) : l'utilisateur y ajoute ses propres jeux/ports déjà
/// installés à la main dans `Library/`. Le fichier séparé garantit qu'une
/// mise à jour de `ports.json` (remplacé en entier par le mainteneur)
/// n'efface jamais ces entrées. Jamais fatal, contrairement à `load_config` :
/// fichier absent, JSON mal formé ou vide retombent silencieusement sur un
/// catalogue local vide -- c'est un fichier de confort, pas le catalogue
/// même du lanceur.
pub fn load_local_config(path: &Path) -> Vec<Port> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    let Some(obj) = data.as_object() else { return Vec::new() };
    let Some(raw_ports) = obj.get("ports").and_then(serde_json::Value::as_array) else { return Vec::new() };
    raw_ports.iter().filter_map(|p| port_from_value(p).ok()).collect()
}

/// Fusionne le catalogue local dans le principal -- une entrée locale dont
/// `folder` existe déjà côté principal REMPLACE celle du principal
/// (l'utilisateur a explicitement choisi de la personnaliser localement),
/// jamais un doublon affiché deux fois.
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
