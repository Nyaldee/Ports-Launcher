//! Installation, lancement, et actions liées à une ligne/carte de port
//! (favori, dossier, désinstallation).

use super::dialogs::{
    dialog_is_open, open_error_dialog, open_message_dialog, open_progress_dialog, open_update_toggle_dialog, tr,
};
use super::events::{lock, AppEvent};
use super::playtime::is_port_running;
use super::state::AppState;
use crate::core::jobs::InstallOutcome;
use crate::core::models::{Port, SourceType};
use crate::core::executable_detect::ExecutableSelectionError;
use crate::ui::gamepad_router::GamepadRouter;
use crate::Tr;
use serde_json::Value;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

pub(crate) fn open_path_if_exists(path: &Path) {
    if path.exists() {
        crate::core::launch::open_path(path);
    }
}

/// Lance une install en tâche de fond -- ignore les activations répétées
/// pendant qu'une install pour CE port tourne déjà (`installing`).
///
/// `asset_override` : fichier choisi manuellement après une erreur
/// `Ambiguous`, contourne l'heuristique automatique pour cette tentative.
/// `release_override` : release choisie via `open_version_picker` (bouton
/// "Change version" dans Info), contourne "toujours la dernière" pour cette
/// tentative -- voir installer::install_port.
pub(crate) fn start_install(
    app: &Rc<AppState>,
    router: &Rc<RefCell<GamepadRouter>>,
    port: Port,
    asset_override: Option<Value>,
    release_override: Option<Value>,
) {
    let key = port.key().to_string();
    // Ne jamais écraser les fichiers d'un port en cours d'exécution -- vaut
    // pour un premier install comme pour une mise à jour, les deux passant
    // par ici.
    if app.install_runtime.installing.borrow().contains(&key) || is_port_running(app, &key) {
        return;
    }
    app.install_runtime.installing.borrow_mut().insert(key.clone());

    let window = app.window();
    let tr = window.global::<crate::Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_installing(), &tr.invoke_progress_installing(port.name.clone().into()));

    if app.stress_test {
        // --visual-stress-test : aucun téléchargement réel -- crée un
        // dossier et un "game.exe" au contenu arbitraire plutôt que de
        // passer par core::jobs::run_install.
        let dest_dir = app.paths.library_dir.join(&port.folder);
        let _ = std::fs::create_dir_all(&dest_dir);
        let _ = std::fs::write(dest_dir.join("game.exe"), b"not a real executable -- visual stress test");
        lock(&app.events).push(AppEvent::InstallDone { key, tag: Some("v1.0.0".to_string()), pin_version: false });
        return;
    }

    // Un release_override vient forcément de "Change version" (voir
    // open_version_picker) -- calculé ici, avant que le thread ci-dessous
    // n'en prenne possession, pour que le handler d'InstallDone sache
    // désactiver l'auto-MAJ de ce port SANS avoir à distinguer les appelants
    // de start_install lui-même.
    let pin_version = release_override.is_some();
    let (github_token, gitlab_token) = {
        let s = app.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    };
    let library_dir = app.paths.library_dir.clone();
    let cache_dir = app.paths.cache_dir.clone();
    let saves_backup_dir = app.paths.saves_backup_dir.clone();
    let events = app.events.clone();
    let progress_key = key.clone();

    std::thread::spawn(move || {
        let events_progress = events.clone();
        let mut on_progress = move |message: &str| {
            lock(&events_progress).push(AppEvent::InstallProgress { message: message.to_string() });
        };
        let overrides = crate::core::installer::InstallOverrides { asset: asset_override.as_ref(), release: release_override.as_ref() };
        let paths = crate::core::installer::InstallPaths { library_dir: &library_dir, cache_dir: &cache_dir, saves_backup_dir: &saves_backup_dir };
        let outcome = crate::core::jobs::run_install(&port, paths, github_token.as_deref(), gitlab_token.as_deref(), overrides, &mut on_progress);
        let event = match outcome {
            InstallOutcome::Done { tag } => AppEvent::InstallDone { key: progress_key, tag, pin_version },
            InstallOutcome::AssetAmbiguous { assets } => AppEvent::InstallAssetAmbiguous { key: progress_key, assets },
            InstallOutcome::Error(message) => AppEvent::InstallError { key: progress_key, message },
        };
        lock(&events).push(event);
    });
}

/// Bouton "Change version" de l'InfoDialog -- récupère en arrière-plan les
/// 3 dernières releases GitHub/GitLab de `port`, puis ouvre un
/// `ListPickerDialog` (voir `AppEvent::VersionsFetched`) pour choisir
/// laquelle installer. Même chemin que le choix d'asset ambigu, avec
/// `release_override` au lieu d'`asset_override`.
pub(crate) fn open_version_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let key = port.key().to_string();
    if app.install_runtime.installing.borrow().contains(&key) || is_port_running(app, &key) {
        return;
    }
    // Réserve la clé comme start_install -- le ProgressDialog ouvert juste
    // après masque déjà le bouton, mais ça protège d'un double-fetch si un
    // futur appelant atteint ce chemin autrement.
    app.install_runtime.installing.borrow_mut().insert(key.clone());
    let window = app.window();
    let tr = window.global::<crate::Tr>();
    open_progress_dialog(app, router, &tr.invoke_dialog_title_loading(), &tr.invoke_progress_fetching_versions(port.name.clone().into()));

    let (github_token, gitlab_token) = {
        let s = app.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    };
    let events = app.events.clone();
    let repo = port.repo.clone().unwrap_or_default();
    let source_type = port.source_type;

    std::thread::spawn(move || {
        let result = match source_type {
            SourceType::Github => crate::core::github_api::list_releases(&repo, github_token.as_deref(), 3).map_err(|e| e.message().to_string()),
            SourceType::Gitlab => crate::core::gitlab_api::list_releases(&repo, gitlab_token.as_deref(), 3).map_err(|e| e.message().to_string()),
            SourceType::DirectUrl | SourceType::Local => Err("This source has no version history.".to_string()),
        };
        let event = match result {
            Ok(releases) => AppEvent::VersionsFetched { key, releases },
            Err(message) => AppEvent::VersionsFetchError { key, message },
        };
        lock(&events).push(event);
    });
}

/// Bouton "favori" de l'InfoDialog -- scanne les exécutables candidats du
/// dossier du port (même détection que le Play ambigu, voir
/// `autodetect_executable`) et laisse choisir lequel sera lancé directement
/// aux prochains Play, sans repasser par le picker (voir
/// `set_favorite_exe`). Tout est local : pas de thread, pas de réseau.
pub(crate) fn open_favorite_exe_picker(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: Port) {
    let Ok(game_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else { return };
    if !game_dir.exists() {
        return;
    }
    let candidates: Vec<PathBuf> = match crate::core::executable_detect::autodetect_executable(&game_dir) {
        Ok(single) => vec![single],
        Err(ExecutableSelectionError::Ambiguous(_, candidates)) => candidates,
        Err(ExecutableSelectionError::Message(message)) => {
            open_message_dialog(app, router, &tr!(app).invoke_dialog_title_no_executable_found(), &message);
            return;
        }
    };
    // "Ask every time" en tête de liste -- efface le favori défini
    // (set_favorite_exe(key, None)) pour revenir au comportement par défaut
    // sans désinstaller/réinstaller.
    let mut labels = vec![tr!(app).invoke_picker_ask_every_time().to_string()];
    labels.extend(candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()));
    let key = port.key().to_string();
    // Stocké RELATIF à game_dir, jamais en absolu : déplacer le dossier
    // Ports Launcher (donc library_dir) casserait sinon silencieusement
    // tous les favoris déjà choisis. Rejoint à game_dir au moment du Play
    // (voir launch_flow).
    let game_dir_owned = game_dir.clone();
    super::dialogs::open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_favorite_executable(), labels, move |app, _router, idx| {
        let exe = if idx == 0 {
            None
        } else {
            candidates.get(idx - 1).and_then(|p| p.strip_prefix(&game_dir_owned).ok()).map(|p| p.to_string_lossy().to_string())
        };
        app.state.borrow_mut().set_favorite_exe(&key, exe);
    });
}

/// Retélécharge la jaquette de `port` si son fichier de cache a disparu --
/// normalement rempli à l'Install/Update, mais il peut avoir été supprimé à
/// la main ou un premier téléchargement avoir échoué silencieusement (voir
/// `cache_image`, best-effort). Déclenché au clic sur Jouer sans jamais
/// bloquer le lancement : la vérification est un `Path::exists()` local et
/// le téléchargement tourne sur un thread séparé.
pub(crate) fn repair_missing_cached_image(app: &Rc<AppState>, port: &Port) {
    let Some(url) = port.image.clone() else { return };
    let Ok(dest) = crate::core::image_cache::cached_image_path(&app.paths.cache_dir, &port.folder) else { return };
    if dest.exists() {
        return;
    }
    let cache_dir = app.paths.cache_dir.clone();
    let folder = port.folder.clone();
    let events = app.events.clone();
    std::thread::spawn(move || {
        crate::core::image_cache::cache_image(&url, &cache_dir, &folder);
        if crate::core::image_cache::cached_image_path(&cache_dir, &folder).map(|p| p.exists()).unwrap_or(false) {
            lock(&events).push(AppEvent::ImageCached { folder });
        }
    });
}

pub(crate) fn launch_executable(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port, exe: &Path) {
    // Point d'entrée UNIQUE de tous les chemins de lancement (clic, Entrée,
    // manette, choix d'exécutable ambigu) -- le seul endroit où ce garde-fou
    // contre les activations répétées doit vivre.
    if is_port_running(app, port.key()) {
        return;
    }
    if !exe.exists() {
        open_error_dialog(app, router, port.clone());
        return;
    }
    if app.stress_test {
        // --visual-stress-test : ne tente jamais de lancer le faux
        // "game.exe" créé par start_install -- son contenu arbitraire fait
        // apparaître la boîte système "application 16 bits non prise en
        // charge" (Windows passe par NTVDM avant de renvoyer une erreur),
        // que le driver ne peut pas fermer sans simuler un clic au niveau
        // OS, ce qu'il s'interdit.
        return;
    }
    if let Ok(child) = crate::core::launch::launch(exe) {
        app.install_runtime.running_processes.borrow_mut().insert(port.key().to_string(), child);
        app.install_runtime.launch_started_at.borrow_mut().insert(port.key().to_string(), Instant::now());
        repair_missing_cached_image(app, port);
        // Se minimiser en plein écran : notre fenêtre couvre tout l'écran
        // sans être un mode exclusif OS, et Windows ne redonne pas toujours
        // le premier plan au jeu qui démarre -- il se retrouverait ouvert
        // mais caché derrière nous. `poll_app_events` remonte la fenêtre dès
        // qu'aucun jeu ne tourne plus.
        if app.window().get_big_mode() {
            app.window().window().set_minimized(true);
            app.install_runtime.minimized_for_game.set(true);
        }
    }
}

pub(crate) fn launch_flow(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    let Ok(game_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else {
        let window = app.window();
        let tr = window.global::<crate::Tr>();
        open_message_dialog(app, router, &tr.invoke_dialog_title_invalid_port(), &tr.invoke_message_invalid_folder_name());
        return;
    };
    if !game_dir.exists() {
        open_error_dialog(app, router, port.clone());
        return;
    }
    // Exécutable favori choisi par l'utilisateur (voir
    // open_favorite_exe_picker) -- stocké relatif à game_dir, rejoint ici
    // via safe_join, même garde-fou contre une entrée qui sortirait du
    // dossier que pour le champ "executable" de ports.json. Revalidé sur
    // disque à CHAQUE Play : périmé ou disparu (dossier réinstallé, version
    // changée), on retombe sur la détection normale sans bloquer le Play.
    if let Some(favorite) = app.state.borrow().get(port.key()).and_then(|i| i.favorite_exe.clone()) {
        if let Ok(path) = crate::core::path_safety::safe_join(&game_dir, &favorite) {
            if path.exists() {
                launch_executable(app, router, port, &path);
                return;
            }
        }
    }
    match crate::core::executable_detect::resolve_executable(port.executable.as_ref(), &game_dir) {
        Ok(exe) => launch_executable(app, router, port, &exe),
        Err(ExecutableSelectionError::Ambiguous(_, candidates)) => {
            let labels: Vec<String> =
                candidates.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()).collect();
            let port2 = port.clone();
            super::dialogs::open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_executable(), labels, move |app, router, idx| {
                if let Some(exe) = candidates.get(idx) {
                    launch_executable(app, router, &port2, exe);
                }
            });
        }
        // ports.json mal configuré ("executable" introuvable) plutôt qu'une
        // install incomplète -- MessageDialog et non ErrorDialog, dont le
        // bouton "Reinstall" ne réglerait rien ici.
        Err(ExecutableSelectionError::Message(message)) => {
            open_message_dialog(app, router, &tr!(app).invoke_dialog_title_executable_not_found(), &message)
        }
    }
}

/// Récupère le port à `index` dans la liste fenêtrée affichée et applique
/// `action` dessus -- corps commun des callbacks `on_list_row_*_requested`.
///
/// Le `let` séparé, hors du scrutinee du `if let`, est nécessaire : un
/// emprunt créé DANS le scrutinee reste vivant pendant tout le bloc, donc un
/// `action` qui ré-emprunte `displayed_windowed` (delete_port ->
/// refresh_current_view -> rebuild_windowed, qui fait un `borrow_mut()`)
/// paniquerait sur "already borrowed" -- et un panic dans un callback Slint
/// ferme l'application. Uninstall, synchrone jusqu'au rebuild, est le seul
/// bouton directement exposé.
pub(crate) fn with_indexed_port(app: &Rc<AppState>, index: i32, action: impl FnOnce(Port)) {
    let port = app.windowed_nav.displayed_windowed.borrow().get(index as usize).cloned();
    if let Some(port) = port {
        action(port);
    }
}

/// Bouton "Install" d'une ligne non installée -- SEUL appelant restant
/// depuis que le badge "Update" (voir `open_update_toggle_dialog_row`
/// ci-dessous) ouvre le dialogue de bascule au lieu de lancer un install
/// directement. Reste aussi emprunté par un install déclenché depuis Play
/// sur un port jamais installé (voir `activate_port`).
pub(crate) fn install_row(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, index: i32) {
    if dialog_is_open(app) {
        return;
    }
    with_indexed_port(app, index, |port| start_install(app, router, port, None, None));
}

/// Badge "Update" barré d'une ligne (voir PortItem.auto-update-off et
/// app-window.slint::update-btn) -- ce badge ne signale qu'un seul cas
/// (auto-MAJ désactivée pour ce port), donc cliquer dessus propose toujours
/// d'activer/désactiver l'auto-MAJ, jamais un install direct -- même
/// dialogue que le bouton "Update" d'InfoDialog.
pub(crate) fn open_update_toggle_dialog_row(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, index: i32) {
    if dialog_is_open(app) {
        return;
    }
    with_indexed_port(app, index, |port| open_update_toggle_dialog(app, router, port));
}

/// Point d'entrée unique de "activer la sélection courante" -- réutilisé par
/// le clavier (Entrée), la souris (clic sur une ligne/carte) et la manette
/// (A/Start).
pub(crate) fn activate_port(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    if dialog_is_open(app) || app.install_runtime.installing.borrow().contains(port.key()) {
        return;
    }
    if crate::core::installer::is_installed(port, &app.paths.library_dir) {
        launch_with_update_check(app, router, port);
    } else {
        start_install(app, router, port.clone(), None, None);
    }
}

/// Vérifie une éventuelle mise à jour juste avant de lancer un port -- mais
/// seulement si ça vaut le coût réseau : source GitHub/GitLab, auto-MAJ
/// activée pour ce port (voir `InstalledInfo::update`/le bouton "Update"
/// d'InfoDialog), et install() vieille de plus de 24h (voir
/// `is_stale_for_update_check`) -- sinon lance directement, 0 requête API.
/// Un port en auto-MAJ désactivée ne passe JAMAIS ici, et n'est JAMAIS
/// vérifié par ailleurs (voir PortItem.auto-update-off) : c'est le seul
/// point d'entrée réseau pour un port. Le résultat ne bloque jamais le
/// Play : une erreur (rate limit, offline...) lance quand même la version
/// déjà installée (voir AppEvent::PlayUpdateChecked).
pub(crate) fn launch_with_update_check(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    let (tag, installed_at) = {
        let state = app.state.borrow();
        let should_check = state.release_sync
            && matches!(port.source_type, SourceType::Github | SourceType::Gitlab)
            && state.get(port.key()).map(|i| i.update).unwrap_or(true);
        if !should_check {
            drop(state);
            launch_flow(app, router, port);
            return;
        }
        let info = state.get(port.key());
        let installed_at = info.map(|i| i.installed_at.clone()).unwrap_or_default();
        if !crate::core::state::is_stale_for_update_check(&installed_at) {
            drop(state);
            launch_flow(app, router, port);
            return;
        }
        (info.and_then(|i| i.installed_tag.clone()), installed_at)
    };

    let (github_token, gitlab_token) = {
        let s = app.state.borrow();
        (s.github_token.clone(), s.gitlab_token.clone())
    };
    let port_owned = port.clone();
    let events = app.events.clone();
    std::thread::spawn(move || {
        let available = crate::core::jobs::run_update_check(&port_owned, tag.as_deref(), &installed_at, github_token.as_deref(), gitlab_token.as_deref())
            .unwrap_or(false);
        lock(&events).push(AppEvent::PlayUpdateChecked { port: port_owned, available });
    });
}

pub(crate) fn activate_selection(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if let Some(port) = app.current_selected_port() {
        activate_port(app, router, &port);
    }
}

pub(crate) fn show_info_for_current_selection(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    if let Some(port) = app.current_selected_port() {
        super::dialogs::open_info_dialog(app, router, &port);
    }
}

/// Maj+Entrée -- ouvre le dossier du port sélectionné dans l'Explorateur
/// plutôt que de le lancer. `game_dir.exists()` tient lieu de vérification
/// d'installation : c'est exactement la définition de
/// `core::installer::is_installed`. Silencieux si rien n'est
/// sélectionné/installé.
pub(crate) fn reveal_selected_folder(app: &Rc<AppState>) {
    let Some(port) = app.current_selected_port() else { return };
    let Ok(game_dir) = crate::core::path_safety::safe_join(&app.paths.library_dir, &port.folder) else { return };
    open_path_if_exists(&game_dir);
}

/// Seul point d'entrée de désinstallation (bouton "×" de
/// app-window.slint) -- ignore un port en cours d'installation, et affiche
/// une erreur si la suppression échoue plutôt que de l'avaler.
pub(crate) fn delete_port(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>, port: &Port) {
    // Jeu encore lancé : un `remove_dir_all` sur des fichiers verrouillés
    // par Windows échoue de toute façon, mais potentiellement après avoir
    // déjà supprimé une partie de l'arborescence -- mieux vaut une erreur
    // propre avant toute suppression.
    //
    // Un port LOCAL n'est jamais supprimé ici : app-window.slint route déjà
    // ces ports vers un bouton "Open Folder" séparé, la garde reste en
    // défense en profondeur comme dialog_is_open.
    if dialog_is_open(app) || app.install_runtime.installing.borrow().contains(port.key()) || is_port_running(app, port.key()) || port.source_type == SourceType::Local {
        return;
    }
    match crate::core::installer::uninstall_port(port, &app.paths.library_dir, &app.paths.saves_backup_dir) {
        Ok(()) => {
            app.state.borrow_mut().mark_removed(port.key());
            app.refresh_current_view();
        }
        Err(message) => open_message_dialog(app, router, &tr!(app).invoke_dialog_title_uninstall_error(), &message),
    }
}
