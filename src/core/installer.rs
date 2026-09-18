
use super::asset_select::AssetSelectionError;
use super::models::{Port, SourceType};
use super::path_safety::safe_join;
use super::platform_resolve::{resolve_per_platform, resolve_preferred_asset};
use super::save_backup;
use super::{github_api, gitlab_api, image_cache};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_UNCOMPRESSED_SIZE: u64 = 20 * 1024 * 1024 * 1024;

pub type ProgressCallback<'a> = Option<&'a mut dyn FnMut(&str)>;

#[derive(Debug)]
pub enum InstallError {
    Message(String),
    Ambiguous(#[allow(dead_code)] String, Vec<Value>),
}

impl From<AssetSelectionError> for InstallError {
    fn from(e: AssetSelectionError) -> Self {
        match e {
            AssetSelectionError::Message(m) => InstallError::Message(m),
            AssetSelectionError::Ambiguous(m, a) => InstallError::Ambiguous(m, a),
        }
    }
}

trait InstallErrorExt<T> {
    fn install_err(self) -> Result<T, InstallError>;
}

impl<T, E: std::fmt::Display> InstallErrorExt<T> for Result<T, E> {
    fn install_err(self) -> Result<T, InstallError> {
        self.map_err(|e| InstallError::Message(e.to_string()))
    }
}

fn notify(on_progress: &mut ProgressCallback, message: &str) {
    if let Some(cb) = on_progress.as_deref_mut() {
        cb(message);
    }
}

fn safe_download_name(name: &str) -> String {
    Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "download".to_string())
}

fn pixeldrain_file_id(url: &str) -> Option<&str> {
    for prefix in ["https://pixeldrain.com/u/", "http://pixeldrain.com/u/"] {
        if let Some(rest) = url.strip_prefix(prefix) {
            let id = rest.split(['/', '?', '#']).next().unwrap_or(rest);
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn pixeldrain_real_name(id: &str) -> Option<String> {
    let agent = super::http::agent(Duration::from_secs(10));
    let url = format!("https://pixeldrain.com/api/file/{id}/info");
    let mut resp = agent.get(&url).call().ok()?;
    let json: Value = resp.body_mut().read_json().ok()?;
    json.get("name").and_then(Value::as_str).map(str::to_string)
}

fn rename_dest(dest: &Path, filename: &str) -> PathBuf {
    dest.parent().map(|parent| parent.join(safe_download_name(filename))).unwrap_or_else(|| dest.to_path_buf())
}

fn has_known_archive_extension(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".zip", ".tar.gz", ".tgz", ".tar", ".7z", ".rar", ".exe"].iter().any(|ext| lower.ends_with(ext))
}

fn content_disposition_filename(header: &str) -> Option<String> {
    let after = header.split("filename=").nth(1)?.trim_start();
    match after.strip_prefix('"') {
        Some(rest) => rest.split('"').next().map(str::to_string),
        None => after.split(';').next().map(|s| s.trim().to_string()),
    }
}

fn download(url: &str, dest: &Path, on_progress: &mut ProgressCallback) -> Result<PathBuf, InstallError> {
    let mut url = url.to_string();
    let mut dest = dest.to_path_buf();
    if let Some(id) = pixeldrain_file_id(&url) {
        if let Some(real_name) = pixeldrain_real_name(id) {
            dest = rename_dest(&dest, &real_name);
        }
        url = format!("https://pixeldrain.com/api/file/{id}");
    }

    let name = url.rsplit('/').next().unwrap_or(&url).to_string();
    if !has_known_archive_extension(&dest.to_string_lossy()) && has_known_archive_extension(&name) {
        dest = rename_dest(&dest, &name);
    }
    notify(on_progress, &format!("Downloading {name}..."));

    let agent = super::http::agent(Duration::from_secs(30 * 60));
    let mut resp = agent.get(&url).call().install_err()?;
    if !has_known_archive_extension(&dest.to_string_lossy()) {
        if let Some(cd_name) = resp
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .and_then(content_disposition_filename)
            .filter(|n| has_known_archive_extension(n))
        {
            dest = rename_dest(&dest, &cd_name);
        }
    }
    if let Some(final_name) =
        ureq::ResponseExt::get_uri(&resp).path().rsplit('/').next().filter(|n| has_known_archive_extension(n) && *n != name)
    {
        dest = rename_dest(&dest, final_name);
    }
    let mut file = fs::File::create(&dest).install_err()?;
    let mut reader = resp.body_mut().as_reader();
    io::copy(&mut reader, &mut file).install_err()?;
    Ok(dest)
}

fn reject_if_too_large(total_bytes: u64, max_uncompressed_size: u64) -> Result<(), InstallError> {
    if total_bytes > max_uncompressed_size {
        let gb = total_bytes as f64 / 1024f64.powi(3);
        return Err(InstallError::Message(format!(
            "This archive claims to decompress to {gb:.1} GB, which looks like a zip bomb -- refusing to extract it."
        )));
    }
    Ok(())
}

fn flatten_single_wrapper_folder(dir: &Path) -> io::Result<()> {
    loop {
        let entries: Vec<PathBuf> = fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        if entries.len() != 1 || !entries[0].is_dir() {
            return Ok(());
        }
        let wrapper = &entries[0];
        let wrapper_name = wrapper.file_name().unwrap().to_owned();
        let staging = dir.join(format!(".flatten-{}", wrapper_name.to_string_lossy()));
        fs::rename(wrapper, &staging)?;
        for item in fs::read_dir(&staging)?.filter_map(|e| e.ok()) {
            let target = dir.join(item.file_name());
            fs::rename(item.path(), target)?;
        }
        fs::remove_dir(&staging)?;
    }
}

fn merge_into(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)?.filter_map(|e| e.ok()) {
        let item = entry.path();
        let target = dest.join(entry.file_name());
        if item.is_dir() {
            if target.exists() && !target.is_dir() {
                fs::remove_file(&target)?;
            }
            merge_into(&item, &target)?;
        } else {
            if target.is_dir() {
                fs::remove_dir_all(&target)?;
            } else if target.exists() {
                fs::remove_file(&target)?;
            }
            fs::rename(&item, &target)?;
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
const SEVENZIP_CANDIDATES: &[&str] = &["7z.exe"];
#[cfg(target_os = "linux")]
const SEVENZIP_CANDIDATES: &[&str] = &["7zzs"];

fn sevenzip_exe_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for name in SEVENZIP_CANDIDATES {
        let here = dir.join(name);
        if here.is_file() {
            return Some(here);
        }
    }
    let one_up = dir.parent()?;
    for name in SEVENZIP_CANDIDATES {
        let candidate = one_up.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn run_7z(tool: &Path, args: &[&std::ffi::OsStr], action: &str) -> Result<std::process::Output, InstallError> {
    let mut cmd = std::process::Command::new(tool);
    cmd.arg("-sccUTF-8").args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd.output().install_err()?;
    if output.status.code().is_none_or(|c| c >= 2) {
        return Err(InstallError::Message(format!("7-Zip failed to {action} this archive: {}", seven_zip_error(&output.stdout, &output.stderr))));
    }
    Ok(output)
}

fn seven_zip_error(stdout: &[u8], stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .chain(String::from_utf8_lossy(stdout).lines())
        .find(|l| l.contains("ERROR"))
        .map(str::trim)
        .unwrap_or("unknown error")
        .to_string()
}

fn extract_archive_via_7z(archive: &Path, staging: &Path, max_uncompressed_size: u64) -> Result<String, InstallError> {
    use std::ffi::OsStr;
    let tool = sevenzip_exe_path()
        .ok_or_else(|| InstallError::Message("7-Zip is required next to the application to extract this file (missing).".to_string()))?;

    let listing = run_7z(&tool, &[OsStr::new("l"), OsStr::new("-slt"), archive.as_os_str()], "read")?;
    let text = String::from_utf8_lossy(&listing.stdout);
    let kind = text.lines().find_map(|l| l.strip_prefix("Type = ")).unwrap_or("").trim().to_string();
    let total: u64 = text.lines().filter_map(|l| l.strip_prefix("Size = ")).filter_map(|n| n.trim().parse::<u64>().ok()).sum();
    reject_if_too_large(total, max_uncompressed_size)?;

    let is_nsis = kind.eq_ignore_ascii_case("Nsis");
    let out_arg = format!("-o{}", staging.display());
    let mut args = vec![OsStr::new("x"), archive.as_os_str(), OsStr::new(&out_arg), OsStr::new("-y"), OsStr::new("-bd")];
    if is_nsis {
        args.push(OsStr::new("-x!$PLUGINSDIR\\*"));
    }
    run_7z(&tool, &args, "extract")?;

    if kind.eq_ignore_ascii_case("gzip") {
        if let Some(inner) = fs::read_dir(staging).install_err()?.filter_map(|e| e.ok()).map(|e| e.path()).next() {
            run_7z(&tool, &[OsStr::new("x"), inner.as_os_str(), OsStr::new(&out_arg), OsStr::new("-y"), OsStr::new("-bd")], "extract")?;
            fs::remove_file(&inner).install_err()?;
        }
    }
    Ok(kind)
}

fn find_exe_folder(staging: &Path, target: &str) -> Result<PathBuf, InstallError> {
    let mut matches = Vec::new();
    collect_files(staging, &mut matches).install_err()?;

    let target_lower = target.to_lowercase();
    matches.retain(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().contains(&target_lower)));
    match matches.len() {
        0 => Err(InstallError::Message(format!("No file matches \"{target}\" (see \"exe_is_archive\" for this port)."))),
        1 => Ok(matches[0].parent().unwrap_or(staging).to_path_buf()),
        n => Err(InstallError::Message(format!("{n} files match \"{target}\" -- please report this port."))),
    }
}

fn extract_to_staging(archive: &Path, staging: &Path, exe_is_archive: Option<&str>) -> Result<PathBuf, InstallError> {
    let name_lower = archive.to_string_lossy().to_lowercase();
    let is_exe_archive = name_lower.ends_with(".exe") && exe_is_archive.is_some();
    let is_known_archive = is_exe_archive
        || name_lower.ends_with(".zip")
        || name_lower.ends_with(".tar.gz")
        || name_lower.ends_with(".tgz")
        || name_lower.ends_with(".tar")
        || name_lower.ends_with(".7z")
        || name_lower.ends_with(".rar");

    if is_known_archive {
        extract_archive_via_7z(archive, staging, MAX_UNCOMPRESSED_SIZE)?;
        if let Some(target) = exe_is_archive {
            find_exe_folder(staging, target)
        } else {
            flatten_single_wrapper_folder(staging).install_err()?;
            Ok(staging.to_path_buf())
        }
    } else {
        let dest = staging.join(archive.file_name().unwrap_or_default());
        fs::copy(archive, &dest).install_err()?;
        Ok(staging.to_path_buf())
    }
}

fn extract(archive: &Path, dest_dir: &Path, library_dir: &Path, exe_is_archive: Option<&str>, on_progress: &mut ProgressCallback) -> Result<(), InstallError> {
    notify(on_progress, "Extracting...");
    let staging_holder = tempfile::Builder::new().prefix("_staging_").tempdir_in(library_dir).install_err()?;
    let staging = staging_holder.path();
    let merge_root = extract_to_staging(archive, staging, exe_is_archive)?;
    merge_into(&merge_root, dest_dir).install_err()?;
    Ok(())
}

fn install_extra(url: &str, dest_dir: &Path, library_dir: &Path, on_progress: &mut ProgressCallback) -> Result<(), InstallError> {
    notify(on_progress, "Downloading extra files...");
    let tmp = tempfile::Builder::new().prefix("_extra_download_").tempdir_in(library_dir).install_err()?;
    let filename = url.rsplit('/').next().unwrap_or("extra");
    let dest = tmp.path().join(safe_download_name(filename));
    let archive_path = download(url, &dest, on_progress)?;

    let staging_holder = tempfile::Builder::new().prefix("_extra_staging_").tempdir_in(library_dir).install_err()?;
    let staging = staging_holder.path();
    let merge_root = extract_to_staging(&archive_path, staging, None)?;
    merge_into(&merge_root, dest_dir).install_err()?;
    Ok(())
}

pub fn install_extra_only(port: &Port, library_dir: &Path, mut on_progress: ProgressCallback) -> Result<(), InstallError> {
    let Some(url) = port.extra.as_deref() else {
        return Err(InstallError::Message("This port has no \"extra\" files to install.".to_string()));
    };
    let dest_dir = safe_join(library_dir, &port.folder).map_err(InstallError::Message)?;
    if !dest_dir.exists() {
        return Err(InstallError::Message("Install this port first, then add its extra files.".to_string()));
    }
    install_extra(url, &dest_dir, library_dir, &mut on_progress)
}

#[derive(Clone, Copy)]
pub struct InstallPaths<'a> {
    pub library_dir: &'a Path,
    pub cache_dir: &'a Path,
    pub saves_backup_dir: &'a Path,
}

#[derive(Default, Clone, Copy)]
pub struct InstallOverrides<'a> {
    pub asset: Option<&'a Value>,
    pub release: Option<&'a Value>,
}

#[allow(clippy::too_many_arguments)]
fn download_release_asset(
    repo: &str,
    token: Option<&str>,
    overrides: InstallOverrides,
    tmp_path: &Path,
    on_progress: &mut ProgressCallback,
    get_latest_release: impl FnOnce(&str, Option<&str>) -> Result<Value, InstallError>,
    pick_release_asset: impl FnOnce(&Value) -> Result<Value, InstallError>,
    url_field: &str,
) -> Result<(PathBuf, Option<String>), InstallError> {
    let release = match overrides.release {
        Some(r) => r.clone(),
        None => get_latest_release(repo, token)?,
    };
    let asset = match overrides.asset {
        Some(a) => a.clone(),
        None => pick_release_asset(&release)?,
    };
    let installed_tag = release.get("tag_name").and_then(Value::as_str).map(str::to_string);
    let name = asset.get("name").and_then(Value::as_str).unwrap_or("download");
    let dest = tmp_path.join(safe_download_name(name));
    let url = asset
        .get(url_field)
        .and_then(Value::as_str)
        .ok_or_else(|| InstallError::Message(format!("Asset without a \"{url_field}\" field -- please report this port.")))?;
    let archive_path = download(url, &dest, on_progress)?;
    Ok((archive_path, installed_tag))
}

#[cfg(target_os = "windows")]
fn write_website_shortcut(dest_dir: &Path, url: &str) -> io::Result<()> {
    let url = url.replace(['\r', '\n'], "");
    fs::create_dir_all(dest_dir)?;
    fs::write(dest_dir.join("Website.url"), format!("[InternetShortcut]\r\nURL={url}\r\n"))
}

#[cfg(target_os = "linux")]
fn write_website_shortcut(dest_dir: &Path, url: &str) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let quoted_url = format!("'{}'", url.replace('\'', "'\\''"));
    fs::create_dir_all(dest_dir)?;
    let path = dest_dir.join("website.sh");
    fs::write(&path, format!("#!/bin/sh\nexec xdg-open {quoted_url}\n"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
}

pub fn install_port(
    port: &Port,
    paths: InstallPaths,
    github_token: Option<&str>,
    gitlab_token: Option<&str>,
    overrides: InstallOverrides,
    mut on_progress: ProgressCallback,
) -> Result<Option<String>, InstallError> {
    let InstallPaths { library_dir, cache_dir, saves_backup_dir } = paths;
    let dest_dir = safe_join(library_dir, &port.folder).map_err(InstallError::Message)?;
    if port.source_type == SourceType::Local {
        if let Some(url) = port.website_url() {
            write_website_shortcut(&dest_dir, url).map_err(|e| InstallError::Message(e.to_string()))?;
            return Ok(None);
        }
        return Err(InstallError::Message(format!(
            "This port has no download source -- place the game files yourself in \"{}\".",
            dest_dir.display()
        )));
    }
    let mut installed_tag = None;

    let tmp = tempfile::Builder::new().prefix("_download_").tempdir_in(library_dir).install_err()?;
    let tmp_path = tmp.path();
    let preferred_asset = port.preferred_asset.as_ref().and_then(resolve_preferred_asset);

    let archive_path = match port.source_type {
        SourceType::Github => {
            let repo = port.repo.as_deref().unwrap_or_default();
            let (path, tag) = download_release_asset(
                repo,
                github_token,
                overrides,
                tmp_path,
                &mut on_progress,
                |r, t| github_api::get_latest_release(r, t).map_err(InstallError::from),
                |r| github_api::pick_release_asset(r, preferred_asset.as_deref()).map_err(InstallError::from),
                "browser_download_url",
            )?;
            installed_tag = tag;
            path
        }
        SourceType::Gitlab => {
            let repo = port.repo.as_deref().unwrap_or_default();
            let (path, tag) = download_release_asset(
                repo,
                gitlab_token,
                overrides,
                tmp_path,
                &mut on_progress,
                |r, t| gitlab_api::get_latest_release(r, t).map_err(InstallError::from),
                |r| gitlab_api::pick_release_asset(r, preferred_asset.as_deref()).map_err(InstallError::from),
                "url",
            )?;
            installed_tag = tag;
            path
        }
        SourceType::DirectUrl => {
            let resolved = resolve_per_platform(&port.source);
            let url = resolved
                .as_ref()
                .and_then(Value::as_str)
                .ok_or_else(|| InstallError::Message("\"source\" is not a usable link for this port".to_string()))?;
            let filename = url.rsplit('/').next().unwrap_or("download");
            let dest = tmp_path.join(safe_download_name(filename));
            download(url, &dest, &mut on_progress)?
        }
        SourceType::Local => unreachable!("SourceType::Local exclu avant ce match"),
    };

    extract(&archive_path, &dest_dir, library_dir, port.exe_is_archive.as_deref(), &mut on_progress)?;
    save_backup::restore_all_after_install(port, saves_backup_dir, &dest_dir);

    if let Some(url) = &port.image {
        if url.starts_with("http://") || url.starts_with("https://") {
            image_cache::cache_image(url, cache_dir, &port.folder);
        }
    }

    notify(&mut on_progress, "Done.");
    Ok(installed_tag)
}

pub fn uninstall_port(port: &Port, library_dir: &Path, saves_backup_dir: &Path) -> Result<(), String> {
    let dest_dir = safe_join(library_dir, &port.folder)?;
    if dest_dir.exists() {
        if !save_backup::preserve_all_before_uninstall(port, saves_backup_dir, &dest_dir) {
            return Err("Couldn't back up the local save (disk full or permission denied?) -- uninstall cancelled to avoid losing it.".to_string());
        }
        fs::remove_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn is_installed(port: &Port, library_dir: &Path) -> bool {
    safe_join(library_dir, &port.folder).map(|p| p.exists()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ports_launcher_installer_test_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn reject_if_too_large_accepte_sous_la_limite_et_rejette_au_dessus() {
        assert!(reject_if_too_large(MAX_UNCOMPRESSED_SIZE, MAX_UNCOMPRESSED_SIZE).is_ok());
        assert!(reject_if_too_large(MAX_UNCOMPRESSED_SIZE + 1, MAX_UNCOMPRESSED_SIZE).is_err());
    }

    #[test]
    fn pixeldrain_file_id_extrait_un_lien_de_partage() {
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/u/XQjoRAjJ"), Some("XQjoRAjJ"));
        assert_eq!(pixeldrain_file_id("http://pixeldrain.com/u/XQjoRAjJ"), Some("XQjoRAjJ"));
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/u/XQjoRAjJ?x=1"), Some("XQjoRAjJ"));
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/u/XQjoRAjJ/comments"), Some("XQjoRAjJ"));
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/u/XQjoRAjJ#preview"), Some("XQjoRAjJ"));
    }

    #[test]
    fn pixeldrain_file_id_ignore_les_liens_deja_directs_ou_non_pixeldrain() {
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/api/file/XQjoRAjJ"), None);
        assert_eq!(pixeldrain_file_id("https://example.com/u/XQjoRAjJ"), None);
        assert_eq!(pixeldrain_file_id("https://github.com/foo/bar/releases/download/v1/a.zip"), None);
    }

    #[test]
    fn content_disposition_filename_extrait_le_nom_quote() {
        assert_eq!(
            content_disposition_filename("attachment; filename=\"ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo.zip\"; filename*=UTF-8''ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo.zip"),
            Some("ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo.zip".to_string())
        );
    }

    #[test]
    fn content_disposition_filename_gere_le_nom_sans_guillemets() {
        assert_eq!(content_disposition_filename("attachment; filename=a.zip"), Some("a.zip".to_string()));
    }

    #[test]
    fn content_disposition_filename_none_si_absent() {
        assert_eq!(content_disposition_filename("attachment"), None);
    }

    #[test]
    fn has_known_archive_extension_ignore_les_points_dun_numero_de_version() {
        assert!(!has_known_archive_extension("ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo"));
        assert!(has_known_archive_extension("ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo.zip"));
        assert!(has_known_archive_extension("Foo.Bar.v2.5.TAR.GZ"));
    }

    fn build_archive(kind: &str, files: &[(&str, &[u8])], archive_path: &Path) {
        let src = archive_path.parent().unwrap().join("_src");
        let _ = fs::remove_dir_all(&src);
        fs::create_dir_all(&src).unwrap();
        for (rel_path, content) in files {
            let path = src.join(rel_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
        }
        let tool = sevenzip_exe_path().expect("7z.exe introuvable -- requis pour fabriquer les archives de test");
        let output =
            std::process::Command::new(&tool).current_dir(&src).arg("a").arg(format!("-t{kind}")).arg(archive_path).arg(".").output().unwrap();
        assert!(output.status.success(), "échec de fabrication de l'archive de test ({kind}): {}", String::from_utf8_lossy(&output.stdout));
        fs::remove_dir_all(&src).unwrap();
    }

    fn build_tar_gz_archive(files: &[(&str, &[u8])], archive_path: &Path) {
        let tar_path = archive_path.with_extension("");
        build_archive("tar", files, &tar_path);
        let tool = sevenzip_exe_path().unwrap();
        let output = std::process::Command::new(&tool).arg("a").arg("-tgzip").arg(archive_path).arg(&tar_path).output().unwrap();
        assert!(output.status.success());
        fs::remove_file(&tar_path).unwrap();
    }

    #[test]
    fn extract_archive_via_7z_rejette_une_archive_zip_bomb() {
        let dir = temp_dir("zipbomb");
        let archive_path = dir.join("bomb.zip");
        build_archive("zip", &[("a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        assert!(extract_archive_via_7z(&archive_path, &staging, 0).is_err());
    }

    #[test]
    fn extract_archive_via_7z_extrait_un_zip_normalement() {
        let dir = temp_dir("zipok");
        let archive_path = dir.join("ok.zip");
        build_archive("zip", &[("sub/a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        extract_archive_via_7z(&archive_path, &staging, MAX_UNCOMPRESSED_SIZE).unwrap();
        assert_eq!(fs::read_to_string(staging.join("sub").join("a.txt")).unwrap(), "hello world");
    }

    #[test]
    fn extract_archive_via_7z_extrait_un_tar_gz_normalement() {
        let dir = temp_dir("targz");
        let archive_path = dir.join("ok.tar.gz");
        build_tar_gz_archive(&[("sub/a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        extract_archive_via_7z(&archive_path, &staging, MAX_UNCOMPRESSED_SIZE).unwrap();
        assert_eq!(fs::read_to_string(staging.join("sub").join("a.txt")).unwrap(), "hello world");
    }

    #[test]
    fn extract_archive_via_7z_extrait_un_7z_normalement() {
        let dir = temp_dir("sevenz");
        let archive_path = dir.join("ok.7z");
        build_archive("7z", &[("sub/a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        extract_archive_via_7z(&archive_path, &staging, MAX_UNCOMPRESSED_SIZE).unwrap();
        assert_eq!(fs::read_to_string(staging.join("sub").join("a.txt")).unwrap(), "hello world");
    }

    fn build_fake_exe_archive(path: &Path, exe_folder: &str) {
        let exe_path = format!("{exe_folder}/game.exe");
        let dll_path = format!("{exe_folder}/game.dll");
        build_archive(
            "zip",
            &[("_rels/.rels", b"<Relationships/>".as_slice()), ("[Content_Types].xml", b"<Types/>".as_slice()), (&exe_path, b"fake-exe".as_slice()), (&dll_path, b"fake-dll".as_slice())],
            path,
        );
    }

    #[test]
    fn extract_exe_is_archive_ne_garde_que_le_dossier_de_l_exe() {
        let dir = temp_dir("exe_archive_ok");
        let archive_path = dir.join("Install.exe");
        build_fake_exe_archive(&archive_path, "lib/net6.0");

        let dest_dir = dir.join("dest");
        let mut on_progress: ProgressCallback = None;
        extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut on_progress).unwrap();

        assert_eq!(fs::read_to_string(dest_dir.join("game.exe")).unwrap(), "fake-exe");
        assert_eq!(fs::read_to_string(dest_dir.join("game.dll")).unwrap(), "fake-dll");
        assert!(!dest_dir.join("_rels").exists());
        assert!(!dest_dir.join("[Content_Types].xml").exists());
        assert!(!dest_dir.join("lib").exists());
    }

    #[test]
    fn extract_exe_is_archive_echoue_si_l_ancre_matche_plusieurs_fichiers() {
        let dir = temp_dir("exe_archive_ambiguous");
        let archive_path = dir.join("Install.exe");
        build_archive("zip", &[("win64/game.exe", b"a".as_slice()), ("win32/game.exe", b"b".as_slice())], &archive_path);

        let dest_dir = dir.join("dest");
        let mut on_progress: ProgressCallback = None;
        assert!(extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut on_progress).is_err());
    }

    #[test]
    fn extract_exe_is_archive_ancre_leve_l_ambiguite() {
        let dir = temp_dir("exe_archive_base_exe");
        let archive_path = dir.join("Install.exe");
        build_archive("zip", &[("redist/vc_redist.x64.exe", b"redist".as_slice()), ("bin/game.exe", b"the-game".as_slice())], &archive_path);

        let dest_dir = dir.join("dest");
        let mut on_progress: ProgressCallback = None;
        extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut on_progress).unwrap();

        assert_eq!(fs::read_to_string(dest_dir.join("game.exe")).unwrap(), "the-game");
        assert!(!dest_dir.join("vc_redist.x64.exe").exists());
        assert!(!dest_dir.join("redist").exists());
    }

    #[test]
    fn extract_exe_is_archive_echoue_proprement_si_ni_zip_ni_nsis() {
        let dir = temp_dir("exe_archive_neither_format");
        let archive_path = dir.join("Install.exe");
        fs::write(&archive_path, b"not a zip, not an nsis installer either").unwrap();

        let dest_dir = dir.join("dest");
        let mut on_progress: ProgressCallback = None;
        assert!(extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut on_progress).is_err());
    }

    #[test]
    fn extract_route_un_rar_vers_7z_plutot_que_de_le_copier_tel_quel() {
        let dir = temp_dir("rar_routed_to_7z");
        let archive_path = dir.join("release.rar");
        fs::write(&archive_path, b"not a real rar file").unwrap();

        let dest_dir = dir.join("dest");
        let mut on_progress: ProgressCallback = None;
        assert!(extract(&archive_path, &dest_dir, &dir, None, &mut on_progress).is_err());
        assert!(!dest_dir.join("release.rar").exists(), "aurait dû être extrait, pas copié tel quel");
    }

    #[test]
    fn extract_exe_is_archive_absent_copie_l_exe_tel_quel() {
        let dir = temp_dir("exe_not_archive");
        let archive_path = dir.join("portable.exe");
        fs::write(&archive_path, b"MZ-fake-pe-header").unwrap();

        let dest_dir = dir.join("dest");
        let mut on_progress: ProgressCallback = None;
        extract(&archive_path, &dest_dir, &dir, None, &mut on_progress).unwrap();

        assert_eq!(fs::read(dest_dir.join("portable.exe")).unwrap(), b"MZ-fake-pe-header");
    }

    #[test]
    fn flatten_replie_un_seul_dossier_wrapper() {
        let dir = temp_dir("flatten_one");
        fs::create_dir_all(dir.join("wrapper").join("sub")).unwrap();
        fs::write(dir.join("wrapper").join("game.exe"), b"").unwrap();
        fs::write(dir.join("wrapper").join("sub").join("data.bin"), b"").unwrap();
        flatten_single_wrapper_folder(&dir).unwrap();
        assert!(dir.join("game.exe").exists());
        assert!(dir.join("sub").join("data.bin").exists());
        assert!(!dir.join("wrapper").exists());
    }

    #[test]
    fn flatten_gere_la_collision_de_nom_parent_enfant() {
        let dir = temp_dir("flatten_collision");
        fs::create_dir_all(dir.join("Wrapper").join("Wrapper")).unwrap();
        fs::write(dir.join("Wrapper").join("Wrapper").join("game.exe"), b"").unwrap();
        flatten_single_wrapper_folder(&dir).unwrap();
        assert!(dir.join("game.exe").exists());
    }

    #[test]
    fn flatten_boucle_sur_plusieurs_niveaux() {
        let dir = temp_dir("flatten_multi");
        fs::create_dir_all(dir.join("release-v1.0").join("win64")).unwrap();
        fs::write(dir.join("release-v1.0").join("win64").join("game.exe"), b"").unwrap();
        flatten_single_wrapper_folder(&dir).unwrap();
        assert!(dir.join("game.exe").exists());
    }

    #[test]
    fn flatten_ne_fait_rien_si_plusieurs_entrees() {
        let dir = temp_dir("flatten_noop");
        fs::write(dir.join("a.exe"), b"").unwrap();
        fs::write(dir.join("b.exe"), b"").unwrap();
        flatten_single_wrapper_folder(&dir).unwrap();
        assert!(dir.join("a.exe").exists());
        assert!(dir.join("b.exe").exists());
    }

    #[test]
    fn merge_remplace_par_nom_sans_toucher_aux_entrees_absentes_de_src() {
        let dir = temp_dir("merge");
        let src = dir.join("src");
        let dest = dir.join("dest");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("game.exe"), b"new-version").unwrap();
        fs::write(dest.join("game.exe"), b"old-version").unwrap();
        fs::write(dest.join("savegame.dat"), b"my-save").unwrap();

        merge_into(&src, &dest).unwrap();

        assert_eq!(fs::read_to_string(dest.join("game.exe")).unwrap(), "new-version");
        assert_eq!(fs::read_to_string(dest.join("savegame.dat")).unwrap(), "my-save");
    }

    fn port_with_save(folder: &str, save: Option<&str>) -> Port {
        port_with_saves(folder, save, None)
    }

    fn port_with_saves(folder: &str, save: Option<&str>, save2: Option<&str>) -> Port {
        Port {
            name: "n".into(),
            name_lower: "n".into(),
            tags: vec![],
            tags_lower: vec![],
            source: Value::String("s".into()),
            folder: folder.into(),
            executable: None,
            website: None,
            instructions: String::new(),
            mods: None,
            image: None,
            icon: None,
            save: save.map(|s| Value::String(s.into())),
            save2: save2.map(|s| Value::String(s.into())),
            source_type: SourceType::DirectUrl,
            repo: None,
            exe_is_archive: None,
            preferred_asset: None,
            extra: None,
            user_managed: false,
        }
    }

    #[test]
    fn uninstall_port_preserve_une_sauvegarde_locale() {
        let dir = temp_dir("uninstall_preserve_save");
        let saves_backup_dir = dir.join("Saves Backup");
        let port = port_with_save("MyGame", Some("Save"));
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();
        fs::write(dest_dir.join("game.exe"), b"exe").unwrap();

        uninstall_port(&port, &dir, &saves_backup_dir).unwrap();

        assert!(!dest_dir.exists());
        let backed_up = save_backup::pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").join("slot1.dat");
        assert_eq!(fs::read_to_string(backed_up).unwrap(), "precious");
    }

    #[test]
    fn uninstall_port_annule_et_ne_supprime_rien_si_la_preservation_echoue() {
        let dir = temp_dir("uninstall_preserve_fails");
        let saves_backup_dir = dir.join("Saves Backup");
        fs::write(&saves_backup_dir, b"blocks Saves Backup from being a directory").unwrap();
        let port = port_with_save("MyGame", Some("Save"));
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();

        assert!(uninstall_port(&port, &dir, &saves_backup_dir).is_err());

        assert_eq!(fs::read_to_string(dest_dir.join("Save").join("slot1.dat")).unwrap(), "precious");
    }

    #[test]
    fn uninstall_port_ne_touche_pas_une_sauvegarde_externe_au_dossier_du_jeu() {
        let dir = temp_dir("uninstall_external_save");
        let saves_backup_dir = dir.join("Saves Backup");
        let external_save = dir.join("external_save");
        fs::create_dir_all(&external_save).unwrap();
        fs::write(external_save.join("slot1.dat"), b"precious").unwrap();
        let port = port_with_save("MyGame", Some(external_save.to_str().unwrap()));
        fs::create_dir_all(dir.join("MyGame")).unwrap();

        uninstall_port(&port, &dir, &saves_backup_dir).unwrap();

        assert!(external_save.join("slot1.dat").exists());
        assert!(!save_backup::pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").exists());
    }

    #[test]
    fn save_et_save2_ne_s_ecrasent_jamais_meme_avec_des_noms_de_fichier_identiques() {
        let dir = temp_dir("two_saves");
        let saves_backup_dir = dir.join("Saves Backup");
        let port = port_with_saves("MyGame", Some("Save"), Some("Save2"));
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"from-save-folder").unwrap();
        fs::create_dir_all(dest_dir.join("Save2")).unwrap();
        fs::write(dest_dir.join("Save2").join("slot1.dat"), b"from-save-folder2").unwrap();
        fs::write(dest_dir.join("game.exe"), b"exe").unwrap();

        uninstall_port(&port, &dir, &saves_backup_dir).unwrap();

        let backup1 = save_backup::pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder").join("slot1.dat");
        let backup2 = save_backup::pending_restore_dir(&saves_backup_dir, "MyGame", "save_folder2").join("slot1.dat");
        assert_eq!(fs::read_to_string(backup1).unwrap(), "from-save-folder");
        assert_eq!(fs::read_to_string(backup2).unwrap(), "from-save-folder2");
    }

    #[test]
    fn is_installed_faux_si_dossier_absent_et_ne_paniques_pas_sur_folder_invalide() {
        let dir = temp_dir("is_installed");
        let port = Port {
            name: "n".into(),
            name_lower: "n".into(),
            tags: vec![],
            tags_lower: vec![],
            source: Value::String("s".into()),
            folder: "../evil".into(),
            executable: None,
            website: None,
            instructions: String::new(),
            mods: None,
            image: None,
            icon: None,
            save: None,
            save2: None,
            source_type: SourceType::DirectUrl,
            repo: None,
            exe_is_archive: None,
            preferred_asset: None,
            extra: None,
            user_managed: false,
        };
        assert!(!is_installed(&port, &dir));
    }

    fn build_adversarial_zip(path: &Path, cycle: usize) {
        let wrapper = format!("Jeu-Recomp-{cycle}-\u{1F3AE}");
        let mut entries: Vec<(String, Vec<u8>)> = (0..20)
            .map(|i| (format!("{wrapper}/data/niveau_{i}_\u{00e9}.bin"), format!("contenu-{cycle}-{i}").into_bytes()))
            .collect();
        entries.push((format!("{wrapper}/game.exe"), b"fake-exe".to_vec()));
        let files: Vec<(&str, &[u8])> = entries.iter().map(|(p, c)| (p.as_str(), c.as_slice())).collect();
        build_archive("zip", &files, path);
    }

    #[test]
    fn cycle_install_reinstall_uninstall_avec_archives_adversariales_ne_plante_jamais() {
        let library_dir = temp_dir("cycle_stress");
        let saves_backup_dir = library_dir.join("Saves Backup");
        const CYCLES: usize = 25;

        for cycle in 0..CYCLES {
            let archive_path = library_dir.join(format!("archive_{cycle}.zip"));
            build_adversarial_zip(&archive_path, cycle);

            let port = port_with_save(&format!("Game{cycle}"), Some("Save"));
            let dest_dir = library_dir.join(&port.folder);

            let mut on_progress: ProgressCallback = None;
            extract(&archive_path, &dest_dir, &library_dir, None, &mut on_progress).unwrap();

            if dest_dir.exists() {
                fs::create_dir_all(dest_dir.join("Save")).unwrap();
                fs::write(dest_dir.join("Save").join("slot.dat"), format!("save-{cycle}")).unwrap();
            }

            let _ = uninstall_port(&port, &library_dir, &saves_backup_dir);
            assert!(!dest_dir.exists(), "cycle {cycle}: uninstall_port n'a pas nettoyé dest_dir");
        }

        let leftover: Vec<_> = fs::read_dir(&library_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("_staging_"))
            .collect();
        assert!(leftover.is_empty(), "dossiers de transit non nettoyés après {CYCLES} cycles: {leftover:?}");
    }
}
