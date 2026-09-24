
use std::fs;
use std::io;
use std::path::Path;

pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_remplace_le_contenu_sans_laisser_de_fichier_temporaire() {
        let dir = std::env::temp_dir().join(format!("ports_launcher_files_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        fs::write(&path, b"old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(!dir.join("state.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
