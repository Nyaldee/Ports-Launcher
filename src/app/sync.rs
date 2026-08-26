//! Rafraîchissement en tâche de fond : mise à jour du launcher lui-même,
//! catalogue de ports, catalogue de thèmes.

use super::dialogs::{open_message_dialog, tr};
use super::events::{lock, AppEvent};
use super::state::AppState;
use crate::ui::gamepad_router::GamepadRouter;
use crate::Tr;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

/// Lance ports_launcher_updater.bat (à côté de l'exe, voir base_dir) puis
/// ferme l'appli -- le .bat tue le process par sécurité (taskkill) au cas où,
/// mais une fermeture propre ici ne dépend pas uniquement de ce kill forcé.
/// Efface `launcher_update_available` (et redémarre son délai) juste avant de
/// relancer -- le tout nouveau build qui redémarre ensuite n'a plus de
/// raison de croire qu'une MAJ l'attend encore, voir le commentaire du champ.
pub(crate) fn launch_self_update(app: &Rc<AppState>, router: &Rc<RefCell<GamepadRouter>>) {
    let updater = crate::base_dir().join("ports_launcher_updater.bat");
    match crate::core::launch::launch(&updater) {
        Ok(_) => {
            let mut state = app.state.borrow_mut();
            state.mark_launcher_update_check();
            state.set_launcher_update_available(false);
            let _ = slint::quit_event_loop();
        }
        Err(e) => open_message_dialog(app, router, &tr!(app).invoke_dialog_title_update_error(), &e.to_string()),
    }
}

/// Vérifie si une nouvelle release du launcher LUI-MÊME est disponible. Le
/// launcher n'a pas d'`InstalledInfo` (aucune étape "install") --
/// `core::version::APP_VERSION`, dérivée de la date de compilation par
/// build.rs, sert de référence. `NEUTRAL_INSTALLED_AT` neutralise le repli
/// par date d'`update_decision` : seule la comparaison de tag compte ici,
/// chaque publication du launcher ayant un tag garanti différent.
///
/// Gardé par `should_check_launcher_update()`, qui suspend tout appel tant
/// que `launcher_update_available` est déjà vrai (voir son commentaire dans
/// state.rs) -- inutile de reconfirmer une MAJ déjà connue.
/// `mark_launcher_update_check()` est appelé ICI directement, synchrone,
/// AVANT même de lancer la requête -- même convention que
/// `mark_catalog_check` ("après CHAQUE tentative de vérification") : sans
/// ça, une erreur réseau laisserait `last_launcher_update_check` vide et
/// redéclencherait ce check à CHAQUE lancement au lieu d'une fois toutes les
/// `LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS`.
pub(crate) fn start_self_update_check(app: &Rc<AppState>) {
    let github_token = {
        let mut state = app.state.borrow_mut();
        if !state.should_check_launcher_update() {
            return;
        }
        state.mark_launcher_update_check();
        state.github_token.clone()
    };
    let events = app.events.clone();
    std::thread::spawn(move || {
        let result = crate::core::github_api::check_update_available(
            crate::SELF_REPO,
            Some(crate::core::version::APP_VERSION),
            crate::core::version::NEUTRAL_INSTALLED_AT,
            github_token.as_deref(),
        );
        match result {
            Ok((true, _, _)) => lock(&events).push(AppEvent::SelfUpdateAvailable),
            Ok((false, _, _)) => {}
            Err(e) => eprintln!("[self update check] {}", e.message()),
        }
    });
}

/// Rafraîchit `ports.json` depuis GitHub en tâche de fond -- voir
/// `core::catalog_sync` pour le mécanisme (ETag, throttle dédié
/// `should_check_catalog`/`mark_catalog_check`, ENTIÈREMENT séparé de
/// `should_check_launcher_update` -- pas le même quota, voir
/// CATALOG_CHECK_INTERVAL_HOURS). Démarrée APRÈS
/// `window.show()`, pour ne jamais bloquer le démarrage sur le réseau.
pub(crate) fn start_catalog_sync(app: &Rc<AppState>) {
    let known_etag = {
        let state = app.state.borrow();
        if !state.should_check_catalog() {
            return;
        }
        state.last_catalog_etag.clone()
    };
    let ports_json_path = app.paths.config_dir.join("ports.json");
    let ports_local_json_path = app.paths.config_dir.join("ports.local.json");
    let events = app.events.clone();
    std::thread::spawn(move || match crate::core::catalog_sync::fetch_ports_if_changed(&known_etag) {
        Ok(crate::core::catalog_sync::CatalogUpdate::NotModified) => {
            lock(&events).push(AppEvent::PortsCheckDone { etag: known_etag });
        }
        Ok(crate::core::catalog_sync::CatalogUpdate::Updated { text, etag }) => {
            // Déjà validé par fetch_if_changed -- unwrap_or_default plutôt
            // qu'un panic dans un thread de rafraîchissement.
            let remote_ports = crate::core::config::parse_catalog(&text).unwrap_or_default();
            // Refusionne ports.local.json : sans ça, les ports ajoutés à la
            // main, absents du catalogue distant, disparaîtraient de la vue
            // jusqu'au prochain démarrage. Même fonction qu'au lancement,
            // pas une seconde logique de fusion à maintenir.
            let ports = crate::core::config::merge_local_catalog(remote_ports, crate::core::config::load_local_config(&ports_local_json_path));
            // Écrit aussi sur disque -- le prochain lancement démarre
            // directement sur cette version, sans attendre un nouveau fetch.
            let _ = std::fs::write(&ports_json_path, &text);
            let mut events = lock(&events);
            events.push(AppEvent::PortsCheckDone { etag });
            events.push(AppEvent::RemoteCatalogFetched(ports));
        }
        Err(e) => eprintln!("[catalog sync] {e}"),
    });
}

/// Rafraîchit `themes.json` depuis GitHub en tâche de fond -- même
/// mécanisme que `start_catalog_sync` (ETag, écriture directe sur disque),
/// throttle dédié ENTIÈREMENT séparé (`should_check_themes`/
/// `mark_themes_check`, voir THEMES_CHECK_INTERVAL_HOURS dans state.rs) --
/// rien n'oblige les deux fichiers à se rafraîchir au même rythme. Démarrée
/// APRÈS window.show(), pour ne jamais bloquer le démarrage sur le réseau --
/// le catalogue de couleurs déjà chargé (voir ui::theme::load dans main())
/// reste affiché tel quel jusqu'à ce que ce fetch aboutisse.
pub(crate) fn start_themes_sync(app: &Rc<AppState>) {
    let known_etag = {
        let state = app.state.borrow();
        if !state.should_check_themes() {
            return;
        }
        state.last_themes_etag.clone()
    };
    let themes_path = app.paths.themes_path.clone();
    let events = app.events.clone();
    std::thread::spawn(move || match crate::core::catalog_sync::fetch_themes_if_changed(&known_etag) {
        Ok(crate::core::catalog_sync::CatalogUpdate::NotModified) => {
            lock(&events).push(AppEvent::ThemesCheckDone { etag: known_etag });
        }
        Ok(crate::core::catalog_sync::CatalogUpdate::Updated { text, etag }) => {
            let _ = std::fs::write(&themes_path, &text);
            let mut events = lock(&events);
            events.push(AppEvent::ThemesCheckDone { etag });
            events.push(AppEvent::RemoteThemesFetched);
        }
        Err(e) => eprintln!("[themes sync] {e}"),
    });
}
