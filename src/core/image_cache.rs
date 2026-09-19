
use super::path_safety::safe_join;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

static CACHE: LazyLock<Mutex<HashMap<String, Vec<u8>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, Vec<u8>>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

fn get_image_bytes(url: &str) -> Result<Vec<u8>, String> {
    if let Some(bytes) = lock().get(url) {
        return Ok(bytes.clone());
    }
    let mut resp =
        super::http::agent(std::time::Duration::from_secs(10)).get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.body_mut().as_reader().read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    lock().insert(url.to_string(), bytes.clone());
    Ok(bytes)
}

fn release_image_bytes(url: &str) {
    lock().remove(url);
}

pub fn cached_image_path(cache_dir: &Path, folder_name: &str) -> Result<PathBuf, String> {
    safe_join(cache_dir, &format!("{folder_name}.png"))
}

pub fn cache_image(url: &str, cache_dir: &Path, folder_name: &str) {
    if let Ok(dest) = cached_image_path(cache_dir, folder_name) {
        if dest.exists() {
            return;
        }
    }
    let Ok(data) = get_image_bytes(url) else { return };
    let Ok(dest) = cached_image_path(cache_dir, folder_name) else { return };
    if let Some(parent) = dest.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if std::fs::write(&dest, &data).is_ok() {
        release_image_bytes(url);
    }
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
