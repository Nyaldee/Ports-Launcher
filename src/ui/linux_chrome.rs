//! Miroir de `windows_chrome.rs` : mêmes noms de fonction, même rôle
//! (focus/modal, icône, glissé de fenêtre no-frame), mais côté X11 via EWMH
//! (`x11rb`). Wayland n'expose délibérément aucun de ces mécanismes aux
//! clients (modèle de sécurité du compositeur -- voir `force_foreground_window`
//! plus bas pour le cas xdg-activation) : chaque fonction y est donc un
//! no-op documenté plutôt qu'une tentative qui échouerait silencieusement.
//!
//! Connexion X11 PARTAGÉE pour tout le processus (voir `with_conn`), jamais
//! celle de winit -- seul le numéro de fenêtre est partagé via
//! `raw-window-handle` (éviterait une interop FFI non triviale entre deux
//! bibliothèques XCB). Une connexion/atome par appel serait coûteux pour
//! rien : réutilisés pour toute la session (voir `cached_atom`).

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

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

/// Connexion X11 + fenêtre racine + dimensions de l'écran par défaut,
/// résolues une seule fois à la connexion (voir `with_conn`).
struct Session {
    conn: RustConnection,
    root: u32,
    screen_width: i32,
    screen_height: i32,
}

thread_local! {
    /// `None` mis en cache si la connexion initiale échoue -- pas de
    /// nouvelle tentative à chaque appel suivant, un serveur X injoignable
    /// une fois l'est pour toute la session. `thread_local!` plutôt qu'un
    /// `static` global : tout ce fichier n'est appelé que depuis le thread UI
    /// (comme le reste de l'appli, voir `AppState`), pas besoin de
    /// synchronisation entre threads.
    static SESSION: OnceCell<Option<Session>> = OnceCell::new();
    /// Atomes EWMH déjà résolus (voir `cached_atom`) -- un `intern_atom` est
    /// lui aussi un aller-retour réseau, la même raison que la connexion
    /// partagée ci-dessus.
    static ATOMS: RefCell<HashMap<&'static str, u32>> = RefCell::new(HashMap::new());
}

/// Exécute `f` avec la session X11 partagée, si une connexion a pu être
/// établie -- `None` sinon (serveur X injoignable, ou fenêtre Wayland en
/// amont), auquel cas l'appelant se comporte comme un no-op.
fn with_conn<T>(f: impl FnOnce(&Session) -> T) -> Option<T> {
    SESSION.with(|cell| {
        let session = cell.get_or_init(|| {
            let (conn, screen_num) = x11rb::connect(None).ok()?;
            // Trois accès `conn.setup().roots[screen_num]` séparés plutôt
            // qu'une référence `screen` retenue le temps de lire ses trois
            // champs : chaque accès reste un emprunt ponctuel et éteint avant
            // le `Session { conn, .. }` qui déplace `conn` juste après (même
            // idiome que le reste de ce fichier avant cette mise en cache).
            let root = conn.setup().roots[screen_num].root;
            let screen_width = conn.setup().roots[screen_num].width_in_pixels as i32;
            let screen_height = conn.setup().roots[screen_num].height_in_pixels as i32;
            Some(Session { conn, root, screen_width, screen_height })
        });
        session.as_ref().map(f)
    })
}

fn intern(conn: &RustConnection, name: &str) -> Option<u32> {
    conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom)
}

/// Atome EWMH mis en cache pour toute la session -- jamais réinterné à
/// chaque appel (voir le commentaire de `ATOMS`).
fn cached_atom(conn: &RustConnection, name: &'static str) -> Option<u32> {
    if let Some(cached) = ATOMS.with(|c| c.borrow().get(name).copied()) {
        return Some(cached);
    }
    let value = intern(conn, name)?;
    ATOMS.with(|c| c.borrow_mut().insert(name, value));
    Some(value)
}

/// Envoie un ClientMessage EWMH standard à la racine (mécanisme utilisé par
/// _NET_ACTIVE_WINDOW/_NET_WM_STATE/_NET_WM_MOVERESIZE) -- c'est ce que font
/// les pagers/barres des tâches pour agir sur une fenêtre qui n'est pas la
/// leur, plutôt qu'un appel direct qu'un gestionnaire de fenêtres ignorerait.
fn send_root_message(conn: &RustConnection, root: u32, window: u32, message_type: u32, data: [u32; 5]) -> Option<()> {
    let event = ClientMessageEvent::new(32, window, message_type, data);
    let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT;
    conn.send_event(false, root, mask, &event).ok()?;
    conn.flush().ok()?;
    Some(())
}

fn active_window(conn: &RustConnection, root: u32) -> Option<u32> {
    let atom = cached_atom(conn, "_NET_ACTIVE_WINDOW")?;
    let reply = conn.get_property(false, root, atom, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
    reply.value32().and_then(|mut it| it.next())
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
    with_conn(|s| {
        (|| -> Option<()> {
            let state = cached_atom(&s.conn, "_NET_WM_STATE")?;
            let modal = cached_atom(&s.conn, "_NET_WM_STATE_MODAL")?;
            // _NET_WM_STATE_REMOVE = 0, _NET_WM_STATE_ADD = 1 ; source
            // indication = 1 (application normale), voir la spec EWMH.
            let action = if enabled { 0 } else { 1 };
            send_root_message(&s.conn, s.root, win, state, [action, modal, 0, 1, 0])
        })();
    });
}

/// Équivalent de `windows_chrome::force_foreground_window`. Contrairement à
/// Windows, X11 n'a pas de "foreground lock timeout" à contourner --
/// _NET_ACTIVE_WINDOW (ClientMessage EWMH, source indication 1) suffit,
/// SAUF si le gestionnaire de fenêtres applique lui-même une politique
/// anti-vol-de-focus (plusieurs le font par défaut) : best-effort, comme
/// sous Windows avec un mécanisme différent. Wayland : aucun équivalent
/// possible côté client sans jeton xdg-activation obtenu depuis une
/// interaction utilisateur récente -- no-op.
pub fn force_foreground_window(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    with_conn(|s| {
        (|| -> Option<()> {
            let atom = cached_atom(&s.conn, "_NET_ACTIVE_WINDOW")?;
            send_root_message(&s.conn, s.root, win, atom, [1, 0, 0, 0, 0])
        })();
    });
}

/// Équivalent de `windows_chrome::foreground_window_belongs_to_us` --
/// compare le PID du processus propriétaire de la fenêtre active
/// (_NET_WM_PID) à `std::process::id()`, même logique que côté Windows.
/// Wayland : aucune API d'introspection de la fenêtre active exposée aux
/// clients arbitraires (restriction volontaire du modèle de sécurité) --
/// retourne `false` par défaut (jamais assumer qu'on a le focus plutôt que
/// l'inverse, pour ne pas faire naviguer la manette en arrière-plan).
pub fn foreground_window_belongs_to_us() -> bool {
    with_conn(|s| -> Option<bool> {
        let active = active_window(&s.conn, s.root)?;
        let pid_atom = cached_atom(&s.conn, "_NET_WM_PID")?;
        let reply = s.conn.get_property(false, active, pid_atom, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
        Some(reply.value32().and_then(|mut it| it.next()) == Some(std::process::id()))
    })
    .flatten()
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
    with_conn(|s| active_window(&s.conn, s.root) == Some(win)).unwrap_or(false)
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

/// Non implémenté délibérément. Sous Linux, l'icône de la barre des
/// tâches/du sélecteur de fenêtres vient normalement du fichier `.desktop`
/// (clé `Icon=`) apparié via `WM_CLASS`, pas d'un push explicite comme
/// `WM_SETICON` sous Windows. Un repli `_NET_WM_ICON` (propriété ARGB32 sur
/// la fenêtre) resterait possible mais suppose de décoder `Icon.ico` ici --
/// la feature "ico" du crate `image` a été délibérément retirée de
/// Cargo.toml (voir son commentaire) puisque rien ne l'utilisait jusqu'ici.
pub fn apply_window_icon(_window: NativeWindow) {}

/// Équivalent de `windows_chrome::force_alt_tab_visible` -- force
/// _NET_WM_WINDOW_TYPE à _NET_WM_WINDOW_TYPE_NORMAL : sans hint de type de
/// fenêtre, certains gestionnaires de fenêtres traitent une fenêtre
/// no-frame comme un utilitaire/popup absent du sélecteur de fenêtres.
/// Wayland : aucun contrôle client sur l'éligibilité au sélecteur de
/// fenêtres (décision entière du compositeur) -- no-op.
pub fn force_alt_tab_visible(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    with_conn(|s| {
        (|| -> Option<()> {
            let wtype = cached_atom(&s.conn, "_NET_WM_WINDOW_TYPE")?;
            let normal = cached_atom(&s.conn, "_NET_WM_WINDOW_TYPE_NORMAL")?;
            s.conn.change_property32(PropMode::REPLACE, win, wtype, AtomEnum::ATOM, &[normal]).ok()?;
            s.conn.flush().ok()?;
            Some(())
        })();
    });
}

/// Équivalent de `windows_chrome::own_window` -- `WM_TRANSIENT_FOR` est
/// l'équivalent X11 de `GWLP_HWNDPARENT` (relation de possession, pas un
/// vrai parentage), reconnu nativement par tous les gestionnaires de
/// fenêtres EWMH. Wayland : `xdg_toplevel.set_parent` ferait l'équivalent,
/// mais suppose une intégration directe au protocole Wayland de winit
/// plutôt qu'une connexion séparée comme ici -- no-op.
pub fn own_window(owned: NativeWindow, owner: NativeWindow) {
    let (NativeWindow::X11(owned), NativeWindow::X11(owner)) = (owned, owner) else { return };
    with_conn(|s| {
        let _ = s.conn.change_property32(PropMode::REPLACE, owned, AtomEnum::WM_TRANSIENT_FOR, AtomEnum::WINDOW, &[owner]);
        let _ = s.conn.flush();
    });
}

/// Équivalent de `windows_chrome::begin_window_drag` -- _NET_WM_MOVERESIZE
/// est le mécanisme EWMH standard pour déléguer un glissé interactif au
/// gestionnaire de fenêtres (position du curseur lue via `query_pointer`,
/// direction 8 = _NET_WM_MOVERESIZE_MOVE). Wayland : nécessiterait
/// `xdg_toplevel::_move` avec le SÉRIAL de l'évènement d'entrée d'origine,
/// une information que seule l'intégration wayland-client de winit possède
/// -- hors de portée d'une connexion X11 séparée comme celle-ci, no-op.
///
/// Best-effort : certains gestionnaires de fenêtres (xfwm4) ignorent ce
/// message sans qu'aucune erreur ne remonte (un ClientMessage n'a pas de
/// mécanisme de retour d'échec). Repli manuel dans ce cas (voir
/// `cursor_position` plus bas et `main.rs::on_window_drag_moved`) -- sans
/// conflit : un WM qui honore réellement ce message prend son propre grab
/// du pointeur, ce qui arrête déjà les `moved` dont le repli a besoin.
///
/// Pas de `ungrab_pointer` (contrairement à `ReleaseCapture()` côté
/// Windows) : casserait le grab implicite de Slint/winit dont le repli
/// manuel dépend, pour un gain qu'aucun WM testé ne concrétise.
pub fn begin_window_drag(window: NativeWindow) {
    let NativeWindow::X11(win) = window else { return };
    with_conn(|s| {
        let Some(pointer) = s.conn.query_pointer(s.root).ok().and_then(|c| c.reply().ok()) else { return };
        let Some(moveresize) = cached_atom(&s.conn, "_NET_WM_MOVERESIZE") else { return };
        const MOVERESIZE_MOVE: u32 = 8;
        let data = [pointer.root_x as u32, pointer.root_y as u32, MOVERESIZE_MOVE, 1, 1];
        let _ = send_root_message(&s.conn, s.root, win, moveresize, data);
    });
}

/// Position du pointeur en coordonnées RACINE (absolues à l'écran) --
/// utilisée par le repli de glissé (voir `main.rs::on_window_drag_moved`)
/// à la place de `mouse-x`/`mouse-y` de Slint, relatifs à la fenêtre elle-
/// même et donc à un repère qui bouge avec elle.
pub fn cursor_position() -> Option<(i32, i32)> {
    with_conn(|s| s.conn.query_pointer(s.root).ok()?.reply().ok().map(|r| (r.root_x as i32, r.root_y as i32))).flatten()
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

/// Équivalent de `windows_chrome::work_area_under_cursor`. Pas de notion de
/// "moniteur sous le curseur" interrogeable sans XRandR -- mais la zone de
/// travail (hors dock/panel) EST disponible via `_NET_WORKAREA` (EWMH,
/// exposée par tout bureau conforme, dont GNOME) : sans elle, le mode plein
/// écran couvrait tout l'écran physique et débordait derrière un dock
/// ancré (ex: le dock GNOME à gauche), poussant la fenêtre hors champ.
/// Repli sur la géométrie de l'écran entier si la propriété est absente ou
/// le bureau non-EWMH, ou sur 1920x1080 si la connexion elle-même échoue.
/// Sert uniquement à la géométrie INITIALE de la fenêtre (voir main.rs) :
/// winit/Slint replacent de toute façon la fenêtre sur le bon moniteur une
/// fois réellement mappée.
pub fn work_area_under_cursor() -> (i32, i32, i32, i32) {
    with_conn(|s| {
        let resolved: Option<(i32, i32, i32, i32)> = (|| {
            let atom = cached_atom(&s.conn, "_NET_WORKAREA")?;
            let reply = s.conn.get_property(false, s.root, atom, AtomEnum::CARDINAL, 0, 4).ok()?.reply().ok()?;
            let mut values = reply.value32()?;
            // Toujours la zone du 1er bureau virtuel -- un futur repli sur
            // `_NET_CURRENT_DESKTOP` n'aurait d'intérêt qu'avec plusieurs
            // bureaux ayant CHACUN un dock/panel différent, cas non
            // rencontré en pratique.
            let (x, y, w, h) = (values.next(), values.next(), values.next(), values.next());
            match (x, y, w, h) {
                (Some(x), Some(y), Some(w), Some(h)) if w > 0 && h > 0 => Some((x as i32, y as i32, w as i32, h as i32)),
                _ => None,
            }
        })();
        resolved.unwrap_or((0, 0, s.screen_width, s.screen_height))
    })
    .unwrap_or((0, 0, 1920, 1080))
}

/// Équivalent de `windows_chrome::scale_factor_under_cursor`. X11 sans
/// XRandR ne donne pas de facteur d'échelle logique fiable avant qu'une
/// fenêtre existe, et Wayland le communique à winit directement, pas ici --
/// 1.0 par défaut : `main.rs` relit `window.scale_factor()` une fois la
/// fenêtre réellement affichée, seule source fiable ensuite.
pub fn scale_factor_under_cursor() -> f32 {
    1.0
}
