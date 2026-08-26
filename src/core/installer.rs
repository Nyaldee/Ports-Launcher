//! Installation/désinstallation d'un port : téléchargement, extraction,
//! nettoyage. Gère les trois types de source (github, gitlab, direct_url)
//! et choisit automatiquement le bon fichier selon la plateforme.
//!
//! Extraction : `extract_archive_via_7z` shelle vers un `7z.exe` externe,
//! seule implémentation pour zip/tar/tar.gz/7z/NSIS/RAR.
//!
//! Sécurité : la taille annoncée est plafonnée avant extraction
//! (`reject_if_too_large`, anti zip-bomb) ; la protection contre un chemin
//! malveillant à l'intérieur d'une archive (zip-slip/tar-slip) est celle
//! intégrée à 7-Zip lui-même.

use super::asset_select::AssetSelectionError;
use super::github_api::GitHubError;
use super::gitlab_api::GitLabError;
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

/// Assez grand pour n'importe quel jeu réel, assez petit pour bloquer une
/// "zip bomb" (une archive de quelques Ko qui annonce plusieurs To/Po une
/// fois décompressée).
pub const MAX_UNCOMPRESSED_SIZE: u64 = 20 * 1024 * 1024 * 1024; // 20 Go

pub type ProgressCallback<'a> = Option<&'a mut dyn FnMut(&str)>;

#[derive(Debug)]
pub enum InstallError {
    Message(String),
    /// Fichiers de release parmi lesquels l'utilisateur peut choisir
    /// manuellement (voir `InstallOverrides::asset`). Le message n'est lu par
    /// aucun appelant -- le dialogue de choix a son propre titre générique
    /// (main.rs::open_picker_dialog) -- mais reste porté pour rester cohérent
    /// avec `AssetSelectionError`/`GitHubError`.
    Ambiguous(#[allow(dead_code)] String, Vec<Value>),
}

impl From<GitHubError> for InstallError {
    fn from(e: GitHubError) -> Self {
        match e {
            GitHubError::Message(m) => InstallError::Message(m),
            GitHubError::Ambiguous(m, a) => InstallError::Ambiguous(m, a),
        }
    }
}

impl From<GitLabError> for InstallError {
    fn from(e: GitLabError) -> Self {
        match e {
            GitLabError::Message(m) => InstallError::Message(m),
            GitLabError::Ambiguous(m, a) => InstallError::Ambiguous(m, a),
        }
    }
}

impl From<AssetSelectionError> for InstallError {
    fn from(e: AssetSelectionError) -> Self {
        match e {
            AssetSelectionError::Message(m) => InstallError::Message(m),
            AssetSelectionError::Ambiguous(m, a) => InstallError::Ambiguous(m, a),
        }
    }
}

/// Aplatit n'importe quelle erreur `Display` en `InstallError::Message`, pour
/// éviter un `.map_err(|e| InstallError::Message(e.to_string()))` à chaque
/// appel faillible de ce module.
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

/// Ne garde que le nom de fichier final d'un nom annoncé par une API/URL
/// externe (mainteneur de la release ou champ `source` de `ports.json`,
/// potentiellement compromis) -- un "../../../Startup/evil.exe" ne doit
/// jamais pouvoir écrire hors du dossier de téléchargement visé.
fn safe_download_name(name: &str) -> String {
    Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "download".to_string())
}

/// `pixeldrain.com/u/<id>` est la page de PARTAGE (HTML), pas le fichier :
/// certaines releases l'utilisent tel quel comme URL d'asset, et la
/// télécharger directement enregistrerait quelques Ko de HTML à la place de
/// l'archive. `/api/file/<id>` est le vrai point de téléchargement direct.
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

/// Vrai nom de fichier (avec extension) d'un fichier pixeldrain, via son API
/// d'info : le nom déclaré dans une release n'inclut pas toujours
/// l'extension (ex: "Patcher.v2.0.4-Windows", sans ".7z"), ce qui ferait
/// échouer la détection du type d'archive dans `extract()`, basée dessus.
/// Best-effort : `None` laisse `download` garder le nom d'origine plutôt que
/// de faire échouer l'install pour un problème de nommage.
fn pixeldrain_real_name(id: &str) -> Option<String> {
    let agent = super::http::agent(Duration::from_secs(10));
    let url = format!("https://pixeldrain.com/api/file/{id}/info");
    let mut resp = agent.get(&url).call().ok()?;
    let json: Value = resp.body_mut().read_json().ok()?;
    json.get("name").and_then(Value::as_str).map(str::to_string)
}

/// Réécrit `dest` pour porter `filename` à la place de son nom actuel, en
/// préservant son dossier parent -- petit utilitaire partagé par les deux
/// stratégies de résolution du VRAI nom de fichier dans `download` (l'une
/// via l'API pixeldrain, l'autre via l'URL finale après redirection) : dans
/// les deux cas, un nom initialement connu (souvent un ID numérique ou un
/// slug sans extension) doit être remplacé par le nom réel AVEC extension,
/// sans quoi `extract_to_staging` (qui décide zip/7z/etc. sur l'EXTENSION du
/// nom de fichier) ne reconnaît pas l'archive et se contente de copier le
/// fichier brut tel quel.
fn rename_dest(dest: &Path, filename: &str) -> PathBuf {
    dest.parent().map(|parent| parent.join(safe_download_name(filename))).unwrap_or_else(|| dest.to_path_buf())
}

/// Télécharge `url` vers `dest` et renvoie le chemin RÉELLEMENT écrit.
/// Deux stratégies de résolution du vrai nom de fichier, dans l'ordre où
/// l'information devient disponible :
/// 1. AVANT la requête -- cas pixeldrain (voir `pixeldrain_file_id`), dont
///    l'URL de partage est réécrite vers son point de téléchargement direct,
///    avec le nom obtenu via son API d'info.
/// 2. APRÈS la requête -- cas générique (mirror qui redirige, ModDB entre
///    autres) : le nom ne diffère de celui déjà connu que si l'URL FINALE
///    après redirection (`ResponseExt::get_uri`, voir ureq) porte un nom
///    différent avec une extension, jamais si elle retombe sur le même
///    chemin (rien à gagner).
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
    notify(on_progress, &format!("Downloading {name}..."));

    // 30 minutes, pas 30s (comme les appels d'API) : `timeout_global` couvre
    // TOUTE l'opération y compris la lecture du corps ci-dessous -- un
    // fichier de release fait couramment plusieurs centaines de Mo, 30s ne
    // suffit que sur une connexion très rapide.
    let agent = super::http::agent(Duration::from_secs(30 * 60));
    let mut resp = agent.get(&url).call().install_err()?;
    if let Some(final_name) = ureq::ResponseExt::get_uri(&resp).path().rsplit('/').next().filter(|n| n.contains('.') && *n != name) {
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

/// Beaucoup de releases zippent leur contenu dans un dossier racine (ex:
/// `pd-x86_64-windows/...`) -- si c'est le seul élément extrait, on remonte
/// son contenu d'un niveau pour que le jeu soit directement dans le
/// dossier du port (utile pour l'auto-détection de l'exécutable).
///
/// En boucle (pas une seule passe) : certaines releases empilent PLUSIEURS
/// dossiers wrapper (`release-v1.0/win64/...`) -- chaque itération retire
/// exactement un niveau, la boucle s'arrête dès que `dir` ne contient plus
/// exactement UN dossier.
///
/// Le wrapper est renommé vers un nom provisoire AVANT de déplacer son
/// contenu : un enfant qui porte EXACTEMENT le même nom que son parent
/// (`Recompiled/Recompiled/...`) ferait sinon échouer le renommage suivant,
/// le wrapper occupant encore ce chemin tant qu'il n'est pas vidé.
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

/// Copie récursivement le contenu de `src` dans `dest`, en écrasant par nom
/// (une entrée présente des deux côtés est remplacée par celle de `src`)
/// -- mais ne touche JAMAIS une entrée de `dest` absente de `src`. C'est
/// cette dernière propriété qui préserve une sauvegarde ou tout autre
/// fichier généré par le jeu lui-même lors d'une mise à jour : seul ce que
/// la nouvelle archive contient réellement est remplacé, jamais un
/// nettoyage préalable de `dest`.
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

/// Cherche récursivement tous les fichiers sous `dir` -- voir
/// `find_exe_folder`. N'importe quelle extension : l'ancre donnée par un
/// port (voir `Port::exe_is_archive`) n'est pas nécessairement un `.exe`.
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

/// Chemin de `7z.exe`, attendu à côté de l'exécutable (jamais empaqueté dans
/// le binaire -- même convention que `ports.json`/`themes.json`, voir
/// `base_dir` dans main.rs, copié par build.rs). Vérifie aussi un niveau
/// au-dessus du dossier de l'exe : `cargo test` tourne depuis
/// target/<profil>/deps/, alors que build.rs copie dans target/<profil>/,
/// un niveau plus haut.
fn sevenzip_exe_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let here = dir.join("7z.exe");
    if here.is_file() {
        return Some(here);
    }
    let one_up = dir.parent()?.join("7z.exe");
    one_up.is_file().then_some(one_up)
}

/// Lance `7z.exe` et vérifie son code de sortie -- 1 (avertissement non
/// fatal chez 7-Zip, ex: un commentaire SFX inhabituel) est accepté, 2+
/// (vraie erreur : format non lu, archive corrompue...) devient une erreur
/// nommée `action` (ex: "read"/"extract").
fn run_7z(tool: &Path, args: &[&std::ffi::OsStr], action: &str) -> Result<std::process::Output, InstallError> {
    let mut cmd = std::process::Command::new(tool);
    // -sccUTF-8 : les noms de fichiers non-ASCII dans les messages d'erreur
    // ne dépendent pas de la page de code console. Stdin fermé : jamais de
    // prompt interactif (mot de passe...) à satisfaire.
    cmd.arg("-sccUTF-8").args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW -- 7z.exe est un programme console, éviterait sinon un flash de fenêtre noire depuis cette appli GUI.
    }
    let output = cmd.output().install_err()?;
    if output.status.code().is_none_or(|c| c >= 2) {
        return Err(InstallError::Message(format!("7z.exe failed to {action} this archive: {}", seven_zip_error(&output.stdout, &output.stderr))));
    }
    Ok(output)
}

/// Repère la ligne `ERROR: ...` dans la sortie de `7z.exe` (stderr d'abord,
/// puis stdout) -- le reste (bannière, résumé "Errors: N"...) n'est pas
/// utile à afficher à l'utilisateur.
fn seven_zip_error(stdout: &[u8], stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .chain(String::from_utf8_lossy(stdout).lines())
        .find(|l| l.contains("ERROR"))
        .map(str::trim)
        .unwrap_or("unknown error")
        .to_string()
}

/// Liste puis extrait `archive` via `7z.exe` -- couvre zip/tar/tar.gz/7z/
/// NSIS/RAR (y compris un `.exe` auto-extractible de n'importe lequel de ces
/// types) en une seule implémentation. Le type réel est détecté à partir du
/// listing technique (`Type = ...`), pas de l'extension du fichier.
///
/// Retourne le type détecté (`"Zip"`, `"Nsis"`, `"Rar5"`...) : l'appelant
/// (`extract`) en a besoin pour savoir si un installeur NSIS a déjà écrit
/// chaque fichier à sa vraie destination (donc rien à deviner) et pour
/// exclure les DLL internes au moteur NSIS lui-même (`$PLUGINSDIR\`,
/// utilisées seulement pour l'assistant d'installation -- jamais des
/// fichiers du jeu, mais listées par 7-Zip comme le reste).
///
/// L'écriture des fichiers est déléguée à `7z.exe` (processus externe) --
/// la protection anti zip-slip est donc celle intégrée à 7-Zip lui-même.
fn extract_archive_via_7z(archive: &Path, staging: &Path, max_uncompressed_size: u64) -> Result<String, InstallError> {
    use std::ffi::OsStr;
    let tool = sevenzip_exe_path()
        .ok_or_else(|| InstallError::Message("7z.exe is required next to the application to extract this file (missing).".to_string()))?;

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

    // gzip n'enveloppe qu'un seul fichier -- un ".tar.gz"/".tgz" est donc
    // deux couches (gzip, puis tar dedans), que `7z x` ne déplie qu'une à la
    // fois : on relance dessus une fois la couche externe retirée.
    if kind.eq_ignore_ascii_case("gzip") {
        if let Some(inner) = fs::read_dir(staging).install_err()?.filter_map(|e| e.ok()).map(|e| e.path()).next() {
            run_7z(&tool, &[OsStr::new("x"), inner.as_os_str(), OsStr::new(&out_arg), OsStr::new("-y"), OsStr::new("-bd")], "extract")?;
            fs::remove_file(&inner).install_err()?;
        }
    }
    Ok(kind)
}

/// Dossier contenant le fichier ancre `target` (voir `Port::exe_is_archive`)
/// -- ce dossier, pas la racine du conteneur, est fusionné dans `dest_dir`.
/// `target` est TOUJOURS fourni désormais (plus d'auto-détection ambiguë) :
/// écarte tout fichier annexe (prérequis, outils, autres dossiers) du
/// dossier gardé, sans avoir à deviner lequel des dossiers candidats est le
/// bon.
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

/// Détecte le format de `archive` et l'extrait/copie vers `staging`, SANS
/// fusion finale dans `dest_dir` -- cœur partagé par `extract` (install
/// principale, ancre optionnelle via `exe_is_archive`) et `install_extra`
/// (toujours sans ancre : `extra` est censé être un contenu autonome, pas un
/// conteneur à la structure imprévisible comme un installeur NSIS). Renvoie
/// le dossier à fusionner dans `dest_dir` (voir `merge_into`).
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
        // Résultat (type d'archive détecté) sans objet ici -- servait
        // seulement à repérer un installeur NSIS pour l'ancien cas
        // "exe_is_archive sans nom", retiré (voir Port::exe_is_archive).
        extract_archive_via_7z(archive, staging, MAX_UNCOMPRESSED_SIZE)?;
        if let Some(target) = exe_is_archive {
            find_exe_folder(staging, target)
        } else {
            flatten_single_wrapper_folder(staging).install_err()?;
            Ok(staging.to_path_buf())
        }
    } else {
        // Fichier brut (ex: un simple .exe portable) -- copié tel quel,
        // pas une archive à décompresser.
        let dest = staging.join(archive.file_name().unwrap_or_default());
        fs::copy(archive, &dest).install_err()?;
        Ok(staging.to_path_buf())
    }
}

/// `exe_is_archive` -- voir `Port::exe_is_archive` -- déclare explicitement
/// que `archive` (un `.exe`) est un conteneur à décompresser (zip déguisé ou
/// installeur NSIS) et donne l'ancre à `find_exe_folder`, jamais deviné
/// depuis le contenu.
fn extract(archive: &Path, dest_dir: &Path, library_dir: &Path, exe_is_archive: Option<&str>, on_progress: &mut ProgressCallback) -> Result<(), InstallError> {
    notify(on_progress, "Extracting...");
    // Extraction dans un dossier de transit ISOLÉ puis fusion : extraire
    // DIRECTEMENT dans `dest_dir` laisserait deux entrées dès que le
    // dossier-wrapper de la release change de nom d'une version à l'autre,
    // empêchant `flatten_single_wrapper_folder` de s'exécuter lors d'une MAJ.
    //
    // Le transit vit DANS `library_dir`, pas dans le dossier temporaire de
    // l'OS : `merge_into` déplace ensuite son contenu via `fs::rename`, qui
    // échoue avec ERROR_NOT_SAME_DEVICE (os error 17) dès que source et
    // destination sont sur des volumes différents -- fréquent entre %TEMP%
    // et la bibliothèque d'installation.
    let staging_holder = tempfile::Builder::new().prefix("_staging_").tempdir_in(library_dir).install_err()?;
    let staging = staging_holder.path();
    let merge_root = extract_to_staging(archive, staging, exe_is_archive)?;
    merge_into(&merge_root, dest_dir).install_err()?;
    Ok(())
}

/// Champ `extra` de `Port` -- télécharge `url` et fusionne (voir
/// `merge_into`, écrase les fichiers de même nom) son contenu dans le
/// dossier déjà installé, APRÈS l'install principale. Appelée depuis
/// `install_port`, qui ignore silencieusement toute erreur ici (lien mort,
/// hors-ligne...) -- ne doit jamais faire échouer une install par ailleurs
/// réussie.
fn install_extra(url: &str, dest_dir: &Path, library_dir: &Path, on_progress: &mut ProgressCallback) -> Result<(), InstallError> {
    notify(on_progress, "Downloading extra files...");
    let tmp = tempfile::Builder::new().prefix("_extra_download_").tempdir_in(library_dir).install_err()?;
    let filename = url.rsplit('/').next().unwrap_or("extra");
    let dest = tmp.path().join(safe_download_name(filename));
    let archive_path = download(url, &dest, on_progress)?;

    let staging_holder = tempfile::Builder::new().prefix("_extra_staging_").tempdir_in(library_dir).install_err()?;
    let staging = staging_holder.path();
    // Jamais d'ancre pour `extra` -- toujours aplati tel quel (voir le
    // commentaire de la fonction).
    let merge_root = extract_to_staging(&archive_path, staging, None)?;
    merge_into(&merge_root, dest_dir).install_err()?;
    Ok(())
}

/// `library_dir`/`cache_dir`/`saves_backup_dir` voyagent toujours ensemble
/// depuis `main()` jusqu'à `install_port` -- regroupés pour la même raison
/// qu'`InstallOverrides` juste en dessous (`clippy::too_many_arguments`).
#[derive(Clone, Copy)]
pub struct InstallPaths<'a> {
    pub library_dir: &'a Path,
    pub cache_dir: &'a Path,
    pub saves_backup_dir: &'a Path,
}

/// `asset`/`release` voyagent toujours en paire depuis `install_port` jusqu'à
/// `download_release_asset` -- regroupés pour ne pas faire déborder ces deux
/// signatures au-delà de 7 paramètres (voir `clippy::too_many_arguments`).
#[derive(Default, Clone, Copy)]
pub struct InstallOverrides<'a> {
    /// Fichier de release choisi manuellement par l'utilisateur (dialogue de
    /// désambiguïsation), contourne l'heuristique `pick_asset` pour cet
    /// install précis.
    pub asset: Option<&'a Value>,
    /// Release choisie manuellement par l'utilisateur (voir
    /// `main.rs::open_version_picker`), contourne "toujours la dernière" pour
    /// cet install précis -- installer une version antérieure quand la
    /// dernière n'a par exemple plus de build Windows.
    pub release: Option<&'a Value>,
}

/// Partie commune à `SourceType::Github`/`SourceType::Gitlab` dans
/// `install_port` : seuls diffèrent le client API (via `get_latest_release`/
/// `pick_release_asset`) et le nom du champ portant l'URL de téléchargement
/// dans l'asset choisi (`browser_download_url` pour GitHub, `url` pour
/// GitLab -- voir `gitlab_api::release_assets`). Les closures sont une vraie
/// variation par source, pas un excès de paramètres, d'où l'`allow` plutôt
/// qu'un regroupement artificiel.
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

/// `overrides` -- voir `InstallOverrides` -- laisse l'utilisateur
/// court-circuiter l'asset et/ou la release auto-détectés pour cet install
/// précis.
///
/// Retourne `installed_tag` seul : la référence temporelle de
/// `check_update_available` est `InstalledInfo::installed_at`, toujours
/// connue, là où une date d'asset/release peut manquer selon la source.
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
    // Port LOCAL (voir SourceType::Local) -- rien à télécharger, l'utilisateur
    // place lui-même les fichiers du jeu dans dest_dir. Message clair plutôt
    // que de tenter un téléchargement sur une source absente.
    if port.source_type == SourceType::Local {
        return Err(InstallError::Message(format!(
            "This port has no download source -- place the game files yourself in \"{}\".",
            dest_dir.display()
        )));
    }
    let mut installed_tag = None;

    // DANS `library_dir`, pas dans %TEMP% -- même raison que le dossier de
    // transit d'`extract()` : tout le cycle de vie d'un install
    // (téléchargement, extraction, écriture) reste dans un dossier propre à
    // l'appli, ce qu'un scanner comportemental attend davantage d'un
    // logiciel légitime qu'une écriture dans le temporaire système.
    let tmp = tempfile::Builder::new().prefix("_download_").tempdir_in(library_dir).install_err()?;
    let tmp_path = tmp.path();
    // Per-plateforme comme executable/save (voir
    // resolve_preferred_asset) -- résolu UNE fois ici, pas à chaque
    // closure ci-dessous.
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
        // Exclu plus haut (retour anticipé avant même la création du
        // dossier de transit) -- jamais atteint.
        SourceType::Local => unreachable!("SourceType::Local exclu avant ce match"),
    };

    extract(&archive_path, &dest_dir, library_dir, port.exe_is_archive.as_deref(), &mut on_progress)?;
    save_backup::restore_all_after_install(port, saves_backup_dir, &dest_dir);

    if let Some(url) = &port.image {
        if url.starts_with("http://") || url.starts_with("https://") {
            image_cache::cache_image(url, cache_dir, &port.folder);
        }
    }

    // Best-effort, jamais fatal (voir install_extra) -- un lien mort ou une
    // panne réseau sur ce fichier annexe ne doit jamais faire échouer une
    // install par ailleurs réussie.
    if let Some(url) = &port.extra {
        let _ = install_extra(url, &dest_dir, library_dir, &mut on_progress);
    }

    notify(&mut on_progress, "Done.");
    Ok(installed_tag)
}

/// `safe_join` fait remonter une erreur si `folder` sort de
/// `library_dir` -- laissé tel quel à l'appelant : un `remove_dir_all` sur
/// un chemin non validé est le risque le plus grave du lot (suppression
/// arbitraire), jamais de repli silencieux ici.
///
/// Si `preserve_all_before_uninstall` échoue (ex: disque plein en cours de
/// copie), `dest_dir` n'est PAS supprimé -- annuler la désinstallation
/// plutôt que perdre une sauvegarde locale que la copie de secours n'a pas
/// réussi à mettre à l'abri.
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

/// Simple vérification en lecture (appelée pour CHAQUE port du catalogue
/// au démarrage) -- un `folder` invalide dans une seule entrée ne
/// doit jamais faire planter tout le lanceur, contrairement à
/// install/uninstall où l'erreur doit être vue.
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
        // Suffixe (page de commentaires, query string...) écarté, seul l'id compte.
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/u/XQjoRAjJ?x=1"), Some("XQjoRAjJ"));
    }

    #[test]
    fn pixeldrain_file_id_ignore_les_liens_deja_directs_ou_non_pixeldrain() {
        assert_eq!(pixeldrain_file_id("https://pixeldrain.com/api/file/XQjoRAjJ"), None);
        assert_eq!(pixeldrain_file_id("https://example.com/u/XQjoRAjJ"), None);
        assert_eq!(pixeldrain_file_id("https://github.com/foo/bar/releases/download/v1/a.zip"), None);
    }

    /// Fabrique une archive de test via `7z.exe a` : écrit chaque fichier
    /// sur disque dans un dossier source temporaire, puis le compresse.
    /// `kind` ("zip"/"tar"/"7z") pilote le switch `-t`, indépendant de
    /// l'extension de `archive_path` (utile pour un zip déguisé en `.exe`).
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

    /// `.tar.gz` : 7-Zip ne le fabrique pas en un seul passage (le switch `-t`
    /// ne combine pas deux formats) -- un vrai `.tar` d'abord, gzippé ensuite.
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
        // Une vraie archive valide mais dont on force artificiellement la
        // limite à 0 pour simuler une taille annoncée énorme -- construire
        // une vraie archive de 20+ Go serait absurde pour un test.
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

    /// Zip synthétique : bruit d'emballage à la racine (`_rels`,
    /// `[Content_Types].xml`) + le vrai jeu dans un sous-dossier de nom
    /// arbitraire, repéré par `find_exe_folder` via l'exe qu'il contient.
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
        // Deux dossiers distincts contiennent chacun un fichier qui matche
        // l'ancre -- jamais de devinette au hasard entre les deux, l'install
        // doit échouer clairement plutôt que d'installer la mauvaise variante.
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
        // Pas un vrai .rar (7-Zip ne sait pas en créer, format propriétaire)
        // -- ce test vérifie juste que l'extension seule suffit à router vers
        // extract_archive_via_7z plutôt que la copie brute, pas que 7z arrive
        // à le lire.
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
        // Contenu volontairement invalide en zip -- un vrai exécutable
        // portable n'a aucune raison de s'ouvrir comme une archive.
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
        // L'enfant porte EXACTEMENT le même nom que son parent -- cas qui
        // fait échouer un rename() naïf, voir flatten_single_wrapper_folder.
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
        // Un fichier de sauvegarde, présent seulement côté dest -- ne doit
        // jamais être supprimé par la fusion (c'est la propriété qui
        // préserve les saves lors d'une mise à jour).
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
            save: save.map(|s| Value::String(s.into())),
            save2: save2.map(|s| Value::String(s.into())),
            source_type: SourceType::DirectUrl,
            repo: None,
            exe_is_archive: None,
            preferred_asset: None,
            extra: None,
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
        // Même technique que save_backup::tests -- un FICHIER bloque
        // l'endroit où la copie de secours doit créer un dossier, simule un
        // échec de copie (disque plein, permission refusée...) sans
        // dépendre d'un vrai disque plein.
        let dir = temp_dir("uninstall_preserve_fails");
        let saves_backup_dir = dir.join("Saves Backup");
        fs::write(&saves_backup_dir, b"blocks Saves Backup from being a directory").unwrap();
        let port = port_with_save("MyGame", Some("Save"));
        let dest_dir = dir.join("MyGame");
        fs::create_dir_all(dest_dir.join("Save")).unwrap();
        fs::write(dest_dir.join("Save").join("slot1.dat"), b"precious").unwrap();

        assert!(uninstall_port(&port, &dir, &saves_backup_dir).is_err());

        // dest_dir n'a PAS été supprimé -- la sauvegarde locale n'a jamais
        // été mise en danger par une copie de secours ratée.
        assert_eq!(fs::read_to_string(dest_dir.join("Save").join("slot1.dat")).unwrap(), "precious");
    }

    #[test]
    fn uninstall_port_ne_touche_pas_une_sauvegarde_externe_au_dossier_du_jeu() {
        // save absolu, en dehors de dest_dir (le cas le plus courant
        // en pratique, ex: %APPDATA%/...) -- jamais dans la trajectoire de
        // uninstall_port (qui ne supprime QUE dest_dir), donc jamais
        // sauvegardé/déplacé non plus : rien à faire pour lui.
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
            save: None,
            save2: None,
            source_type: SourceType::DirectUrl,
            repo: None,
            exe_is_archive: None,
            preferred_asset: None,
            extra: None,
        };
        assert!(!is_installed(&port, &dir));
    }

    /// Archive de test variée selon `cycle` (dossier wrapper au nom
    /// unicode/emoji, fichiers imbriqués) pour le stress test ci-dessous.
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
        // Stress test de robustesse -- ce test ne doit JAMAIS :
        // - écrire hors du dossier sandbox dédié à CE test (jamais dans un
        //   vrai library_dir utilisateur, jamais dans %TEMP% système)
        // - toucher le réseau : extract() est appelée directement sur des
        //   archives 100% locales et synthétiques, jamais install_port ni
        //   download (qui feraient un vrai appel HTTP)
        // - paniquer ou laisser un dossier de transit orphelin, même en
        //   enchaînant install/uninstall sur de nombreux cycles avec des
        //   noms unicode/emoji.
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

        // Aucun dossier de transit oublié après {CYCLES} cycles --
        // tempfile::TempDir nettoie sur Drop (voir extract()), un survivant
        // voudrait dire un chemin qui a échappé au ménage normal.
        let leftover: Vec<_> = fs::read_dir(&library_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("_staging_"))
            .collect();
        assert!(leftover.is_empty(), "dossiers de transit non nettoyés après {CYCLES} cycles: {leftover:?}");
    }
}
