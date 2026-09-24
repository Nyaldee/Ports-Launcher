
use super::path_safety::safe_join;
use super::platform_resolve::{is_truthy, resolve_per_platform};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn is_uninstaller_name(name_lower: &str) -> bool {
    name_lower.contains("unins")
}

#[derive(Debug)]
pub enum ExecutableSelectionError {
    NotFound,
    Ambiguous(Vec<PathBuf>),
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

fn collect_candidates(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_candidates(&path, out),
            Ok(t) if t.is_file() => {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
                if is_candidate(&path) && !is_uninstaller_name(&name) {
                    out.push(path);
                }
            }
            _ => {}
        }
    }
}

pub fn autodetect_executable(game_dir: &Path) -> Result<PathBuf, ExecutableSelectionError> {
    let mut candidates = Vec::new();
    collect_candidates(game_dir, &mut candidates);
    match candidates.len() {
        0 => Err(ExecutableSelectionError::NotFound),
        1 => Ok(candidates.remove(0)),
        _ => {
            candidates.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            Err(ExecutableSelectionError::Ambiguous(candidates))
        }
    }
}

pub fn resolve_executable(executable: Option<&Value>, game_dir: &Path) -> Result<PathBuf, ExecutableSelectionError> {
    match executable.filter(|v| is_truthy(v)) {
        Some(exe) => resolve_per_platform(exe)
            .as_ref()
            .and_then(Value::as_str)
            .and_then(|s| safe_join(game_dir, s).ok())
            .ok_or(ExecutableSelectionError::NotFound),
        None => autodetect_executable(game_dir),
    }
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
        assert_eq!(autodetect_executable(&dir).unwrap(), game);
    }

    #[test]
    fn autodetect_exclut_les_desinstalleurs() {
        let dir = temp_dir("autodetect_uninstaller");
        write_candidate(&dir, "unins000");
        let game = write_candidate(&dir, "game");
        assert_eq!(autodetect_executable(&dir).unwrap(), game);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn autodetect_detecte_un_raccourci_lnk() {
        let dir = temp_dir("autodetect_lnk");
        std::fs::write(dir.join("game.lnk"), b"").unwrap();
        assert_eq!(autodetect_executable(&dir).unwrap(), dir.join("game.lnk"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn autodetect_exe_et_lnk_sont_ambigus_ensemble() {
        let dir = temp_dir("autodetect_exe_et_lnk");
        std::fs::write(dir.join("game.exe"), b"").unwrap();
        std::fs::write(dir.join("game.lnk"), b"").unwrap();
        match autodetect_executable(&dir) {
            Err(ExecutableSelectionError::Ambiguous(candidates)) => assert_eq!(candidates.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn autodetect_detecte_un_script_sh_sans_bit_executable() {
        let dir = temp_dir("autodetect_sh");
        let script = dir.join("game.sh");
        std::fs::write(&script, b"").unwrap();
        assert_eq!(autodetect_executable(&dir).unwrap(), script);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn autodetect_binaire_et_script_sh_sont_ambigus_ensemble() {
        let dir = temp_dir("autodetect_bin_et_sh");
        write_candidate(&dir, "game");
        std::fs::write(dir.join("launch.sh"), b"").unwrap();
        match autodetect_executable(&dir) {
            Err(ExecutableSelectionError::Ambiguous(candidates)) => assert_eq!(candidates.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    #[test]
    fn autodetect_zero_candidat_est_une_erreur() {
        let dir = temp_dir("autodetect_zero");
        assert!(matches!(autodetect_executable(&dir), Err(ExecutableSelectionError::NotFound)));
    }

    #[test]
    fn autodetect_plusieurs_candidats_est_ambigu() {
        let dir = temp_dir("autodetect_many");
        write_candidate(&dir, "a");
        write_candidate(&dir, "b");
        match autodetect_executable(&dir) {
            Err(ExecutableSelectionError::Ambiguous(candidates)) => assert_eq!(candidates.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    #[test]
    fn resolve_executable_refuse_un_chemin_hors_du_dossier() {
        let dir = temp_dir("resolve_escape");
        let exe = serde_json::json!("../outside.exe");
        assert!(matches!(resolve_executable(Some(&exe), &dir), Err(ExecutableSelectionError::NotFound)));
    }
}
