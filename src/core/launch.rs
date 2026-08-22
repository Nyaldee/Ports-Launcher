//! Lancement de l'exécutable résolu d'un port installé (voir
//! `platform_utils::resolve_executable`) -- toujours un binaire connu et
//! déjà vérifié existant sur disque, jamais un chemin arbitraire à faire
//! résoudre par le shell : `Command::spawn` suffit pour un vrai `.exe`.
//!
//! `.lnk` et `.bat` font exception : ni l'un ni l'autre n'est un PE, et
//! `CreateProcess` (ce qu'utilise `Command::spawn`) ne sait exécuter
//! directement ni un raccourci ni un script batch -- les deux passent par le
//! Shell (voir `launch_via_shell`).

use std::io;
use std::path::Path;
use std::process::{Child, Command};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
use windows::Win32::System::Threading::GetExitCodeProcess;
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Un `.exe` lancé via `Command::spawn` donne un vrai `std::process::Child`
/// (attente/nettoyage standards). Un `.lnk` lancé via `ShellExecuteExW` ne
/// donne qu'un `HANDLE` Win32 brut (avec `SEE_MASK_NOCLOSEPROCESS`, sinon le
/// Shell le referme lui-même et on ne peut plus jamais savoir si le jeu
/// tourne encore) -- ce type unifie les deux pour que `is_port_running`
/// n'ait pas à connaître la différence.
pub enum LaunchedProcess {
    Native(Child),
    Shell(HANDLE),
}

impl LaunchedProcess {
    /// Équivalent de `Child::try_wait().is_ok_and(|s| s.is_none())` pour les
    /// deux variantes -- ne consomme/nettoie rien côté OS au passage
    /// (contrairement à `try_wait`, qui récupère le statut de sortie une
    /// fois pour toutes) : `GetExitCodeProcess` peut être interrogé autant
    /// de fois que voulu sans effet de bord.
    pub fn is_running(&mut self) -> bool {
        match self {
            LaunchedProcess::Native(child) => matches!(child.try_wait(), Ok(None)),
            LaunchedProcess::Shell(handle) => {
                let mut code = 0u32;
                unsafe { GetExitCodeProcess(*handle, &mut code) }.is_ok() && code == STILL_ACTIVE.0 as u32
            }
        }
    }
}

impl Drop for LaunchedProcess {
    fn drop(&mut self) {
        // `SEE_MASK_NOCLOSEPROCESS` nous a rendus propriétaires du handle --
        // à nous de le refermer, sinon fuite de handle à chaque jeu lancé
        // via un .lnk.
        if let LaunchedProcess::Shell(handle) = self {
            unsafe {
                let _ = CloseHandle(*handle);
            }
        }
    }
}

/// `cwd` = dossier de l'exécutable, pour imiter un double-clic Explorer
/// (beaucoup de jeux portables ont besoin de leurs fichiers relatifs à
/// côté de l'exe).
pub fn launch(exe_path: &Path) -> io::Result<LaunchedProcess> {
    let cwd = exe_path.parent().unwrap_or(exe_path);
    // `.bat`/`.cmd` n'est pas un PE -- `Command::spawn` (CreateProcess) ne
    // sait pas l'exécuter directement, il faut passer par le Shell (même
    // chemin que `.lnk`) pour que l'association de fichier standard
    // (`cmd.exe /c`) s'en charge.
    let needs_shell = exe_path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("lnk") || e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
        .unwrap_or(false);
    if needs_shell {
        launch_via_shell(exe_path, cwd)
    } else {
        Command::new(exe_path).current_dir(cwd).spawn().map(LaunchedProcess::Native)
    }
}

/// `ShellExecuteExW` plutôt que `ShellExecuteW` pour récupérer le `HANDLE`
/// du processus lancé (voir `LaunchedProcess`). Utilisée pour `.lnk`
/// (raccourci, résolu par le Shell comme un double-clic Explorateur) ET
/// `.bat`/`.cmd` (script, exécuté via son association de fichier standard).
/// `exe_path` vient toujours de `resolve_executable`/`autodetect_executable`,
/// jamais d'une entrée arbitraire de `ports.json`.
fn launch_via_shell(target_path: &Path, cwd: &Path) -> io::Result<LaunchedProcess> {
    let verb = to_wide("open");
    let file = to_wide(&target_path.to_string_lossy());
    let dir = to_wide(&cwd.to_string_lossy());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpDirectory: PCWSTR(dir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(io::Error::other)?;
    if info.hProcess.is_invalid() {
        return Err(io::Error::other("ShellExecuteExW n'a renvoyé aucun processus"));
    }
    Ok(LaunchedProcess::Shell(info.hProcess))
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `ShellExecuteW` plutôt qu'un `Command::new("cmd").args(["/C", "start", ...])`
/// -- une URL/un chemin viennent de `ports.json` (communautaire, voir
/// `InfoDialog`) : passés en un seul argument natif à `ShellExecuteW`, ils ne
/// traversent jamais le parseur de commandes de `cmd.exe`, qui interprète
/// `&`/`|`/`^` même à l'intérieur de guillemets dans certains cas -- une
/// vraie injection de commande serait possible via un simple lien `website`
/// malveillant sinon.
fn shell_execute_open(target: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    let op = to_wide("open");
    let file = to_wide(target);
    unsafe {
        let _ = ShellExecuteW(None, PCWSTR(op.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Ouvre `url` (http/https, déjà filtré par l'appelant -- voir InfoDialog)
/// dans le navigateur par défaut.
pub fn open_url(url: &str) {
    shell_execute_open(url);
}

/// Ouvre `path` dans l'Explorateur -- dossier du jeu/dossier de sauvegarde
/// (voir InfoDialog).
pub fn open_path(path: &Path) {
    shell_execute_open(&path.to_string_lossy());
}
