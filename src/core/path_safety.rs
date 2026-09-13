//! Vérification qu'un chemin construit à partir d'une donnée potentiellement
//! non fiable (`ports.json` édité à la main, chemin d'archive) reste bien
//! sous le dossier prévu.

use std::path::{Path, PathBuf};

/// Normalisation purement lexicale (`.`/`..` résolus par composant, jamais
/// une vérification sur le disque) -- contrairement à
/// `std::fs::canonicalize`, fonctionne même si `path` n'existe pas encore
/// (cas courant : `dest_dir` d'une toute première installation).
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `base / relative`, avec vérification que le résultat reste bien SOUS
/// `base` une fois résolu. `relative` vient de données potentiellement non
/// fiables (`ports.json` édité à la main, chemins d'archive) : un chemin
/// absolu ou des `..` pourrait sinon faire sortir une opération de fichier
/// du dossier prévu. Normalisation PUIS vérification -- pas de rejet a
/// priori sur la forme de la chaîne : joindre un composant absolu à `base`
/// REMPLACE silencieusement `base` (comportement documenté de `Path::join`,
/// identique à `pathlib`), donc seule une vérification a posteriori sur le
/// résultat normalisé est fiable.
pub fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, String> {
    let base_norm = lexical_normalize(base);
    let target = lexical_normalize(&base.join(relative));
    if target != base_norm && !target.starts_with(&base_norm) {
        return Err(format!("\"{relative}\" sort de \"{}\"", base.display()));
    }
    Ok(target)
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
    fn safe_join_rejette_un_chemin_absolu_qui_ecrase_base() {
        let base = temp_dir("safe_join_absolute");
        let evil = if cfg!(windows) { "C:\\Windows\\evil" } else { "/etc/evil" };
        assert!(safe_join(&base, evil).is_err());
    }
}
