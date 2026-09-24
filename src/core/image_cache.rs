
use super::path_safety::safe_join;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn cached_image_path(cache_dir: &Path, folder_name: &str) -> Result<PathBuf, String> {
    safe_join(cache_dir, &format!("{folder_name}.png"))
}

pub fn cache_image(url: &str, cache_dir: &Path, folder_name: &str) {
    let Ok(dest) = cached_image_path(cache_dir, folder_name) else { return };
    if dest.exists() {
        return;
    }
    let Ok(mut resp) = super::http::agent(Duration::from_secs(10)).get(url).call() else { return };
    let Ok(bytes) = resp.body_mut().read_to_vec() else { return };
    if image::guess_format(&bytes).is_err() {
        return;
    }
    if let Some(parent) = dest.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = super::files::write_atomic(&dest, &bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_image_cache_test_{}_{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn cached_image_path_rejette_une_sortie_de_cache_dir() {
        let dir = temp_dir("path_escape");
        assert!(cached_image_path(&dir, "../evil").is_err());
    }

    #[test]
    fn cached_image_path_autorise_un_sous_dossier() {
        let dir = temp_dir("path_subdir");
        let p = cached_image_path(&dir, "Sub/Game").unwrap();
        assert_eq!(p, dir.join("Sub").join("Game.png"));
    }

    #[test]
    fn cache_image_ne_touche_pas_au_reseau_si_deja_en_cache() {
        let dir = temp_dir("already_cached");
        let dest = cached_image_path(&dir, "Game").unwrap();
        std::fs::write(&dest, b"already-here").unwrap();
        cache_image("http://127.0.0.1:1/unreachable.png", &dir, "Game");
        assert_eq!(std::fs::read(&dest).unwrap(), b"already-here");
    }
}
