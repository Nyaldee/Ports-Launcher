//! *** NON TESTÉ *** -- écrit par anticipation d'un éventuel port Linux,
//! jamais compilé ni exécuté sur une vraie machine Linux (aucun poste de
//! dev Linux disponible au moment de l'écriture). Ne pas faire confiance à
//! ce fichier avant de l'avoir vérifié sur une vraie session X11/Wayland.
//!
//! Miroir de `windows_chrome.rs` : mêmes noms de fonction, même rôle
//! (focus/modal, icône, glissé de fenêtre no-frame), mais côté X11 via EWMH
//! (`x11rb`). Wayland n'expose délibérément aucun de ces mécanismes aux
//! clients (modèle de sécurité du compositeur -- voir la discussion sur
//! `force_foreground_window`/xdg-activation) : chaque fonction y est donc un
//! no-op documenté plutôt qu'une tentative qui échouerait silencieusement.
//!
//! Ouvre sa PROPRE connexion X11 (`x11rb::connect`) à chaque appel, plutôt
//! que de réutiliser celle de winit -- seul le numéro de fenêtre (u32) est
//! partagé via `raw-window-handle`, jamais la connexion elle-même (éviterait
//! une interop FFI non triviale entre deux bibliothèques XCB différentes).
//! Ces appels restent rares (ouverture/fermeture de dialogue, glissé de
//! fenêtre) : le coût d'une connexion à usage unique est négligeable ici.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, PropMode};

/// Fenêtre native, X11 (identifiant XCB) ou Wayland (aucune poignée
/// exploitable côté client pour ce qu'on veut en faire -- voir l'en-tête).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeWindow {
    X11(u32),
    Wayland,
}

/// Équivalent de `windows_chrome::native_hwnd` -- prend le `slint::Window`
/// plutôt qu'un composant concret, pour rester utilisable sur AppWindow
/// comme sur n'importe quel dialogue.
pub fn native_window(window: &slint::Window) -> Option<NativeWindow> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Xcb(h) => Some(NativeWindow::X11(h.window.get())),
        RawWindowHandle::Xlib(h) => Some(NativeWindow::X11(h.window as u32)),
        RawWindowHandle::Wayland(_) => Some(NativeWindow::Wayland),
        _ => None,
    }
}

fn intern(conn: &impl Connection, name: &str) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

/// Envoie un ClientMessage EWMH standard à la racine (mécanisme utilisé par
/// _NET_ACTIVE_WINDOW/_NET_WM_STATE/_NET_WM_MOVERESIZE) -- c'est ce que font
/// les pagers/barres des tâches pour agir sur une fenêtre qui n'est pas la
/// leur, plutôt qu'un appel direct qu'un gestionnaire de fenêtres ignorerait.
fn send_root_message(conn: &impl Connection, root: u32, window: u32, message_type: u32, data: [u32; 5]) -> Result<(), Box<dyn std::error::Error>> {
    let event = ClientMessageEvent::new(32, window, message_type, data);
    let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT;
    conn.send_event(false, root, mask, &event)?;
    conn.flush()?;
    Ok(())
}

fn active_window(conn: &impl Connection, root: u32) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let atom = intern(conn, "_NET_ACTIVE_WINDOW")?;
    let reply = conn.get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)?.reply()?;
    Ok(reply.value32().and_then(|mut it| it.next()))
}

/// Équivalent de `windows_chrome::set_window_enabled` -- Win32 a
/// `EnableWindow`, X11 n'a rien de tel : le repli documenté est de marquer
/// la fenêtre `_NET_WM_STATE_MODAL` par ClientMessage (EWMH), que la
/// majorité des gestionnaires de fenêtres respectent pour bloquer les clics
/// sur les fenêtres non-modales du même groupe. Best-effort seulement --
/// contrairement à Windows, rien ne garantit que CE gestionnaire de
/// fenêtres l'honore. Wayland : no-op, aucune notion de modalité côté
/// client (voir l'en-tête).
pub fn set_window_enabled(window: NativeWindow, enabled: bool) {
    let NativeWindow::X11(win) = window else { return };
    let Ok((conn, screen_num)) = x11rb::connect(None) else { return };
    let root = conn.setup().roots[screen_num].root;
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        let state = intern(&conn, "_NET_WM_STATE")?;
        let modal = intern(&conn, "_NET_WM_STATE_MODAL")?;
        // _NET_WM_STATE_REMOVE = 0, _NET_WM_STATE_ADD = 1 ; source
        // indication = 1 (application normale), voir la spec EWMH.
        let action = if enabled { 0 } else { 1 };
        send_root_message(&conn, root, win, state, [action, modal, 0, 1, 0])
    })();
}

/// Équivalent de `windows_chrome::force_foreground_window`. Contrairement à
/// Windows, X11 n'a pas de "foreground lock timeout" à contourner --
/// _NET_ACTIVE_WINDOW (ClientMessage EWMH, source indication 1) suffit,
/// SAUF si le gestionnaire de fenêtres applique lui-même une politique
/// anti-vol-de-focus (plusieurs le font par défaut) : best-effort, comme
/// sous Windows avec un mécanisme différent. Wayland : aucun équivalent
/// possible côté client sans jeton xdg-activation obtenu depuis une
/// interaction utilisateur récente -- no-op, voir la discussion qui a
/// motivé ce fichier.
pub fn force_foreground_window(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    let Ok((conn, screen_num)) = x11rb::connect(None) else { return };
    let root = conn.setup().roots[screen_num].root;
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        let atom = intern(&conn, "_NET_ACTIVE_WINDOW")?;
        send_root_message(&conn, root, win, atom, [1, 0, 0, 0, 0])
    })();
}

/// Équivalent de `windows_chrome::foreground_window_belongs_to_us` --
/// compare le PID du processus propriétaire de la fenêtre active
/// (_NET_WM_PID) à `std::process::id()`, même logique que côté Windows.
/// Wayland : aucune API d'introspection de la fenêtre active exposée aux
/// clients arbitraires (restriction volontaire du modèle de sécurité) --
/// retourne `false` par défaut (jamais assumer qu'on a le focus plutôt que
/// l'inverse, pour ne pas faire naviguer la manette en arrière-plan).
pub fn foreground_window_belongs_to_us() -> bool {
    let Ok((conn, screen_num)) = x11rb::connect(None) else { return false };
    let root = conn.setup().roots[screen_num].root;
    (|| -> Result<bool, Box<dyn std::error::Error>> {
        let Some(active) = active_window(&conn, root)? else { return Ok(false) };
        let pid_atom = intern(&conn, "_NET_WM_PID")?;
        let reply = conn.get_property(false, active, pid_atom, AtomEnum::CARDINAL, 0, 1)?.reply()?;
        Ok(reply.value32().and_then(|mut it| it.next()) == Some(std::process::id()))
    })()
    .unwrap_or(false)
}

/// Équivalent de `windows_chrome::is_foreground_window`. Wayland : jamais
/// interrogeable (voir `foreground_window_belongs_to_us`) -- `true` par
/// défaut ici plutôt que `false` : cette fonction sert à éviter de RE-forcer
/// le premier plan si on l'a déjà, un faux positif est donc sans risque
/// (juste un appel `force_foreground_window` superflu), contrairement à
/// `foreground_window_belongs_to_us` où un faux positif routerait la
/// manette vers la mauvaise fenêtre.
pub fn is_foreground_window(window: NativeWindow) -> bool {
    let NativeWindow::X11(win) = window else { return true };
    let Ok((conn, screen_num)) = x11rb::connect(None) else { return false };
    let root = conn.setup().roots[screen_num].root;
    active_window(&conn, root).ok().flatten() == Some(win)
}

fn message_box(title: &str, message: &str) {
    use std::process::Command;
    // zenity (GNOME/GTK) puis kdialog (KDE) -- les deux repli standard pour
    // une boîte de message sans dépendance directe à une bibliothèque GUI ;
    // eprintln! en dernier recours si aucun n'est installé (un exécutable
    // Linux lancé depuis un terminal ou un .desktop garde en général un
    // stderr consultable, contrairement au subsystem `windows` de la build
    // Windows -- voir windows_chrome::show_startup_error).
    if matches!(Command::new("zenity").args(["--error", "--title", title, "--text", message]).status(), Ok(s) if s.success())
    {
        return;
    }
    if matches!(Command::new("kdialog").args(["--title", title, "--error", message]).status(), Ok(s) if s.success()) {
        return;
    }
    eprintln!("{title}: {message}");
}

pub fn show_startup_error(message: &str) {
    message_box("Ports Launcher", message);
}

/// Voir le commentaire équivalent dans `windows_chrome::show_info`.
#[cfg(debug_assertions)]
pub fn show_info(title: &str, message: &str) {
    message_box(title, message);
}

/// TODO non implémenté. Sous Linux, l'icône de la barre des tâches/du
/// sélecteur de fenêtres vient normalement du fichier `.desktop` (clé
/// `Icon=`) apparié via `WM_CLASS`, pas d'un push explicite comme
/// `WM_SETICON` sous Windows. Un repli `_NET_WM_ICON` (propriété ARGB32 sur
/// la fenêtre) resterait possible mais suppose de décoder `Icon.ico` ici --
/// la feature "ico" du crate `image` a été délibérément retirée de
/// Cargo.toml (voir son commentaire) puisque rien ne l'utilisait jusqu'ici.
/// À réévaluer si ce repli s'avère nécessaire en pratique.
pub fn apply_window_icon(_window: NativeWindow) {}

/// Équivalent de `windows_chrome::force_alt_tab_visible` -- force
/// _NET_WM_WINDOW_TYPE à _NET_WM_WINDOW_TYPE_NORMAL : sans hint de type de
/// fenêtre, certains gestionnaires de fenêtres traitent une fenêtre
/// no-frame comme un utilitaire/popup absent du sélecteur de fenêtres.
/// Wayland : aucun contrôle client sur l'éligibilité au sélecteur de
/// fenêtres (décision entière du compositeur) -- no-op.
pub fn force_alt_tab_visible(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    let Ok((conn, _screen_num)) = x11rb::connect(None) else { return };
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        let wtype = intern(&conn, "_NET_WM_WINDOW_TYPE")?;
        let normal = intern(&conn, "_NET_WM_WINDOW_TYPE_NORMAL")?;
        conn.change_property32(PropMode::REPLACE, win, wtype, AtomEnum::ATOM, &[normal])?;
        conn.flush()?;
        Ok(())
    })();
}

/// Équivalent de `windows_chrome::own_window` -- `WM_TRANSIENT_FOR` est
/// l'équivalent X11 de `GWLP_HWNDPARENT` (relation de possession, pas un
/// vrai parentage), reconnu nativement par tous les gestionnaires de
/// fenêtres EWMH. Wayland : `xdg_toplevel.set_parent` ferait l'équivalent,
/// mais suppose une intégration directe au protocole Wayland de winit
/// plutôt qu'une connexion séparée comme ici -- no-op pour l'instant.
pub fn own_window(owned: NativeWindow, owner: NativeWindow) {
    let (NativeWindow::X11(owned), NativeWindow::X11(owner)) = (owned, owner) else { return };
    let Ok((conn, _screen_num)) = x11rb::connect(None) else { return };
    let _ = conn.change_property32(PropMode::REPLACE, owned, AtomEnum::WM_TRANSIENT_FOR, AtomEnum::WINDOW, &[owner]);
    let _ = conn.flush();
}

/// Équivalent de `windows_chrome::begin_window_drag` -- _NET_WM_MOVERESIZE
/// est le mécanisme EWMH standard pour déléguer un glissé interactif au
/// gestionnaire de fenêtres (position du curseur lue via `query_pointer`,
/// direction 8 = _NET_WM_MOVERESIZE_MOVE). Wayland : nécessiterait
/// `xdg_toplevel::_move` avec le SÉRIAL de l'évènement d'entrée d'origine,
/// une information que seule l'intégration wayland-client de winit possède
/// -- hors de portée d'une connexion X11 séparée comme celle-ci, no-op.
pub fn begin_window_drag(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    let Ok((conn, screen_num)) = x11rb::connect(None) else { return };
    let root = conn.setup().roots[screen_num].root;
    let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
        let pointer = conn.query_pointer(root)?.reply()?;
        let moveresize = intern(&conn, "_NET_WM_MOVERESIZE")?;
        const MOVERESIZE_MOVE: u32 = 8;
        let data = [pointer.root_x as u32, pointer.root_y as u32, MOVERESIZE_MOVE, 1, 1];
        send_root_message(&conn, root, win, moveresize, data)
    })();
}

/// Équivalent de `windows_chrome::double_click_time_ms`. Pas d'API système
/// universelle sous Linux (chaque environnement range ce réglage dans son
/// propre mécanisme -- XSettings pour GTK, kdeglobals pour KDE...) -- 500ms
/// est la valeur par défaut la plus répandue (GTK, et Windows lui-même) en
/// repli fixe plutôt que d'ajouter un client XSettings complet pour ça.
pub fn double_click_time_ms() -> u32 {
    500
}

/// Équivalent de `windows_chrome::enable_dpi_awareness`. Aucun opt-in
/// process requis sous Linux : X11 (XRandR) et Wayland (wl_output.scale)
/// communiquent l'échelle par sortie DIRECTEMENT à winit, sans notion
/// d'"awareness" explicite côté application comme sur Windows -- no-op.
pub fn enable_dpi_awareness() {}
