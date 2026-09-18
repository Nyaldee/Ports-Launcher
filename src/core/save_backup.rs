
use super::models::Port;
use super::path_safety::{is_within, safe_join};
use super::platform_resolve::resolve_save_folder;
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const PENDING_RESTORE_DIR: &str = "Pending Restore";
pub(crate) const GLOBAL_BACKUPS_DIR: &str = "Global Backups";

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
pub fn preserve_before_uninstall(save_folder: Option<&Value>, saves_backup_dir: &Path, dest_dir: &Path, folder_name: &str, field: &str) -> bool {
    let Some(save_folder) = save_folder else { return true };
    let Some(src) = resolve_save_folder(save_folder, dest_dir) else { return true };
    if !is_within(&src, dest_dir) {
        return true;
    }
    if !src.exists() {
        return true;
    }
    let dst = pending_restore_dir(saves_backup_dir, folder_name, field);
    let _ = fs::remove_dir_all(&dst);
    copy_non_empty(&src, &dst).is_ok()
}

pub fn restore_after_install(save_folder: Option<&Value>, saves_backup_dir: &Path, dest_dir: &Path, folder_name: &str, field: &str) {
    let src = pending_restore_dir(saves_backup_dir, folder_name, field);
    if !src.exists() {
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
    let mut all_preserved = true;
    for (save_folder, field) in [(port.save.as_ref(), "save_folder"), (port.save2.as_ref(), "save_folder2")] {
        all_preserved &= preserve_before_uninstall(save_folder, saves_backup_dir, dest_dir, &port.folder, field);
    }
    all_preserved
}

pub fn restore_all_after_install(port: &Port, saves_backup_dir: &Path, dest_dir: &Path) {
    for (save_folder, field) in [(port.save.as_ref(), "save_folder"), (port.save2.as_ref(), "save_folder2")] {
        restore_after_install(save_folder, saves_backup_dir, dest_dir, &port.folder, field);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GlobalBackupSummary {
    pub copied: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub fn run_global_backup(
    catalog: &[Port],
    library_dir: &Path,
    saves_backup_dir: &Path,
    date: &str,
    on_progress: &mut dyn FnMut(&str),
) -> GlobalBackupSummary {
    let mut summary = GlobalBackupSummary::default();
    for port in catalog {
        on_progress(&port.name);
        let Ok(game_dir) = safe_join(library_dir, &port.folder) else { continue };
        for (save_folder, field) in [(port.save.as_ref(), "save_folder"), (port.save2.as_ref(), "save_folder2")] {
            let dst = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join(date).join(&port.folder).join(field);
            match save_folder.and_then(|v| resolve_save_folder(v, &game_dir)).filter(|src| src.exists()) {
                Some(src) => match copy_non_empty(&src, &dst) {
                    Ok(true) => summary.copied += 1,
                    Ok(false) => summary.skipped += 1,
                    Err(_) => summary.failed += 1,
                },
                None => summary.skipped += 1,
            }
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::SourceType;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_save_backup_test_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn port_with_save_folders(folder_name: &str, save_folder: Option<&str>, save_folder2: Option<&str>) -> Port {
        Port {
            name: folder_name.into(),
            name_lower: folder_name.to_lowercase(),
            tags: vec![],
            tags_lower: vec![],
            source: Value::String("s".into()),
            folder: folder_name.into(),
            executable: None,
            website: None,
            instructions: String::new(),
            mods: None,
            image: None,
            icon: None,
            save: save_folder.map(|s| Value::String(s.into())),
            save2: save_folder2.map(|s| Value::String(s.into())),
            source_type: SourceType::DirectUrl,
            repo: None,
            exe_is_archive: None,
            preferred_asset: None,
            extra: None,
            user_managed: false,
        }
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
    fn restore_after_install_garde_le_slot_pending_restore_si_la_copie_echoue() {
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
    fn preserve_puis_restore_round_trip() {
        let dir = temp_dir("round_trip");
        let saves_backup_dir = dir.join("Saves Backup");
        let port = port_with_save_folders("MyGame", Some("Save"), None);
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();

        assert!(preserve_before_uninstall(port.save.as_ref(), &saves_backup_dir, &dest_dir, &port.folder, "save_folder"));
        assert_eq!(
            fs::read_to_string(pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").join("slot1.dat")).unwrap(),
            "precious"
        );

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
        let backed_up = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join("2026-08-23").join("MyGame").join("save_folder").join("slot1.dat");
        assert_eq!(fs::read_to_string(backed_up).unwrap(), "precious");
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
        let backed_up = saves_backup_dir.join(GLOBAL_BACKUPS_DIR).join("2026-08-23").join("MyGame").join("save_folder2").join("slot1.dat");
        assert_eq!(fs::read_to_string(backed_up).unwrap(), "precious");
    }
}
