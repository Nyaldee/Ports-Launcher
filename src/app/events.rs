//! File de messages produits par les threads d'arrière-plan (install, vérif
//! de MAJ, sync du catalogue) et leur application sur le thread UI.

use super::dialogs::{
    apply_theme, close_current_dialog, open_message_dialog, open_picker_dialog, resize_progress_dialog, tr, DialogSlot,
};
use super::install_launch::{launch_flow, start_install};
use super::playtime::{any_process_running, checkpoint_playtime, refresh_live_playtime_display};
use super::state::AppState;
use crate::core::models::Port;
use crate::ui::gamepad_router::GamepadRouter;
use crate::ui::windows_chrome;
use crate::Tr;
use serde_json::Value;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

/// Verrou tolérant à l'empoisonnement. `.lock().unwrap()` condamnerait le
/// Mutex dès qu'UN thread panique en le tenant (une vérif de MAJ sur une
/// réponse API inattendue, par exemple) : tout locker suivant, thread UI
/// compris, paniquerait à son tour et tuerait l'appli. Les données protégées
/// ici sont une simple file de messages, sans invariant qu'un producteur
/// mort en plein milieu pourrait laisser cassé -- les récupérer malgré
/// l'empoisonnement est donc sûr.
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Messages produits par les threads d'arrière-plan (install, vérif de MAJ,
/// sync du catalogue) -- que des données `Send`, jamais un `Rc`/composant
/// Slint : empilés dans `AppState.events` depuis n'importe quel thread,
/// dépilés et appliqués uniquement par `poll_app_events` sur le thread UI.
/// Ce détour évite d'avoir à rendre `Rc<AppState>` `Send` (il ne l'est pas)
/// et se contente du `slint::Timer` qui réveille déjà le thread UI, sans
/// `slint::invoke_from_event_loop`.
pub(crate) enum AppEvent {
    InstallProgress { message: String },
    /// `pin_version` -- vrai seulement pour un install lancé depuis "Change
    /// version" (voir `start_install`/`open_version_picker`) : le handler
    /// désactive alors l'auto-MAJ de ce port (voir `set_port_update`), pour
    /// ne pas se faire silencieusement écraser le pin au Play suivant.
    InstallDone { key: String, tag: Option<String>, pin_version: bool },
    /// `release_override` -- la release choisie via "Select version" qui a
    /// mené à cette ambiguïté, à repasser telle quelle une fois l'asset
    /// choisi manuellement (voir son handler) : sans ça, le pin de version
    /// (voir `InstallDone::pin_version`) se perdait silencieusement dès
    /// qu'un port avait plusieurs assets pour la même release -- notamment
    /// tout port GitLab à plusieurs plateformes (voir
    /// `asset_select::pick_asset`, dont le nom d'asset GitLab n'a jamais
    /// d'extension et tombait donc toujours en Ambiguous).
    InstallAssetAmbiguous { key: String, assets: Vec<Value>, release_override: Option<Value> },
    InstallError { key: String, message: String },
    /// Voir `app::install_launch::start_extra_install` -- installation à la
    /// demande des fichiers `extra` d'un port (bouton "Install extras"
    /// d'InfoDialog) terminée. `Ok` = fusionnés dans le dossier du port ;
    /// `Err(message)` = lien injoignable / archive illisible, rien n'a été
    /// touché. Un MessageDialog l'annonce dans les deux cas (voir son handler).
    ExtraInstallDone { key: String, result: Result<(), String> },
    /// Résultat du check déclenché par un clic sur Play (voir
    /// `launch_with_update_check`) -- porte le `Port` complet, pas juste sa
    /// clé : le handler doit soit lancer un install (`start_install` le veut
    /// par valeur) soit lancer le jeu directement, dans les deux cas sans
    /// second lookup dans `app.catalog`. `Box` : `Port` fait ~650 octets et
    /// gonflerait chaque variante de la file (`clippy::large_enum_variant`) --
    /// une seule indirection, dans le seul handler qui déballe cette variante.
    PlayUpdateChecked { port: Box<Port>, available: bool },
    SelfUpdateAvailable,
    /// Voir `open_version_picker` -- liste des releases disponibles pour
    /// `key` récupérée en arrière-plan, prête à peupler un `ListPickerDialog`.
    VersionsFetched { key: String, releases: Vec<Value> },
    VersionsFetchError { key: String, message: String },
    /// Voir `repair_missing_cached_image` -- la jaquette de `folder`
    /// vient d'être retéléchargée avec succès en arrière-plan, la grille
    /// doit relire le fichier au prochain rendu au lieu du repli texte déjà
    /// affiché.
    ImageCached { folder: String },
    /// Une seconde instance vient de se connecter au verrou loopback pour
    /// signaler sa présence -- voir le thread `accept()` dans `main()` et
    /// `SINGLE_INSTANCE_PORT`.
    BringToForeground,
    /// Voir `start_catalog_sync` -- un `ports.json` plus récent vient d'être
    /// téléchargé ET déjà validé (voir `catalog_sync::fetch_if_changed`),
    /// prêt à remplacer `app.catalog` et rafraîchir la vue affichée.
    RemoteCatalogFetched(Vec<Port>),
    /// Voir `start_catalog_sync` -- une vérification vient de se conclure
    /// (`304` ou `200`, jamais sur une erreur réseau) : remet le throttle à
    /// zéro depuis le thread UI, `StateManager` n'étant jamais muté depuis
    /// un thread d'arrière-plan.
    PortsCheckDone { etag: String },
    /// Voir `start_themes_sync` -- un `themes.json` plus récent vient d'être
    /// téléchargé, validé, et déjà écrit sur disque -- reste juste à
    /// recharger le catalogue de couleurs en mémoire et réappliquer le
    /// thème actuellement actif (voir son handler).
    RemoteThemesFetched,
    /// Symétrique de `PortsCheckDone` pour `themes.json`.
    ThemesCheckDone { etag: String },
    /// Voir `start_save_backup` -- un port vient d'être traité par
    /// `core::save_backup::run_global_backup`, affiché dans le
    /// ProgressDialog comme un message d'install. Nom brut plutôt qu'un
    /// message déjà formaté -- ce `push` a lieu depuis le thread de fond de
    /// `run_global_backup` (voir start_save_backup), qui ne peut pas
    /// appeler `.global::<Tr>()` (accès Slint réservé au thread UI) ; la
    /// traduction se fait dans le handler de cet event, sur le thread UI.
    SaveBackupProgress { name: String },
    /// Export manuel terminé -- voir `start_save_backup`.
    SaveBackupDone { copied: usize, skipped: usize, failed: usize },
}

/// Libellés d'un `ListPickerDialog` extraits d'un champ texte commun à une
/// liste d'objets JSON -- `InstallAssetAmbiguous` ("name") et
/// `VersionsFetched` ("tag_name"), avec le même repli "?".
fn json_field_labels(items: &[Value], field: &str) -> Vec<String> {
    items.iter().map(|v| v.get(field).and_then(Value::as_str).unwrap_or("?").to_string()).collect()
}

/// Vide la file d'évènements d'arrière-plan et applique chacun sur le thread
/// UI (voir `AppEvent`) -- appelée par un `slint::Timer` dans `main()`,
/// qu'une install ou une vérif de MAJ soit en cours ou non : vider une file
/// vide ne coûte rien.
pub(crate) fn poll_app_events(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    app.refresh_geometry_if_scale_changed();
    checkpoint_playtime(app);
    refresh_live_playtime_display(app);
    let drained: Vec<AppEvent> = std::mem::take(&mut *lock(&app.events));
    // Ne garde que le DERNIER message de progression d'une rafale (un
    // download qui rapporte sa progression, ou un backup qui traite une
    // centaine de ports, peuvent empiler plusieurs de ces évènements entre
    // deux polls) -- les intermédiaires seraient de toute façon écrasés
    // avant peinture, mesurer leur texte et repositionner la fenêtre pour
    // chacun ne coûterait que pour rien.
    let last_install_progress = drained.iter().rposition(|e| matches!(e, AppEvent::InstallProgress { .. }));
    let last_save_backup_progress = drained.iter().rposition(|e| matches!(e, AppEvent::SaveBackupProgress { .. }));
    for (i, event) in drained.into_iter().enumerate() {
        match event {
            AppEvent::InstallProgress { .. } if Some(i) != last_install_progress => {}
            AppEvent::SaveBackupProgress { .. } if Some(i) != last_save_backup_progress => {}
            AppEvent::InstallProgress { message } => {
                if let DialogSlot::Progress(d) = &*app.dialog_nav.dialogs.borrow() {
                    resize_progress_dialog(app, d, &message);
                }
            }
            AppEvent::InstallDone { key, tag, pin_version } => {
                app.install_runtime.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                // Invalide la jaquette en cache : un (ré)install peut en
                // avoir livré une différente, et refresh_current_view juste
                // en dessous afficherait sinon l'ancienne. Le port lui-même
                // est aussi réutilisé plus bas pour un éventuel lancement
                // automatique (voir pending_launch_after_install) -- un seul
                // lookup dans le catalogue pour les deux besoins.
                let port = app.catalog.borrow().iter().find(|p| p.key() == key).cloned();
                if let Some(port) = &port {
                    app.grid_nav.card_image_cache.borrow_mut().remove(&port.folder);
                }
                {
                    let mut state = app.state.borrow_mut();
                    state.mark_installed(&key, tag);
                    if pin_version {
                        state.set_port_update(&key, false);
                    }
                }
                app.refresh_current_view();
                // Lancement automatique après un auto-install déclenché par
                // Play (voir launch_with_update_check/AppEvent::PlayUpdateChecked)
                // -- absent de la file pour un install/update "normal"
                // (bouton Install, Select version), qui ne doit jamais
                // lancer le jeu tout seul.
                if app.install_runtime.pending_launch_after_install.borrow_mut().remove(&key) {
                    if let Some(port) = port {
                        launch_flow(app, router, &port);
                    }
                }
            }
            AppEvent::InstallAssetAmbiguous { key, assets, release_override } => {
                app.install_runtime.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                if let Some(port) = app.catalog.borrow().iter().find(|p| p.key() == key).cloned() {
                    let labels = json_field_labels(&assets, "name");
                    open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_file(), labels, move |app, router, idx| {
                        if let Some(chosen) = assets.get(idx) {
                            start_install(app, router, port.clone(), Some(chosen.clone()), release_override.clone());
                        }
                    });
                }
            }
            AppEvent::VersionsFetched { key, releases } => {
                app.install_runtime.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                if let Some(port) = app.catalog.borrow().iter().find(|p| p.key() == key).cloned() {
                    let labels = json_field_labels(&releases, "tag_name");
                    open_picker_dialog(app, router, &tr!(app).invoke_dialog_title_choose_version(), labels, move |app, router, idx| {
                        if let Some(release) = releases.get(idx) {
                            start_install(app, router, port.clone(), None, Some(release.clone()));
                        }
                    });
                }
            }
            AppEvent::VersionsFetchError { key, message } => {
                app.install_runtime.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                open_message_dialog(app, router, &tr!(app).invoke_dialog_title_error(), &message);
            }
            AppEvent::InstallError { key, message } => {
                app.install_runtime.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                open_message_dialog(app, router, &tr!(app).invoke_dialog_title_installation_error(), &message);
            }
            // `Err` couvre aussi un lien mort/vide (voir download) : message
            // informatif, pas une erreur d'install -- le dossier du port n'a
            // pas été touché dans ce cas (voir installer::install_extra_only).
            AppEvent::ExtraInstallDone { key, result } => {
                app.install_runtime.installing.borrow_mut().remove(&key);
                close_current_dialog(app, router);
                let window = app.window();
                let tr = window.global::<Tr>();
                let message = match result {
                    Ok(()) => tr.invoke_message_extras_installed(),
                    Err(e) => tr.invoke_message_extras_failed(e.into()),
                };
                open_message_dialog(app, router, &tr.invoke_dialog_title_extras(), &message);
            }
            // `available == false` couvre AUSSI une erreur réseau/API (voir
            // launch_with_update_check) -- jamais bloquant, le Play doit
            // toujours aboutir sur la version déjà installée dans ce cas.
            AppEvent::PlayUpdateChecked { port, available } => {
                if available {
                    app.install_runtime.pending_launch_after_install.borrow_mut().insert(port.key().to_string());
                    start_install(app, router, *port, None, None);
                } else {
                    launch_flow(app, router, &port);
                }
            }
            // Change aussi le comportement du clic -- voir on_github_requested
            // et launch_self_update. Persisté (voir
            // StateManager::launcher_update_available) pour que le bouton
            // survive à un redémarrage sans nouveau check.
            AppEvent::SelfUpdateAvailable => {
                app.window().set_self_update_available(true);
                app.state.borrow_mut().set_launcher_update_available(true);
            }
            AppEvent::ImageCached { folder } => {
                app.grid_nav.card_image_cache.borrow_mut().remove(&folder);
                app.refresh_current_view();
            }
            AppEvent::RemoteCatalogFetched(ports) => {
                *app.catalog.borrow_mut() = ports;
                // La liste affichée doit refléter le nouveau catalogue tout
                // de suite, pas au prochain changement de recherche/mode.
                app.refresh_current_view();
            }
            AppEvent::PortsCheckDone { etag } => {
                app.state.borrow_mut().mark_catalog_check(etag);
            }
            AppEvent::ThemesCheckDone { etag } => {
                app.state.borrow_mut().mark_themes_check(etag);
            }
            AppEvent::RemoteThemesFetched => {
                // Résolu sur le thème ACTUELLEMENT actif -- une
                // synchronisation ne change jamais le choix de
                // l'utilisateur, seulement le catalogue de couleurs
                // disponibles (voir ui::theme::load).
                let active = app.state.borrow().active_theme.clone();
                crate::ui::theme::load(&app.paths.themes_path, &mut app.theme.theme_config.borrow_mut(), &active);
                apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
            }
            AppEvent::SaveBackupProgress { name } => {
                if let DialogSlot::Progress(d) = &*app.dialog_nav.dialogs.borrow() {
                    let message = d.global::<crate::Tr>().invoke_progress_backing_up_named(name.into());
                    resize_progress_dialog(app, d, &message);
                }
            }
            AppEvent::SaveBackupDone { copied, skipped, failed } => {
                close_current_dialog(app, router);
                // `failed` distinct de "skipped" -- une vraie erreur de
                // copie (disque plein, permission refusée) mérite d'être
                // signalée, pas confondue avec "rien à sauvegarder". La
                // condition (failed > 0) vit désormais côté .slint (voir
                // Tr.message-backup-summary), deux gabarits @tr() distincts
                // plutôt qu'un suffixe concaténé -- l'ordre des mots peut
                // différer entièrement d'une langue à l'autre.
                let window = app.window();
                let tr = window.global::<crate::Tr>();
                open_message_dialog(
                    app,
                    router,
                    &tr.invoke_dialog_title_saves_backup(),
                    &tr.invoke_message_backup_summary(copied as i32, skipped as i32, failed as i32),
                );
            }
            AppEvent::BringToForeground => {
                let window = app.window();
                // Même geste que la remontée post-jeu plus bas. L'évènement
                // vient du thread accept() de SINGLE_INSTANCE_PORT, sans
                // entrée utilisateur associée à notre thread : un
                // SetForegroundWindow simple serait silencieusement bloqué.
                window.window().set_minimized(false);
                if let Some(hwnd) = windows_chrome::native_hwnd(window.window()) {
                    windows_chrome::force_foreground_window(hwnd);
                }
            }
        }
    }

    // Appelé INCONDITIONNELLEMENT à chaque tick (jamais court-circuité par
    // le `&&` ci-dessous) -- son nettoyage des process terminés/l'appel à
    // record_playtime doivent tourner même en mode fenêtré ou hors
    // minimized_for_game, sinon un temps de jeu ne se persiste qu'au
    // prochain clic sur CE port précis (is_port_running), potentiellement
    // bien après avoir fermé le jeu.
    let still_running = any_process_running(app);
    // Remonte la fenêtre minimisée par launch_executable dès que TOUS les
    // jeux lancés ont quitté. `get_big_mode()` revérifié ici : si
    // l'utilisateur est repassé en fenêtré entre-temps, la fenêtre n'est
    // plus minimisée et il n'y a rien à forcer.
    if app.install_runtime.minimized_for_game.get() && app.window().get_big_mode() && !still_running {
        app.install_runtime.minimized_for_game.set(false);
        let window = app.window();
        window.window().set_minimized(false);
        // `set_minimized(false)` dé-minimise sans garantir de repasser
        // AU-DESSUS des autres fenêtres : le jeu qui vient de quitter a pu
        // laisser une autre fenêtre au premier plan. Un SetForegroundWindow
        // simple ne suffit pas non plus depuis ce Timer d'arrière-plan --
        // voir force_foreground_window.
        if let Some(hwnd) = windows_chrome::native_hwnd(window.window()) {
            windows_chrome::force_foreground_window(hwnd);
        }
    }

    reclaim_dialog_foreground_if_needed(app);
}

/// Corrige un Alt+Tab qui ramène la fenêtre PRINCIPALE devant un dialogue
/// encore ouvert. `own_window` (voir finish_dialog_open) traite le
/// dialogue, pas la fenêtre principale, qui reste un top-level Alt-tabable.
/// Compare spécifiquement à `main_hwnd` et non
/// `foreground_window_belongs_to_us`, trop large : Alt-Tab vers une AUTRE
/// appli pendant un install doit rester possible sans vol de focus.
fn reclaim_dialog_foreground_if_needed(app: &Rc<AppState>) {
    let dialog_hwnd = super::dialogs::dialog_window(&app.dialog_nav.dialogs.borrow()).and_then(windows_chrome::native_hwnd);
    let (Some(dialog_hwnd), Some(main_hwnd)) = (dialog_hwnd, windows_chrome::native_hwnd(app.window().window())) else { return };
    if windows_chrome::is_foreground_window(main_hwnd) {
        windows_chrome::force_foreground_window(dialog_hwnd);
    }
}
