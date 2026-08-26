//! Détail natif Windows (HWND) pour la fenêtre principale et les dialogues
//! -- focus/modal, icône, DPI, géométrie de moniteur, glissé de fenêtre.
//! Aucune dépendance à `AppState`/`DialogSlot` : uniquement des
//! `HWND`/`slint::Window`, pour rester indépendant de la logique d'appli.
//! `core::platform_resolve` garde son propre accès Win32 séparé (dossiers
//! connus via `SHGetKnownFolderPath`) -- sans rapport avec une fenêtre, pas
//! fusionné ici pour ne pas transformer ce fichier en fourre-tout Win32.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetDoubleClickTime, ReleaseCapture};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, MessageBoxW,
    PostMessageW, SendMessageW, SetForegroundWindow, SetProcessDPIAware, SetWindowLongPtrW, SetWindowPos,
    GWLP_HWNDPARENT, GWL_EXSTYLE, HICON, HTCAPTION, ICON_BIG, ICON_SMALL, MB_ICONERROR, MB_OK, MESSAGEBOX_STYLE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_LBUTTONUP, WM_NCLBUTTONDOWN, WM_SETICON,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

/// Sans ça, Windows applique un redimensionnement bitmap flou à toute la
/// fenêtre sur un écran mis à l'échelle (125%/150%/200%...), avec le
/// contenu qui apparaît ~2x trop grand par rapport à la fenêtre.
///
/// "Per-Monitor V2" (`SetProcessDpiAwarenessContext`, Windows 10 1703+)
/// d'abord : v1 (`SetProcessDpiAwareness`) ne suffit pas au backend winit
/// de Slint pour détecter le DPI réel -- `slint::Window::scale_factor()`
/// reste bloqué à 1.0 sur un écran à 200% alors que la géométrie Win32
/// brute reste exacte, ce qui laisse le contenu rendu bien plus grand que
/// la fenêtre qui le contient. Repli en cascade (v2 -> v1 -> API user32
/// historique) pour les versions de Windows plus anciennes.
pub fn enable_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwareness, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        PROCESS_PER_MONITOR_DPI_AWARE,
    };
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err()
            && SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE).is_err()
        {
            let _ = SetProcessDPIAware();
        }
    }
}

/// Moniteur sous le curseur, ou le plus proche si celui-ci est hors écran --
/// support de `work_area_under_cursor`/`scale_factor_under_cursor`.
fn monitor_under_cursor() -> HMONITOR {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
    }
}

/// Zone de travail (x, y, largeur, hauteur) du moniteur sous le curseur --
/// pas `GetSystemMetrics` (résolution brute, barre des tâches comprise) ni
/// l'écran principal : en multi-écrans, la fenêtre doit s'ouvrir et se
/// centrer sur le moniteur où se trouve la souris.
///
/// Ne calcule pas le facteur d'échelle DPI : une fois la fenêtre affichée,
/// main.rs lit celui de Slint. `GetDpiForMonitor` peut diverger de ce que
/// Slint applique en interne, ce qui laisse le contenu rendu à la moitié de
/// la largeur d'une fenêtre par ailleurs correctement dimensionnée.
pub fn work_area_under_cursor() -> (i32, i32, i32, i32) {
    let monitor = monitor_under_cursor();
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let r = info.rcWork;
        (r.left, r.top, (r.right - r.left).max(1), (r.bottom - r.top).max(1))
    } else {
        (0, 0, 1920, 1080)
    }
}

/// Facteur d'échelle DPI (1.0 = 100%, 1.5 = 150%...) du moniteur sous le
/// curseur. Réservé au seul instant précédant le premier `show()`, où la
/// source préférée (`GetDpiForWindow`, voir main.rs) n'est pas encore
/// utilisable faute de HWND ; ensuite main.rs relit le facteur réel de la
/// fenêtre et s'en sert pour tout le reste.
///
/// `initial-width`/`initial-height` sont des `<length>` Slint, donc en
/// pixels LOGIQUES : leur pousser la taille physique issue de
/// `GetMonitorInfoW` la fait remettre à l'échelle une seconde fois au rendu
/// (fenêtre `scale`x trop grande, et mal centrée puisque le centrage se
/// base sur la taille voulue). D'où la division ici, pendant du
/// `Theme.scale-factor` utilisé dans les .slint.
pub fn scale_factor_under_cursor() -> f32 {
    let monitor = monitor_under_cursor();
    unsafe {
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
            dpi_x as f32 / 96.0
        } else {
            1.0
        }
    }
}

/// HWND natif d'une fenêtre Slint quelconque -- prend le `slint::Window`
/// plutôt qu'un composant concret, pour rester utilisable sur AppWindow
/// comme sur n'importe quel dialogue. `raw-window-handle` (feature
/// `raw-window-handle-06` de `slint`) est le seul moyen documenté
/// d'obtenir ce HWND depuis l'API publique de Slint.
pub fn native_hwnd(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

/// Rend `hwnd` véritablement MODALE le temps qu'un dialogue est affiché --
/// `EnableWindow(hwnd, FALSE)` est le mécanisme natif des boîtes modales
/// Win32 (celui de `MessageBox`) : Windows refuse alors tout clic/frappe sur
/// cette fenêtre et refuse de la rendre active au clic.
pub fn set_window_enabled(hwnd: HWND, enabled: bool) {
    unsafe {
        let _ = EnableWindow(hwnd, enabled);
    }
}

/// Force `hwnd` au premier plan, y compris depuis un contexte SANS
/// évènement d'entrée récent (un `slint::Timer` en tâche de fond) --
/// `SetForegroundWindow` seul échoue silencieusement dans ce cas (Windows le
/// bloque : "foreground lock timeout", pour empêcher une appli en
/// arrière-plan de voler le focus). `AttachThreadInput` sur le thread de la
/// fenêtre actuellement au premier plan fait momentanément croire à Windows
/// que notre thread partage son état d'entrée, et `SetForegroundWindow`
/// réussit alors sans restriction -- contournement standard documenté.
pub fn force_foreground_window(hwnd: HWND) {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground == hwnd {
            return;
        }
        let current_thread_id = GetCurrentThreadId();
        let foreground_thread_id = if foreground.0.is_null() { 0 } else { GetWindowThreadProcessId(foreground, None) };
        let attached = foreground_thread_id != 0
            && foreground_thread_id != current_thread_id
            && AttachThreadInput(foreground_thread_id, current_thread_id, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(foreground_thread_id, current_thread_id, false);
        }
    }
}

/// Vrai si la fenêtre au premier plan appartient à NOTRE PROCESSUS --
/// fenêtre principale ou l'un de ses dialogues. XInput (donc `gilrs`, voir
/// core::gamepad) lit l'état de la manette au niveau SYSTÈME, sans notion de
/// fenêtre active : sans cette garde, le routeur manette naviguerait en
/// arrière-plan pendant qu'un jeu au premier plan lit la même manette.
/// Comparer le PROCESSUS et non le HWND de la fenêtre principale : un
/// dialogue est une fenêtre séparée qui prend légitimement le focus, une
/// comparaison stricte couperait toute navigation manette dedans.
pub fn foreground_window_belongs_to_us() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        pid == std::process::id()
    }
}

/// Vrai si `hwnd` est actuellement la fenêtre au premier plan.
pub fn is_foreground_window(hwnd: HWND) -> bool {
    unsafe { GetForegroundWindow() == hwnd }
}

/// Sans console (sous-système GUI, voir `main.rs`), un message sur stderr ne
/// serait jamais vu.
fn message_box(title: &str, message: &str, flags: MESSAGEBOX_STYLE) {
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags);
    }
}

pub fn show_startup_error(message: &str) {
    message_box("Ports Launcher", message, MB_OK | MB_ICONERROR);
}

/// Utilisé par le harnais de stress test visuel (`--visual-stress-test`,
/// `#[cfg(debug_assertions)]` -- absent des builds release) pour signaler la
/// fin du run sans dépendance directe à `windows::`.
#[cfg(debug_assertions)]
pub fn show_info(title: &str, message: &str) {
    message_box(title, message, MB_OK);
}

/// Extrait l'icône embarquée dans `ports_launcher.exe` (voir `build.rs`) et
/// la pousse via `WM_SETICON` -- `icon: @image-url("../Icon.ico")` dans
/// app-window.slint ne suffit pas pour la barre des tâches/Alt+Tab : Slint
/// décode l'image lui-même (un .ico est un conteneur multi-résolutions) et
/// n'en fait pas forcément un WM_SETICON natif sur une fenêtre no-frame.
/// `ExtractIconExW` sur le chemin de l'exe plutôt qu'une resource ID à
/// deviner. Utilisée pour la fenêtre principale ET les dialogues.
pub fn apply_window_icon(hwnd: HWND) {
    let Ok(exe_path) = std::env::current_exe() else { return };
    let wide: Vec<u16> = exe_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut large_icon = HICON::default();
        let mut small_icon = HICON::default();
        let extracted = ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large_icon), Some(&mut small_icon), 1);
        if extracted == 0 {
            return;
        }
        if !large_icon.is_invalid() {
            let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(large_icon.0 as isize)));
        }
        if !small_icon.is_invalid() {
            let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(small_icon.0 as isize)));
        }
    }
}

/// Force WS_EX_APPWINDOW et retire WS_EX_TOOLWINDOW -- règles documentées de
/// Windows pour qu'une fenêtre no-frame (pas de WS_CAPTION) reste éligible
/// au sélecteur Alt+Tab, en plus de la barre des tâches. Défensif (l'icône
/// Alt+Tab dépend surtout d'`apply_window_icon`), sans risque.
pub fn force_alt_tab_visible(hwnd: HWND) {
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = (ex_style | WS_EX_APPWINDOW.0 as isize) & !(WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        // Un changement de style étendu sur une fenêtre déjà visible n'est
        // pas garanti d'être repris par le gestionnaire de fenêtres sans
        // notification explicite -- SWP_FRAMECHANGED s'en charge.
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// Rend `owned` possédée par `owner` (relation de possession, pas un vrai
/// parentage) -- `EnableWindow(FALSE)` seul laisse `owner` sélectionnable
/// via Alt+Tab, qui la ramènerait au premier plan tout en la laissant
/// désactivée, un état incohérent qui plante l'appli. `owned` reste
/// néanmoins listée dans Alt+Tab (voir `apply_window_icon`/
/// `force_alt_tab_visible`).
pub fn own_window(owned: HWND, owner: HWND) {
    unsafe {
        SetWindowLongPtrW(owned, GWLP_HWNDPARENT, owner.0 as isize);
    }
}

/// Délègue le glissé ENTIER de `hwnd` à Windows (no-frame : rien ne le fait
/// nativement) -- `ReleaseCapture` + `WM_NCLBUTTONDOWN`/`HTCAPTION` évite un
/// recalcul manuel de position à chaque évènement `moved`, nettement plus
/// lent (chaque évènement traverse la boucle Slint et winit).
pub fn begin_window_drag(hwnd: HWND) {
    unsafe {
        let _ = ReleaseCapture();
        // SendMessageW est BLOQUANT : Windows gère tout le glissé dans sa
        // propre boucle modale et ne rend la main qu'au relâchement du
        // bouton, qui ne passe jamais par la file de messages normale de la
        // fenêtre -- winit (et donc Slint) ne le voit pas, plus aucune zone
        // ne répondrait au survol/clic après un glissé sans le
        // WM_LBUTTONUP synthétique posté ensuite (aucun clic réel déclenché,
        // faute de WM_LBUTTONDOWN correspondant).
        let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(0)));
        let _ = PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), LPARAM(0));
    }
}

/// Délai de double-clic configuré dans Windows (accessibilité) -- jamais une
/// valeur en dur.
pub fn double_click_time_ms() -> u32 {
    unsafe { GetDoubleClickTime() }
}
