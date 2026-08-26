//! Détection/résolution de l'exécutable à lancer pour un port installé.

use super::path_safety::safe_join;
use super::platform_resolve::{is_truthy, resolve_per_platform};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// "unins..." couvre aussi bien "uninstall.exe" que "unins000.exe" (InnoSetup).
fn is_uninstaller_name(name_lower: &str) -> bool {
    name_lower.contains("unins")
}

#[derive(Debug)]
pub enum ExecutableSelectionError {
    /// Message seul -- aucun exécutable trouvé, ou "executable" configuré
    /// pointe vers un chemin invalide/hors du dossier du jeu. Affiché tel
    /// quel dans un MessageDialog par l'appelant (voir app::install_launch::launch_flow).
    Message(String),
    /// Plusieurs candidats -- l'appelant propose un choix manuel (voir
    /// app::install_launch::launch_flow) sans avoir besoin de ce message : le
    /// dialogue de choix a son propre titre générique.
    Ambiguous(#[allow(dead_code)] String, Vec<PathBuf>),
}

/// Descend récursivement dans `dir` et ajoute à `out` chaque FICHIER dont
/// l'extension est `.exe`/`.lnk`/`.bat` et le nom n'est pas un désinstalleur
/// (voir `is_uninstaller_name`) -- filtré pendant la collecte plutôt
/// qu'après coup, pour ne pas allouer un `PathBuf` par fichier non pertinent
/// d'un dossier de jeu qui peut en contenir plusieurs milliers (assets,
/// textures...). Un dossier illisible (permissions...) est ignoré plutôt que
/// de faire échouer toute la détection.
fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_files_recursive(&path, out),
            Ok(t) if t.is_file() => {
                let is_candidate_ext =
                    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("lnk") || e.eq_ignore_ascii_case("bat"));
                if !is_candidate_ext {
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
                if !is_uninstaller_name(&name) {
                    out.push(path);
                }
            }
            _ => {}
        }
    }
}

/// `.exe` (exécutable PE), `.lnk` (raccourci Windows, qui permet de
/// préconfigurer des arguments) et `.bat` (script de lancement, courant pour
/// les ports/recomps qui posent des variables d'environnement avant le vrai
/// binaire) sont des candidats équivalents, sans préférence de l'un sur
/// l'autre -- voir `core::launch::launch` pour leurs modes d'exécution
/// respectifs.
///
/// `pub` : `app::install_launch` l'appelle aussi pour peupler le picker
/// "exécutable favori" (`open_favorite_exe_picker`) avec les mêmes
/// candidats que le flux Play, indépendamment de tout override "executable"
/// dans ports.json.
pub fn autodetect_executable(game_dir: &Path) -> Result<PathBuf, ExecutableSelectionError> {
    let mut candidates = Vec::new();
    collect_files_recursive(game_dir, &mut candidates);

    let dir_name = game_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(ExecutableSelectionError::Message(format!(
            "No executable found automatically in \"{dir_name}\". Add the \"executable\" key in ports.json for this port."
        ))),
        _ => {
            candidates.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            let names: Vec<&str> =
                candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("")).collect();
            Err(ExecutableSelectionError::Ambiguous(
                format!("Multiple possible executables in \"{dir_name}\": {}. Please choose one.", names.join(", ")),
                candidates,
            ))
        }
    }
}

/// Chemin absolu de l'exécutable à lancer. Si "executable" n'est pas précisé
/// (ou vide/nul, traité comme "non précisé"), le déduit du contenu du dossier
/// du jeu via `autodetect_executable`.
pub fn resolve_executable(executable: Option<&Value>, game_dir: &Path) -> Result<PathBuf, ExecutableSelectionError> {
    if let Some(exe) = executable {
        if is_truthy(exe) {
            let resolved = resolve_per_platform(exe);
            return match resolved.as_ref().and_then(Value::as_str) {
                Some(s) => safe_join(game_dir, s).map_err(ExecutableSelectionError::Message),
                None => Err(ExecutableSelectionError::Message(
                    "\"executable\" n'est pas un chemin exploitable pour ce port".to_string(),
                )),
            };
        }
    }
    autodetect_executable(game_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_test_{}_{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn autodetect_un_seul_candidat() {
        let dir = temp_dir("autodetect_one");
        std::fs::write(dir.join("game.exe"), b"").unwrap();
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, dir.join("game.exe"));
    }

    #[test]
    fn autodetect_exclut_les_desinstalleurs() {
        let dir = temp_dir("autodetect_uninstaller");
        std::fs::write(dir.join("unins000.exe"), b"").unwrap();
        std::fs::write(dir.join("game.exe"), b"").unwrap();
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, dir.join("game.exe"));
    }

    #[test]
    fn autodetect_detecte_un_raccourci_lnk() {
        let dir = temp_dir("autodetect_lnk");
        std::fs::write(dir.join("game.lnk"), b"").unwrap();
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, dir.join("game.lnk"));
    }

    #[test]
    fn autodetect_exe_et_lnk_sont_ambigus_ensemble() {
        let dir = temp_dir("autodetect_exe_et_lnk");
        std::fs::write(dir.join("game.exe"), b"").unwrap();
        std::fs::write(dir.join("game.lnk"), b"").unwrap();
        match autodetect_executable(&dir) {
            Err(ExecutableSelectionError::Ambiguous(_, candidates)) => assert_eq!(candidates.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    #[test]
    fn autodetect_zero_candidat_est_une_erreur() {
        let dir = temp_dir("autodetect_zero");
        assert!(matches!(autodetect_executable(&dir), Err(ExecutableSelectionError::Message(_))));
    }

    #[test]
    fn autodetect_plusieurs_candidats_est_ambigu() {
        let dir = temp_dir("autodetect_many");
        std::fs::write(dir.join("a.exe"), b"").unwrap();
        std::fs::write(dir.join("b.exe"), b"").unwrap();
        match autodetect_executable(&dir) {
            Err(ExecutableSelectionError::Ambiguous(_, candidates)) => assert_eq!(candidates.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }
}
