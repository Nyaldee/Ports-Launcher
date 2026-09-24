
use std::path::{Component, Path, PathBuf};

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, String> {
    let target = lexical_normalize(&base.join(relative));
    if !is_within(&target, base) {
        return Err(format!("\"{relative}\" points outside \"{}\"", base.display()));
    }
    Ok(target)
}

pub fn is_within(candidate: &Path, base: &Path) -> bool {
    lexical_normalize(candidate).starts_with(lexical_normalize(base))
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
    fn safe_join_accepte_base_elle_meme_et_un_descendant() {
        let base = temp_dir("safe_join_ok");
        assert_eq!(safe_join(&base, ".").unwrap(), lexical_normalize(&base));
        assert_eq!(safe_join(&base, "sub/dir").unwrap(), base.join("sub").join("dir"));
    }

    #[test]
    fn safe_join_rejette_une_sortie_via_dotdot() {
        let base = temp_dir("safe_join_dotdot");
        assert!(safe_join(&base, "../evil").is_err());
        assert!(safe_join(&base, "sub/../../evil").is_err());
    }

    #[test]
    fn safe_join_rejette_un_chemin_absolu() {
        let base = temp_dir("safe_join_absolute");
        let evil = if cfg!(windows) { "C:\\Windows\\evil" } else { "/etc/evil" };
        assert!(safe_join(&base, evil).is_err());
    }

    #[test]
    fn is_within_detecte_une_sortie_via_dotdot_meme_non_normalisee() {
        let base = Path::new("C:\\Games\\MyGame");
        let escaping = base.join("..").join("Escaped");
        assert!(!is_within(&escaping, base));
        assert!(is_within(&base.join("Save"), base));
        assert!(is_within(base, base));
    }
}
