
use std::io;
use std::path::Path;
use std::process::{Child, Command};

pub enum LaunchedProcess {
    Native(Child),
    #[cfg(target_os = "windows")]
    Shell(windows::Win32::Foundation::HANDLE),
}

impl LaunchedProcess {
    pub fn is_running(&mut self) -> bool {
        match self {
            LaunchedProcess::Native(child) => matches!(child.try_wait(), Ok(None)),
            #[cfg(target_os = "windows")]
            LaunchedProcess::Shell(handle) => {
                use windows::Win32::Foundation::STILL_ACTIVE;
                use windows::Win32::System::Threading::GetExitCodeProcess;
                let mut code = 0u32;
                unsafe { GetExitCodeProcess(*handle, &mut code) }.is_ok() && code == STILL_ACTIVE.0 as u32
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for LaunchedProcess {
    fn drop(&mut self) {
        if let LaunchedProcess::Shell(handle) = self {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(*handle);
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn launch(exe_path: &Path) -> io::Result<LaunchedProcess> {
    let cwd = exe_path.parent().unwrap_or(exe_path);
    const SHELL_EXTENSIONS: &[&str] = &["lnk", "bat", "cmd", "url"];
    let needs_shell =
        exe_path.extension().and_then(|e| e.to_str()).is_some_and(|e| SHELL_EXTENSIONS.iter().any(|c| e.eq_ignore_ascii_case(c)));
    if needs_shell {
        launch_via_shell(exe_path, cwd)
    } else {
        Command::new(exe_path).current_dir(cwd).spawn().map(LaunchedProcess::Native)
    }
}

#[cfg(target_os = "windows")]
fn launch_via_shell(target_path: &Path, cwd: &Path) -> io::Result<LaunchedProcess> {
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

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

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "linux")]
pub fn launch(exe_path: &Path) -> io::Result<LaunchedProcess> {
    use std::os::unix::fs::PermissionsExt;

    let cwd = exe_path.parent().unwrap_or(exe_path);
    let mode = std::fs::metadata(exe_path)?.permissions().mode();
    let executable_bit =
        mode & 0o111 != 0 || std::fs::set_permissions(exe_path, std::fs::Permissions::from_mode(mode | 0o755)).is_ok();
    let child = if executable_bit {
        Command::new(exe_path).current_dir(cwd).spawn()?
    } else {
        Command::new("sh").arg(exe_path).current_dir(cwd).spawn()?
    };
    Ok(LaunchedProcess::Native(child))
}

#[cfg(target_os = "windows")]
fn shell_execute_open(target: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;
    let op = to_wide("open");
    let file = to_wide(target);
    unsafe {
        let _ = ShellExecuteW(None, PCWSTR(op.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

#[cfg(target_os = "linux")]
fn shell_execute_open(target: &str) {
    let _ = Command::new("xdg-open").arg(target).spawn();
}

pub fn open_url(url: &str) {
    shell_execute_open(url);
}

pub fn open_path(path: &Path) {
    shell_execute_open(&path.to_string_lossy());
}
