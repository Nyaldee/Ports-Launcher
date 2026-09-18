
use super::path_safety::safe_join;
use super::platform_resolve::{is_truthy, resolve_per_platform};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn is_uninstaller_name(name_lower: &str) -> bool {
    name_lower.contains("unins")
}

#[derive(Debug)]
pub enum ExecutableSelectionError {
    Message(String),
    Ambiguous(#[allow(dead_code)] String, Vec<PathBuf>),
}

#[cfg(target_os = "windows")]
fn is_candidate(path: &Path) -> bool {
    const CANDIDATE_EXTENSIONS: &[&str] = &["exe", "lnk", "bat", "url"];
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| CANDIDATE_EXTENSIONS.iter().any(|c| e.eq_ignore_ascii_case(c)))
}

#[cfg(target_os = "linux")]
fn is_candidate(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let executable_bit = std::fs::metadata(path).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
    let script_ext = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sh") || e.eq_ignore_ascii_case("appimage"));
    executable_bit || script_ext
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_files_recursive(&path, out),
            Ok(t) if t.is_file() => {
                if !is_candidate(&path) {
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

    #[cfg(target_os = "windows")]
    fn write_candidate(dir: &Path, stem: &str) -> PathBuf {
        let path = dir.join(format!("{stem}.exe"));
        std::fs::write(&path, b"").unwrap();
        path
    }

    #[cfg(target_os = "linux")]
    fn write_candidate(dir: &Path, stem: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(stem);
        std::fs::write(&path, b"").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn autodetect_un_seul_candidat() {
        let dir = temp_dir("autodetect_one");
        let game = write_candidate(&dir, "game");
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, game);
    }

    #[test]
    fn autodetect_exclut_les_desinstalleurs() {
        let dir = temp_dir("autodetect_uninstaller");
        write_candidate(&dir, "unins000");
        let game = write_candidate(&dir, "game");
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, game);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn autodetect_detecte_un_raccourci_lnk() {
        let dir = temp_dir("autodetect_lnk");
        std::fs::write(dir.join("game.lnk"), b"").unwrap();
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, dir.join("game.lnk"));
    }

    #[cfg(target_os = "windows")]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn autodetect_detecte_un_script_sh_sans_bit_executable() {
        let dir = temp_dir("autodetect_sh");
        let script = dir.join("game.sh");
        std::fs::write(&script, b"").unwrap();
        let found = autodetect_executable(&dir).unwrap();
        assert_eq!(found, script);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn autodetect_binaire_et_script_sh_sont_ambigus_ensemble() {
        let dir = temp_dir("autodetect_bin_et_sh");
        write_candidate(&dir, "game");
        std::fs::write(dir.join("launch.sh"), b"").unwrap();
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
        write_candidate(&dir, "a");
        write_candidate(&dir, "b");
        match autodetect_executable(&dir) {
            Err(ExecutableSelectionError::Ambiguous(_, candidates)) => assert_eq!(candidates.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }
}
