
use super::asset_select::AssetSelectionError;
use super::models::{Port, SourceType};
use super::path_safety::safe_join;
use super::platform_resolve::{resolve_per_platform, resolve_preferred_asset};
use super::save_backup;
use super::{github_api, gitlab_api};
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_UNCOMPRESSED_SIZE: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Debug)]
pub enum InstallError {
    Message(String),
    Ambiguous(Vec<Value>),
}

impl From<AssetSelectionError> for InstallError {
    fn from(e: AssetSelectionError) -> Self {
        match e {
            AssetSelectionError::Message(m) => InstallError::Message(m),
            AssetSelectionError::Ambiguous(a) => InstallError::Ambiguous(a),
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

fn with_path<T>(result: io::Result<T>, path: &Path) -> io::Result<T> {
    result.map_err(|e| io::Error::new(e.kind(), format!("{e} -- {}", path.display())))
}

fn retry_transient<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last_err = None;
    for attempt in 1..=5u64 {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
        std::thread::sleep(Duration::from_millis(100 * attempt));
    }
    Err(last_err.expect("at least one attempt"))
}

fn remove_existing(path: &Path) -> io::Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else { return Ok(()) };
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path).or_else(|_| fs::remove_dir(path))
    }
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn safe_download_name(name: &str) -> String {
    Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "download".to_string())
}

fn pixeldrain_file_id(url: &str) -> Option<&str> {
    ["https://pixeldrain.com/u/", "http://pixeldrain.com/u/"].iter().find_map(|prefix| {
        let rest = url.strip_prefix(prefix)?;
        let id = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        (!id.is_empty()).then_some(id)
    })
}

fn pixeldrain_real_name(id: &str) -> Option<String> {
    let url = format!("https://pixeldrain.com/api/file/{id}/info");
    let mut resp = super::http::agent(Duration::from_secs(10)).get(&url).call().ok()?;
    let json: Value = resp.body_mut().read_json().ok()?;
    json.get("name").and_then(Value::as_str).map(str::to_string)
}

fn rename_dest(dest: &Path, filename: &str) -> PathBuf {
    dest.with_file_name(safe_download_name(filename))
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

fn download(url: &str, dest: &Path, on_progress: &mut dyn FnMut(&str)) -> Result<PathBuf, InstallError> {
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
    on_progress(&format!("Downloading {name}..."));

    let agent = super::http::agent(Duration::from_secs(30 * 60));
    let mut resp = agent.get(&url).call().install_err()?;
    if !has_known_archive_extension(&dest.to_string_lossy()) {
        let disposition_name = resp
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .and_then(content_disposition_filename)
            .filter(|n| has_known_archive_extension(n));
        if let Some(disposition_name) = disposition_name {
            dest = rename_dest(&dest, &disposition_name);
        }
    }
    if let Some(final_name) =
        ureq::ResponseExt::get_uri(&resp).path().rsplit('/').next().filter(|n| has_known_archive_extension(n) && *n != name)
    {
        dest = rename_dest(&dest, final_name);
    }
    let mut file = with_path(fs::File::create(&dest), &dest).install_err()?;
    with_path(io::copy(&mut resp.body_mut().as_reader(), &mut file), &dest).install_err()?;
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
        let entries: Vec<fs::DirEntry> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
        let [wrapper] = entries.as_slice() else { return Ok(()) };
        if !wrapper.file_type()?.is_dir() {
            return Ok(());
        }
        let staging = dir.join(format!(".flatten-{}", wrapper.file_name().to_string_lossy()));
        with_path(retry_transient(|| fs::rename(wrapper.path(), &staging)), &staging)?;
        for item in fs::read_dir(&staging)?.filter_map(|e| e.ok()) {
            let target = dir.join(item.file_name());
            with_path(retry_transient(|| fs::rename(item.path(), &target)), &target)?;
        }
        with_path(retry_transient(|| fs::remove_dir(&staging)), &staging)?;
    }
}

fn merge_into(src: &Path, dest: &Path) -> io::Result<()> {
    with_path(fs::create_dir_all(dest), dest)?;
    for entry in fs::read_dir(src)?.filter_map(|e| e.ok()) {
        let item = entry.path();
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if !is_real_dir(&target) {
                with_path(retry_transient(|| remove_existing(&target)), &target)?;
            }
            merge_into(&item, &target)?;
        } else {
            with_path(retry_transient(|| remove_existing(&target)), &target)?;
            with_path(retry_transient(|| fs::rename(&item, &target)), &target)?;
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)?.filter_map(|e| e.ok()) {
        if entry.file_type()?.is_dir() {
            collect_files(&entry.path(), out)?;
        } else {
            out.push(entry.path());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
const SEVENZIP_NAME: &str = "7z.exe";
#[cfg(target_os = "linux")]
const SEVENZIP_NAME: &str = "7zzs";

pub(crate) fn sevenzip_exe_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [dir.join(SEVENZIP_NAME), dir.parent()?.join(SEVENZIP_NAME)].into_iter().find(|p| p.is_file())
}

pub(crate) fn run_7z(tool: &Path, args: &[&OsStr], action: &str, cwd: Option<&Path>) -> Result<std::process::Output, String> {
    let mut cmd = std::process::Command::new(tool);
    cmd.arg("-sccUTF-8").args(args).stdin(std::process::Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| e.to_string())?;
    if output.status.code().is_none_or(|c| c >= 2) {
        return Err(format!("7-Zip failed to {action} this archive: {}", seven_zip_error(&output.stdout, &output.stderr)));
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

fn extract_archive_via_7z(archive: &Path, staging: &Path, max_uncompressed_size: u64) -> Result<(), InstallError> {
    let tool = sevenzip_exe_path()
        .ok_or_else(|| InstallError::Message("7-Zip is required next to the application to extract this file (missing).".to_string()))?;

    let listing = run_7z(&tool, &[OsStr::new("l"), OsStr::new("-slt"), archive.as_os_str()], "read", None).install_err()?;
    let text = String::from_utf8_lossy(&listing.stdout);
    let kind = text.lines().find_map(|l| l.strip_prefix("Type = ")).unwrap_or("").trim().to_string();
    let total: u64 = text.lines().filter_map(|l| l.strip_prefix("Size = ")).filter_map(|n| n.trim().parse::<u64>().ok()).sum();
    reject_if_too_large(total, max_uncompressed_size)?;

    let out_arg = format!("-o{}", staging.display());
    let run_extract = |source: &Path, extra: Option<&str>| {
        let mut args = vec![OsStr::new("x"), source.as_os_str(), OsStr::new(&out_arg), OsStr::new("-y"), OsStr::new("-bd")];
        args.extend(extra.map(OsStr::new));
        run_7z(&tool, &args, "extract", None).install_err()
    };
    let nsis_exclusion = kind.eq_ignore_ascii_case("Nsis").then_some("-x!$PLUGINSDIR\\*");
    run_extract(archive, nsis_exclusion)?;

    if kind.eq_ignore_ascii_case("gzip") {
        if let Some(inner) = fs::read_dir(staging).install_err()?.filter_map(|e| e.ok()).map(|e| e.path()).next() {
            run_extract(&inner, None)?;
            with_path(retry_transient(|| fs::remove_file(&inner)), &inner).install_err()?;
        }
    }
    Ok(())
}

fn find_exe_folder(staging: &Path, target: &str) -> Result<PathBuf, InstallError> {
    let mut matches = Vec::new();
    collect_files(staging, &mut matches).install_err()?;
    let target_lower = target.to_lowercase();
    matches.retain(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().contains(&target_lower)));
    match matches.as_slice() {
        [] => Err(InstallError::Message(format!("No file matches \"{target}\" (see \"exe_is_archive\" for this port)."))),
        [single] => Ok(single.parent().unwrap_or(staging).to_path_buf()),
        many => Err(InstallError::Message(format!("{} files match \"{target}\" -- please report this port.", many.len()))),
    }
}

#[cfg(target_os = "windows")]
fn clear_readonly_recursive(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let mut perms = metadata.permissions();
    if perms.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(path, perms)?;
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)?.filter_map(|e| e.ok()) {
            clear_readonly_recursive(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn clear_readonly_recursive(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Ok(());
    }
    let mut perms = metadata.permissions();
    if perms.readonly() {
        perms.set_mode(perms.mode() | 0o200);
        fs::set_permissions(path, perms)?;
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)?.filter_map(|e| e.ok()) {
            clear_readonly_recursive(&entry.path())?;
        }
    }
    Ok(())
}

fn extract_to_staging(archive: &Path, staging: &Path, exe_is_archive: Option<&str>) -> Result<PathBuf, InstallError> {
    let name_lower = archive.to_string_lossy().to_lowercase();
    let is_archive = (name_lower.ends_with(".exe") && exe_is_archive.is_some())
        || [".zip", ".tar.gz", ".tgz", ".tar", ".7z", ".rar"].iter().any(|ext| name_lower.ends_with(ext));

    if !is_archive {
        let dest = staging.join(archive.file_name().unwrap_or_default());
        with_path(retry_transient(|| fs::copy(archive, &dest)), &dest).install_err()?;
        return Ok(staging.to_path_buf());
    }
    extract_archive_via_7z(archive, staging, MAX_UNCOMPRESSED_SIZE)?;
    with_path(clear_readonly_recursive(staging), staging).install_err()?;
    match exe_is_archive {
        Some(target) => find_exe_folder(staging, target),
        None => {
            flatten_single_wrapper_folder(staging).install_err()?;
            Ok(staging.to_path_buf())
        }
    }
}

fn extract(archive: &Path, dest_dir: &Path, library_dir: &Path, exe_is_archive: Option<&str>, on_progress: &mut dyn FnMut(&str)) -> Result<(), InstallError> {
    on_progress("Extracting...");
    let staging = with_path(tempfile::Builder::new().prefix("_staging_").tempdir_in(library_dir), library_dir).install_err()?;
    let merge_root = extract_to_staging(archive, staging.path(), exe_is_archive)?;
    merge_into(&merge_root, dest_dir).install_err()
}

fn temp_dir_in(library_dir: &Path, prefix: &str) -> Result<tempfile::TempDir, InstallError> {
    with_path(tempfile::Builder::new().prefix(prefix).tempdir_in(library_dir), library_dir).install_err()
}

pub fn install_extra(port: &Port, library_dir: &Path, on_progress: &mut dyn FnMut(&str)) -> Result<(), InstallError> {
    let url = port.extra.as_deref().ok_or_else(|| InstallError::Message("This port has no \"extra\" files to install.".to_string()))?;
    let dest_dir = safe_join(library_dir, &port.folder).map_err(InstallError::Message)?;
    if !dest_dir.exists() {
        return Err(InstallError::Message("Install this port first, then add its extra files.".to_string()));
    }
    on_progress("Downloading extra files...");
    let tmp = temp_dir_in(library_dir, "_extra_download_")?;
    let dest = tmp.path().join(safe_download_name(url.rsplit('/').next().unwrap_or("extra")));
    let archive_path = download(url, &dest, on_progress)?;
    let staging = temp_dir_in(library_dir, "_extra_staging_")?;
    let merge_root = extract_to_staging(&archive_path, staging.path(), None)?;
    merge_into(&merge_root, &dest_dir).install_err()
}

#[derive(Clone, Copy)]
pub struct InstallPaths<'a> {
    pub library_dir: &'a Path,
    pub saves_backup_dir: &'a Path,
}

#[derive(Default, Clone, Copy)]
pub struct InstallOverrides<'a> {
    pub asset: Option<&'a Value>,
    pub release: Option<&'a Value>,
}

#[derive(Clone, Copy)]
enum Forge {
    Github,
    Gitlab,
}

impl Forge {
    fn latest_installable(self, repo: &str, token: Option<&str>, preferred: Option<&str>) -> Result<(Value, Value), AssetSelectionError> {
        match self {
            Forge::Github => github_api::latest_installable_release(repo, token, preferred),
            Forge::Gitlab => gitlab_api::latest_installable_release(repo, token, preferred),
        }
    }

    fn most_recent_release(self, repo: &str, token: Option<&str>) -> Result<Value, AssetSelectionError> {
        match self {
            Forge::Github => github_api::most_recent_release(repo, token),
            Forge::Gitlab => gitlab_api::most_recent_release(repo, token),
        }
    }

    fn pick_release_asset(self, release: &Value, preferred: Option<&str>) -> Result<Value, AssetSelectionError> {
        match self {
            Forge::Github => github_api::pick_release_asset(release, preferred),
            Forge::Gitlab => gitlab_api::pick_release_asset(release, preferred),
        }
    }

    fn url_field(self) -> &'static str {
        match self {
            Forge::Github => "browser_download_url",
            Forge::Gitlab => "url",
        }
    }
}

fn download_release_asset(
    forge: Forge,
    repo: &str,
    token: Option<&str>,
    preferred: Option<&str>,
    overrides: InstallOverrides,
    tmp_path: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(PathBuf, Option<String>), InstallError> {
    let (release, asset) = match (overrides.release, overrides.asset) {
        (Some(r), Some(a)) => (r.clone(), a.clone()),
        (Some(r), None) => (r.clone(), forge.pick_release_asset(r, preferred)?),
        (None, Some(a)) => (forge.most_recent_release(repo, token)?, a.clone()),
        (None, None) => forge.latest_installable(repo, token, preferred)?,
    };
    let installed_tag = release.get("tag_name").and_then(Value::as_str).map(str::to_string);
    let name = asset.get("name").and_then(Value::as_str).unwrap_or("download");
    let url = asset
        .get(forge.url_field())
        .and_then(Value::as_str)
        .ok_or_else(|| InstallError::Message(format!("Asset without a \"{}\" field -- please report this port.", forge.url_field())))?;
    let archive_path = download(url, &tmp_path.join(safe_download_name(name)), on_progress)?;
    Ok((archive_path, installed_tag))
}

pub fn install_port(
    port: &Port,
    paths: InstallPaths,
    github_token: Option<&str>,
    gitlab_token: Option<&str>,
    overrides: InstallOverrides,
    on_progress: &mut dyn FnMut(&str),
) -> Result<Option<String>, InstallError> {
    let InstallPaths { library_dir, saves_backup_dir } = paths;
    let dest_dir = safe_join(library_dir, &port.folder).map_err(InstallError::Message)?;
    let tmp = temp_dir_in(library_dir, "_download_")?;
    let preferred_asset = port.preferred_asset.as_ref().and_then(resolve_preferred_asset);
    let repo = port.repo.as_deref().unwrap_or_default();

    let (archive_path, installed_tag) = match port.source_type {
        SourceType::Github => {
            download_release_asset(Forge::Github, repo, github_token, preferred_asset.as_deref(), overrides, tmp.path(), on_progress)?
        }
        SourceType::Gitlab => {
            download_release_asset(Forge::Gitlab, repo, gitlab_token, preferred_asset.as_deref(), overrides, tmp.path(), on_progress)?
        }
        SourceType::DirectUrl => {
            let resolved = resolve_per_platform(&port.source);
            let url = resolved
                .as_ref()
                .and_then(Value::as_str)
                .ok_or_else(|| InstallError::Message("\"source\" is not a usable link for this port".to_string()))?;
            let dest = tmp.path().join(safe_download_name(url.rsplit('/').next().unwrap_or("download")));
            (download(url, &dest, on_progress)?, None)
        }
        SourceType::None => return Err(InstallError::Message("This port has nothing to download.".to_string())),
    };

    extract(&archive_path, &dest_dir, library_dir, port.exe_is_archive.as_deref(), on_progress)?;
    save_backup::restore_all_after_install(port, saves_backup_dir, &dest_dir);
    on_progress("Done.");
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
    safe_join(library_dir, &port.folder).is_ok_and(|p| p.exists())
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

    fn no_progress(_: &str) {}

    fn port_with_saves(folder: &str, save: Option<&str>, save2: Option<&str>) -> Port {
        let mut value = serde_json::json!({"name": "n", "folder": folder, "source": "https://example.com/x.zip"});
        if let Some(s) = save {
            value["save"] = s.into();
        }
        if let Some(s) = save2 {
            value["save2"] = s.into();
        }
        super::super::models::port_from_value(&value).unwrap()
    }

    fn port_with_save(folder: &str, save: Option<&str>) -> Port {
        port_with_saves(folder, save, None)
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

    #[test]
    fn safe_download_name_ne_garde_que_le_nom_final() {
        assert_eq!(safe_download_name("../../Startup/evil.exe"), "evil.exe");
        assert_eq!(safe_download_name(".."), "download");
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
        let tool = sevenzip_exe_path().expect("7-Zip introuvable, requis par les tests");
        let output =
            std::process::Command::new(&tool).current_dir(&src).arg("a").arg(format!("-t{kind}")).arg(archive_path).arg(".").output().unwrap();
        assert!(output.status.success(), "échec de création de l'archive ({kind}) : {}", String::from_utf8_lossy(&output.stdout));
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
    fn extract_archive_via_7z_rejette_une_archive_trop_grosse() {
        let dir = temp_dir("zipbomb");
        let archive_path = dir.join("bomb.zip");
        build_archive("zip", &[("a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        assert!(extract_archive_via_7z(&archive_path, &staging, 0).is_err());
    }

    #[test]
    fn extract_archive_via_7z_extrait_un_zip() {
        let dir = temp_dir("zipok");
        let archive_path = dir.join("ok.zip");
        build_archive("zip", &[("sub/a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        extract_archive_via_7z(&archive_path, &staging, MAX_UNCOMPRESSED_SIZE).unwrap();
        assert_eq!(fs::read_to_string(staging.join("sub").join("a.txt")).unwrap(), "hello world");
    }

    #[test]
    fn extract_archive_via_7z_extrait_un_tar_gz() {
        let dir = temp_dir("targz");
        let archive_path = dir.join("ok.tar.gz");
        build_tar_gz_archive(&[("sub/a.txt", b"hello world")], &archive_path);
        let staging = dir.join("staging");
        fs::create_dir_all(&staging).unwrap();
        extract_archive_via_7z(&archive_path, &staging, MAX_UNCOMPRESSED_SIZE).unwrap();
        assert_eq!(fs::read_to_string(staging.join("sub").join("a.txt")).unwrap(), "hello world");
    }

    #[test]
    fn extract_archive_via_7z_extrait_un_7z() {
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
        extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut no_progress).unwrap();

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
        assert!(extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut no_progress).is_err());
    }

    #[test]
    fn extract_exe_is_archive_ancre_leve_l_ambiguite() {
        let dir = temp_dir("exe_archive_base_exe");
        let archive_path = dir.join("Install.exe");
        build_archive("zip", &[("redist/vc_redist.x64.exe", b"redist".as_slice()), ("bin/game.exe", b"the-game".as_slice())], &archive_path);

        let dest_dir = dir.join("dest");
        extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut no_progress).unwrap();

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
        assert!(extract(&archive_path, &dest_dir, &dir, Some("game.exe"), &mut no_progress).is_err());
    }

    #[test]
    fn extract_route_un_rar_vers_7z_plutot_que_de_le_copier() {
        let dir = temp_dir("rar_routed_to_7z");
        let archive_path = dir.join("release.rar");
        fs::write(&archive_path, b"not a real rar file").unwrap();
        let dest_dir = dir.join("dest");
        assert!(extract(&archive_path, &dest_dir, &dir, None, &mut no_progress).is_err());
        assert!(!dest_dir.join("release.rar").exists());
    }

    #[test]
    fn extract_exe_sans_exe_is_archive_copie_l_exe() {
        let dir = temp_dir("exe_not_archive");
        let archive_path = dir.join("portable.exe");
        fs::write(&archive_path, b"MZ-fake-pe-header").unwrap();
        let dest_dir = dir.join("dest");
        extract(&archive_path, &dest_dir, &dir, None, &mut no_progress).unwrap();
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
    fn clear_readonly_recursive_leve_l_attribut_sur_tout_l_arbre() {
        let dir = temp_dir("readonly_clear");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("file.txt"), b"x").unwrap();
        for p in [dir.join("sub"), dir.clone()] {
            let mut perms = fs::metadata(&p).unwrap().permissions();
            perms.set_readonly(true);
            fs::set_permissions(&p, perms).unwrap();
        }

        clear_readonly_recursive(&dir).unwrap();

        assert!(!fs::metadata(&dir).unwrap().permissions().readonly());
        assert!(!fs::metadata(dir.join("sub")).unwrap().permissions().readonly());
        let renamed = dir.with_file_name("readonly_clear_renamed");
        let _ = fs::remove_dir_all(&renamed);
        fs::rename(&dir, &renamed).unwrap();
    }

    #[test]
    fn extract_leve_l_attribut_lecture_seule_herite_de_l_archive() {
        let dir = temp_dir("readonly_archive");
        let src = dir.join("_src");
        fs::create_dir_all(src.join("Wrapper")).unwrap();
        fs::write(src.join("Wrapper").join("game.exe"), b"fake-exe").unwrap();
        let mut perms = fs::metadata(src.join("Wrapper")).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(src.join("Wrapper"), perms).unwrap();

        let archive_path = dir.join("readonly.zip");
        let tool = sevenzip_exe_path().expect("7-Zip introuvable, requis par les tests");
        let output = std::process::Command::new(&tool).current_dir(&src).arg("a").arg("-tzip").arg(&archive_path).arg(".").output().unwrap();
        assert!(output.status.success());

        let dest_dir = dir.join("dest");
        extract(&archive_path, &dest_dir, &dir, None, &mut no_progress).unwrap();
        assert_eq!(fs::read_to_string(dest_dir.join("game.exe")).unwrap(), "fake-exe");
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

    #[test]
    fn merge_remplace_un_fichier_par_un_dossier_et_inversement() {
        let dir = temp_dir("merge_kind_change");
        let src = dir.join("src");
        let dest = dir.join("dest");
        fs::create_dir_all(src.join("data")).unwrap();
        fs::write(src.join("data").join("a.bin"), b"a").unwrap();
        fs::write(src.join("config"), b"file").unwrap();
        fs::create_dir_all(dest.join("config")).unwrap();
        fs::write(dest.join("data"), b"was a file").unwrap();

        merge_into(&src, &dest).unwrap();

        assert_eq!(fs::read_to_string(dest.join("data").join("a.bin")).unwrap(), "a");
        assert_eq!(fs::read_to_string(dest.join("config")).unwrap(), "file");
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
    fn is_installed_faux_si_folder_invalide() {
        let dir = temp_dir("is_installed");
        assert!(!is_installed(&port_with_save("../evil", None), &dir));
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
    fn cycles_install_desinstall_ne_laissent_aucun_dossier_de_transit() {
        let library_dir = temp_dir("cycle_stress");
        let saves_backup_dir = library_dir.join("Saves Backup");
        const CYCLES: usize = 25;

        for cycle in 0..CYCLES {
            let archive_path = library_dir.join(format!("archive_{cycle}.zip"));
            build_adversarial_zip(&archive_path, cycle);
            let port = port_with_save(&format!("Game{cycle}"), Some("Save"));
            let dest_dir = library_dir.join(&port.folder);

            extract(&archive_path, &dest_dir, &library_dir, None, &mut no_progress).unwrap();
            fs::create_dir_all(dest_dir.join("Save")).unwrap();
            fs::write(dest_dir.join("Save").join("slot.dat"), format!("save-{cycle}")).unwrap();

            uninstall_port(&port, &library_dir, &saves_backup_dir).unwrap();
            assert!(!dest_dir.exists(), "cycle {cycle} : dossier du port non supprimé");
        }

        let leftover: Vec<_> = fs::read_dir(&library_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("_staging_"))
            .collect();
        assert!(leftover.is_empty(), "dossiers de transit restants : {leftover:?}");
    }
}
