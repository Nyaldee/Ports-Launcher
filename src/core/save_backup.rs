
use super::installer::{run_7z, sevenzip_exe_path};
use super::models::Port;
use super::path_safety::{is_within, safe_join};
use super::platform_resolve::resolve_save_folder;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PENDING_RESTORE_DIR: &str = "Pending Restore";
const GLOBAL_BACKUPS_DIR: &str = "Global Backups";

fn save_fields(port: &Port) -> [(Option<&Value>, &'static str); 2] {
    [(port.save.as_ref(), "save_folder"), (port.save2.as_ref(), "save_folder2")]
}

fn copy_non_empty(src: &Path, dst: &Path) -> io::Result<bool> {
    let mut copied_any = false;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copied_any |= copy_non_empty(&entry.path(), &dst.join(entry.file_name()))?;
        } else if file_type.is_file() {
            fs::create_dir_all(dst)?;
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
            copied_any = true;
        }
    }
    Ok(copied_any)
}

pub(crate) fn pending_restore_dir(saves_backup_dir: &Path, folder_name: &str, field: &str) -> PathBuf {
    saves_backup_dir.join(PENDING_RESTORE_DIR).join(folder_name).join(field)
}

#[must_use]
fn preserve_before_uninstall(save_folder: Option<&Value>, saves_backup_dir: &Path, dest_dir: &Path, folder_name: &str, field: &str) -> bool {
    let Some(src) = save_folder.and_then(|v| resolve_save_folder(v, dest_dir)) else { return true };
    if !is_within(&src, dest_dir) || !src.exists() {
        return true;
    }
    let dst = pending_restore_dir(saves_backup_dir, folder_name, field);
    if !is_within(&dst, saves_backup_dir) {
        return false;
    }
    let _ = fs::remove_dir_all(&dst);
    copy_non_empty(&src, &dst).is_ok()
}

fn restore_after_install(save_folder: Option<&Value>, saves_backup_dir: &Path, dest_dir: &Path, folder_name: &str, field: &str) {
    let src = pending_restore_dir(saves_backup_dir, folder_name, field);
    if !is_within(&src, saves_backup_dir) || !src.exists() {
        return;
    }
    if let Some(dst) = save_folder.and_then(|v| resolve_save_folder(v, dest_dir)) {
        if !is_within(&dst, dest_dir) {
            return;
        }
        let _ = fs::remove_dir_all(&dst);
        if copy_non_empty(&src, &dst).is_err() {
            return;
        }
    }
    let _ = fs::remove_dir_all(&src);
}

#[must_use]
pub fn preserve_all_before_uninstall(port: &Port, saves_backup_dir: &Path, dest_dir: &Path) -> bool {
    let results = save_fields(port).map(|(save_folder, field)| preserve_before_uninstall(save_folder, saves_backup_dir, dest_dir, &port.folder, field));
    results.iter().all(|&ok| ok)
}

pub fn restore_all_after_install(port: &Port, saves_backup_dir: &Path, dest_dir: &Path) {
    for (save_folder, field) in save_fields(port) {
        restore_after_install(save_folder, saves_backup_dir, dest_dir, &port.folder, field);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GlobalBackupSummary {
    pub copied: usize,
    pub skipped: usize,
    pub failed: usize,
}

fn zip_staging_dir(staging: &Path, zip_path: &Path) -> Result<(), String> {
    let tool = sevenzip_exe_path().ok_or("7-Zip is required next to the application to create this backup (missing).")?;
    if let Some(parent) = zip_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(zip_path);
    run_7z(&tool, &[OsStr::new("a"), OsStr::new("-tzip"), zip_path.as_os_str(), OsStr::new(".")], "create", Some(staging))?;
    Ok(())
}

pub fn run_global_backup(
    catalog: &[Port],
    library_dir: &Path,
    saves_backup_dir: &Path,
    date: &str,
    on_progress: &mut dyn FnMut(&str),
) -> GlobalBackupSummary {
    let mut summary = GlobalBackupSummary::default();
    if fs::create_dir_all(saves_backup_dir).is_err() {
        return summary;
    }
    let Ok(staging) = tempfile::Builder::new().prefix("_backup_").tempdir_in(saves_backup_dir) else {
        return summary;
    };

    for port in catalog {
        on_progress(&port.name);
        let Ok(game_dir) = safe_join(library_dir, &port.folder) else { continue };
        let Ok(port_staging) = safe_join(staging.path(), &port.folder) else { continue };
        for (save_folder, field) in save_fields(port) {
            match save_folder.and_then(|v| resolve_save_folder(v, &game_dir)).filter(|src| src.exists()) {
                Some(src) => match copy_non_empty(&src, &port_staging.join(field)) {
                    Ok(true) => summary.copied += 1,
                    Ok(false) => summary.skipped += 1,
                    Err(_) => summary.failed += 1,
                },
                None => summary.skipped += 1,
            }
        }
    }

    if summary.copied > 0 {
        let zip_path = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join(format!("{date}.zip"));
        if zip_staging_dir(staging.path(), &zip_path).is_err() {
            summary.failed += summary.copied;
            summary.copied = 0;
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_save_backup_test_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn port_with_save_folders(folder_name: &str, save: Option<&str>, save2: Option<&str>) -> Port {
        let mut value = serde_json::json!({"name": folder_name, "folder": folder_name, "source": "https://example.com/x.zip"});
        if let Some(s) = save {
            value["save"] = s.into();
        }
        if let Some(s) = save2 {
            value["save2"] = s.into();
        }
        super::super::models::port_from_value(&value).unwrap()
    }

    #[test]
    fn copy_non_empty_copie_les_fichiers_et_larborescence() {
        let dir = temp_dir("copy_basic");
        let src = dir.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.dat"), b"a").unwrap();
        fs::write(src.join("sub").join("b.dat"), b"b").unwrap();

        let dst = dir.join("dst");
        assert!(copy_non_empty(&src, &dst).unwrap());
        assert_eq!(fs::read_to_string(dst.join("a.dat")).unwrap(), "a");
        assert_eq!(fs::read_to_string(dst.join("sub").join("b.dat")).unwrap(), "b");
    }

    #[test]
    fn copy_non_empty_ignore_les_sous_dossiers_vides_et_ne_cree_rien_si_tout_est_vide() {
        let dir = temp_dir("copy_empty");
        let src = dir.join("src");
        fs::create_dir_all(src.join("empty_sub")).unwrap();
        let dst = dir.join("dst");
        assert!(!copy_non_empty(&src, &dst).unwrap());
        assert!(!dst.exists());
    }

    #[test]
    fn preserve_before_uninstall_echoue_sans_perdre_l_original_si_la_copie_echoue() {
        let dir = temp_dir("preserve_copy_fails");
        let saves_backup_dir = dir.join("Saves Backup");
        fs::write(&saves_backup_dir, b"blocks Saves Backup from being a directory").unwrap();
        let port = port_with_save_folders("MyGame", Some("Save"), None);
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();

        assert!(!preserve_before_uninstall(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder"));
        assert_eq!(fs::read_to_string(dest_dir.join("Save").join("slot1.dat")).unwrap(), "precious");
    }

    #[test]
    fn restore_after_install_garde_la_copie_si_la_restauration_echoue() {
        let dir = temp_dir("restore_copy_fails");
        let saves_backup_dir = dir.join("Saves Backup");
        let port = port_with_save_folders("MyGame", Some("Save"), None);
        let backup = pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder");
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("slot1.dat"), b"precious").unwrap();

        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(dest_dir.join("Save"), b"blocks Save from being a directory").unwrap();

        restore_after_install(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder");
        assert_eq!(fs::read_to_string(backup.join("slot1.dat")).unwrap(), "precious");
    }

    #[test]
    fn preserve_puis_restore_aller_retour() {
        let dir = temp_dir("round_trip");
        let saves_backup_dir = dir.join("Saves Backup");
        let port = port_with_save_folders("MyGame", Some("Save"), None);
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();

        assert!(preserve_before_uninstall(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder"));
        assert_eq!(fs::read_to_string(pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").join("slot1.dat")).unwrap(), "precious");

        fs::remove_dir_all(&dest_dir).unwrap();
        fs::create_dir_all(&dest_dir).unwrap();
        restore_after_install(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder");

        assert_eq!(fs::read_to_string(dest_dir.join("Save").join("slot1.dat")).unwrap(), "precious");
        assert!(!pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").exists());
    }

    #[test]
    fn restore_after_install_refuse_un_save_folder_qui_sort_de_dest_dir() {
        let dir = temp_dir("restore_escape");
        let saves_backup_dir = dir.join("Saves Backup");
        let backup = pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder");
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("slot1.dat"), b"precious").unwrap();

        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(&dest_dir).unwrap();
        let outside = dir.join("Escaped");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("do_not_delete.dat"), b"unrelated").unwrap();

        let escaping_port = port_with_save_folders("MyGame", Some("../Escaped"), None);
        restore_after_install(escaping_port.save.as_ref(), &saves_backup_dir, &dest_dir, "MyGame", "save_folder");

        assert_eq!(fs::read_to_string(outside.join("do_not_delete.dat")).unwrap(), "unrelated");
        assert_eq!(fs::read_to_string(backup.join("slot1.dat")).unwrap(), "precious");
    }

    #[test]
    fn preserve_before_uninstall_refuse_un_folder_name_qui_sort_de_saves_backup_dir() {
        let dir = temp_dir("preserve_folder_name_escape");
        let saves_backup_dir = dir.join("Saves Backup");
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();
        let port = port_with_save_folders("MyGame", Some("Save"), None);

        assert!(!preserve_before_uninstall(port.save.as_ref(), &saves_backup_dir, &dest_dir, "../../Escaped", "save_folder"));
        assert!(!dir.join("Escaped").exists());
    }

    #[test]
    fn restore_after_install_refuse_un_folder_name_qui_sort_de_saves_backup_dir() {
        let dir = temp_dir("restore_folder_name_escape");
        let saves_backup_dir = dir.join("Saves Backup");
        let outside = dir.join("Escaped");
        fs::create_dir_all(outside.join("save_folder")).unwrap();
        fs::write(outside.join("save_folder").join("slot1.dat"), b"unrelated").unwrap();

        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(&dest_dir).unwrap();
        let port = port_with_save_folders("MyGame", Some("Save"), None);

        restore_after_install(port.save.as_ref(), &saves_backup_dir, &dest_dir, "../../Escaped", "save_folder");

        assert!(!dest_dir.join("Save").exists());
        assert_eq!(fs::read_to_string(outside.join("save_folder").join("slot1.dat")).unwrap(), "unrelated");
    }

    #[test]
    fn preserve_ignore_un_save_folder_interne_jamais_cree() {
        let dir = temp_dir("preserve_never_created");
        let saves_backup_dir = dir.join("Saves Backup");
        let port = port_with_save_folders("MyGame", Some("saves"), None);
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(&dest_dir).unwrap();

        assert!(preserve_before_uninstall(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder"));
        assert!(!pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").exists());
    }

    #[test]
    fn preserve_ignore_une_sauvegarde_externe_au_dossier_du_jeu() {
        let dir = temp_dir("preserve_external");
        let saves_backup_dir = dir.join("Saves Backup");
        let external_save = dir.join("external_save");
        fs::create_dir_all(&external_save).unwrap();
        fs::write(external_save.join("slot1.dat"), b"precious").unwrap();
        let port = port_with_save_folders("MyGame", Some(external_save.to_str().unwrap()), None);
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(&dest_dir).unwrap();

        assert!(preserve_before_uninstall(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder"));
        assert!(external_save.join("slot1.dat").exists());
        assert!(!pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").exists());
    }

    fn read_zip_entry(zip_path: &Path, entry_path: &str) -> String {
        let tool = sevenzip_exe_path().unwrap();
        let out_dir = zip_path.with_extension("extracted");
        let _ = fs::remove_dir_all(&out_dir);
        let out_arg = format!("-o{}", out_dir.display());
        let output = std::process::Command::new(&tool).arg("x").arg(zip_path).arg(&out_arg).arg("-y").output().unwrap();
        assert!(output.status.success(), "échec d'extraction du zip : {}", String::from_utf8_lossy(&output.stdout));
        fs::read_to_string(out_dir.join(entry_path)).unwrap()
    }

    #[test]
    fn run_global_backup_sauvegarde_un_port_non_installe_via_un_save_folder_externe() {
        let dir = temp_dir("global_not_installed");
        let library_dir = dir.join("Library");
        let saves_backup_dir = dir.join("Saves Backup");
        fs::create_dir_all(&library_dir).unwrap();
        let external_save = dir.join("external_save");
        fs::create_dir_all(&external_save).unwrap();
        fs::write(external_save.join("slot1.dat"), b"precious").unwrap();
        let port = port_with_save_folders("MyGame", Some(external_save.to_str().unwrap()), None);

        let mut seen = Vec::new();
        let summary = run_global_backup(&[port], &library_dir, &saves_backup_dir, "2026-08-23", &mut |name| seen.push(name.to_string()));

        assert_eq!(seen, vec!["MyGame".to_string()]);
        assert_eq!(summary, GlobalBackupSummary { copied: 1, skipped: 1, failed: 0 });
        let zip_path = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join("2026-08-23.zip");
        assert_eq!(read_zip_entry(&zip_path, "MyGame/save_folder/slot1.dat"), "precious");
    }

    #[test]
    fn run_global_backup_resout_un_save_folder_relatif_contre_library() {
        let dir = temp_dir("global_relative");
        let library_dir = dir.join("Library");
        let saves_backup_dir = dir.join("Saves Backup");
        let game_dir = library_dir.join("MyGame");
        fs::create_dir_all(game_dir.join("saves")).unwrap();
        fs::write(game_dir.join("saves").join("slot1.dat"), b"precious").unwrap();
        let port = port_with_save_folders("MyGame", None, Some("saves"));

        let summary = run_global_backup(&[port], &library_dir, &saves_backup_dir, "2026-08-23", &mut |_| {});

        assert_eq!(summary, GlobalBackupSummary { copied: 1, skipped: 1, failed: 0 });
        let zip_path = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join("2026-08-23.zip");
        assert_eq!(read_zip_entry(&zip_path, "MyGame/save_folder2/slot1.dat"), "precious");
    }

    #[test]
    fn run_global_backup_second_export_le_meme_jour_remplace_le_premier() {
        let dir = temp_dir("global_same_day_overwrite");
        let library_dir = dir.join("Library");
        let saves_backup_dir = dir.join("Saves Backup");
        fs::create_dir_all(&library_dir).unwrap();

        let external_a = dir.join("external_a");
        fs::create_dir_all(&external_a).unwrap();
        fs::write(external_a.join("slot1.dat"), b"a").unwrap();
        let port_a = port_with_save_folders("GameA", Some(external_a.to_str().unwrap()), None);
        run_global_backup(&[port_a], &library_dir, &saves_backup_dir, "2026-08-23", &mut |_| {});

        let external_b = dir.join("external_b");
        fs::create_dir_all(&external_b).unwrap();
        fs::write(external_b.join("slot1.dat"), b"b").unwrap();
        let port_b = port_with_save_folders("GameB", Some(external_b.to_str().unwrap()), None);
        run_global_backup(&[port_b], &library_dir, &saves_backup_dir, "2026-08-23", &mut |_| {});

        let zip_path = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join("2026-08-23.zip");
        assert_eq!(read_zip_entry(&zip_path, "GameB/save_folder/slot1.dat"), "b");
        assert!(!zip_path.with_extension("extracted").join("GameA").exists());
    }
}
