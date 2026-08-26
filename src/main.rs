// Sous-système GUI (pas console) -- sans ça, Windows ouvre une invite de
// commandes derrière l'appli à chaque lancement (comportement par défaut
// d'un `fn main()`, pensé pour des outils en ligne de commande).
#![windows_subsystem = "windows"]

// Orchestration/état/UI -- voir app/mod.rs pour le découpage par
// responsabilité (dialogues, install/lancement, temps de jeu, évènements
// d'arrière-plan, cibles manette, état partagé).
mod app;
mod core;
// Harnais de test visuel (--visual-stress-test) -- utile en dev, jamais dans
// l'exe distribué aux utilisateurs (voir son commentaire de module pour le
// détail du sandbox isolé).
#[cfg(debug_assertions)]
mod stress_test;
mod ui;

use app::dialogs::{apply_theme, dialog_is_open, open_info_dialog, open_settings_dialog, open_uninstall_confirm_dialog, DialogSlot};
use app::events::{lock, poll_app_events, AppEvent};
use app::gamepad_target::AppGamepadTarget;
use app::install_launch::{
    activate_selection, install_row, launch_flow, open_update_toggle_dialog_row, reveal_selected_folder, with_indexed_port,
};
use app::state::{AppPaths, AppState, DialogNav, GridNav, InstallRuntime, ThemeState, WindowGeometry, WindowedNav};
use app::sync::{launch_self_update, start_catalog_sync, start_self_update_check, start_themes_sync};
use core::models::{Port, SourceType};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use ui::font_sizing::{apply_mode_geometry, compute_mode_geometry};
use ui::gamepad_router::GamepadRouter;
use ui::windows_chrome;

slint::include_modules!();

/// Port de loopback arbitraire -- sert à la fois de VERROU (le premier
/// `bind` réussi fait foi, voir `acquire_single_instance_lock`) et de CANAL
/// DE SIGNAL (une seconde instance, qui échoue à bindre, s'y connecte pour
/// demander à la première de se remettre au premier plan -- voir le thread
/// `accept()` dans `main()` et `AppEvent::BringToForeground`). Un
/// `TcpListener` loopback plutôt qu'un mutex nommé Win32 : cross-platform
/// sans code conditionnel, et un mutex nommé n'offrirait que le verrou.
const SINGLE_INSTANCE_PORT: u16 = 57391;

/// Gauche/Droite en mode fenêtré (liste à une seule colonne) sautent d'une
/// "page" de ce nombre de lignes plutôt que de ne rien faire (voir
/// move_requested).
const PAGE_ROWS: i32 = 10;

/// Page ouverte par le bouton "GitHub" quand il affiche encore ce texte --
/// une fois passé en "Update" (voir AppEvent::SelfUpdateAvailable), le clic
/// lance ports_launcher_updater.bat à la place (voir launch_self_update).
const GITHUB_URL: &str = "https://github.com/Nyaldee/Ports-Launcher";
/// Dépôt de référence pour la vérification de mise à jour du launcher
/// lui-même (voir start_self_update_check).
const SELF_REPO: &str = "Nyaldee/Ports-Launcher";
const DISCORD_URL: &str = "https://discord.com/invite/5GYmst9twA";

fn acquire_single_instance_lock() -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)).ok()
}

/// Dossier de l'exécutable en cours -- `ports.json`/`themes.json` vivent
/// toujours à côté de lui, jamais empaquetés dans le binaire, pour que
/// l'utilisateur puisse les éditer/mettre à jour sans recompiler.
fn base_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

/// Badge "Update" -- vrai UNIQUEMENT quand l'auto-MAJ est désactivée pour ce
/// port (voir InfoDialog "Update") : purement l'état du bouton, aucune
/// requête réseau derrière ce badge. Un port en auto-MAJ ne montre jamais ce
/// badge, la MAJ s'installe silencieusement au prochain Play à la place
/// (voir launch_with_update_check).
fn to_port_items(ports: &[&Port], library_dir: &Path, state: &core::state::StateManager) -> Vec<PortItem> {
    ports
        .iter()
        .map(|p| PortItem {
            name: p.name.clone().into(),
            auto_update_off: state.get(p.key()).map(|i| !i.update).unwrap_or(false),
            installed: core::installer::is_installed(p, library_dir),
            is_local: p.source_type == SourceType::Local,
        })
        .collect()
}
fn main() {
    // Partagé avec le thread accept() ci-dessous ET avec AppState (voir son
    // champ `events`) -- créé tôt pour que les deux puissent le cloner sans
    // dépendance d'ordre.
    let events: Arc<Mutex<Vec<AppEvent>>> = Arc::new(Mutex::new(Vec::new()));

    // Une autre instance tourne déjà : plutôt qu'une sortie silencieuse
    // (double-clic accidentel, raccourci relancé...), on lui demande de se
    // remettre au premier plan. N'importe quel octet fait office de signal,
    // la première instance n'inspecte jamais ce qu'elle reçoit.
    let Some(single_instance_listener) = acquire_single_instance_lock() else {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
            let _ = stream.write_all(&[0u8]);
        }
        return;
    };
    // Le thread prend possession du listener pour toute la durée du
    // programme : le port reste bindé (verrou) et les connexions entrantes
    // sont réellement écoutées (canal de signal).
    {
        let events = events.clone();
        std::thread::spawn(move || {
            for stream in single_instance_listener.incoming().flatten() {
                drop(stream);
                lock(&events).push(AppEvent::BringToForeground);
            }
        });
    }

    windows_chrome::enable_dpi_awareness();

    // Le module stress_test n'existe qu'en build debug -- en release,
    // `stress_test_iterations` reste structurellement None.
    #[cfg(debug_assertions)]
    let stress_test_iterations = stress_test::parse_stress_test_iterations();
    #[cfg(not(debug_assertions))]
    let stress_test_iterations: Option<u32> = None;

    #[cfg(debug_assertions)]
    let bdir = if stress_test_iterations.is_some() { stress_test::build_stress_sandbox() } else { base_dir() };
    #[cfg(not(debug_assertions))]
    let bdir = base_dir();

    // Doit rester en vie jusqu'à la toute fin de main() -- voir son Drop.
    #[cfg(debug_assertions)]
    let _sandbox_cleanup = stress_test_iterations.is_some().then(|| stress_test::CleanupSandboxOnDrop(bdir.clone()));

    // Pas encore de ports.json local (premier lancement, ou utilisateur qui
    // l'a supprimé) -- catalogue vide en attendant que start_catalog_sync le
    // télécharge en tâche de fond, même principe que themes.json (voir
    // ui::theme::load, qui retombe sur des couleurs par défaut sans jamais
    // bloquer le démarrage sur le réseau). Un ports.json qui EXISTE mais ne
    // charge pas reste fatal (voir core::config::load_config) : un fichier
    // potentiellement édité à la main qui échoue mérite un signalement
    // clair, jamais un catalogue vide silencieux.
    let ports_json_path = bdir.join("ports.json");
    let catalog = if ports_json_path.exists() {
        match core::config::load_config(&ports_json_path) {
            Ok(v) => v,
            Err(e) => {
                windows_chrome::show_startup_error(&format!("Couldn't load ports.json: {e}"));
                return;
            }
        }
    } else {
        Vec::new()
    };
    // Catalogue local de l'utilisateur -- ses propres ajouts (sans
    // "source", voir SourceType::Local), dans un fichier séparé pour
    // n'être jamais écrasés par une mise à jour de ports.json. Absent, ce
    // n'est pas fatal (voir load_local_config).
    let catalog = core::config::merge_local_catalog(catalog, core::config::load_local_config(&bdir.join("ports.local.json")));

    // Vrai UNIQUEMENT si state.json n'existe pas encore -- vérifié AVANT
    // StateManager::load (qui en écrit un par défaut sinon), pour distinguer
    // un tout premier lancement d'un fichier déjà existant. Sert uniquement
    // à décider si `placeholder_text` doit recevoir un seed traduit selon la
    // langue système une fois la fenêtre créée (voir plus bas) -- jamais
    // retouché ensuite par un changement de langue.
    let is_first_run = !bdir.join("state.json").exists();
    let state = RefCell::new(core::state::StateManager::load(&bdir.join("state.json"), &bdir.join("themes.json")));

    // Pose la date de référence du throttle self-update SANS vérifier tout
    // de suite (voir `should_check_launcher_update`, qui renvoie faux tant
    // que `last_launcher_update_check` est vide plutôt que de sauter le
    // throttle) -- couvre aussi bien un tout premier lancement qu'un
    // `state.json` d'avant ce champ (mise à jour depuis une version plus
    // ancienne du launcher) : dans les deux cas, `last_launcher_update_check`
    // vaut vide ici, et le tout premier VRAI check aura lieu dans
    // LAUNCHER_UPDATE_CHECK_INTERVAL_HOURS, jamais immédiatement.
    if state.borrow().last_launcher_update_check.is_empty() {
        state.borrow_mut().mark_launcher_update_check();
    }

    let library_dir = bdir.join("Library");
    let cache_dir = bdir.join("cache");
    let _ = std::fs::create_dir_all(&library_dir);
    let _ = std::fs::create_dir_all(&cache_dir);

    // Adopte les ports déposés à la main dans Library/ sans passer par
    // Install : `is_installed` (vérité disque) les traite déjà comme
    // installés, mais sans entrée dans state.json Info les afficherait "Not
    // installed". `mark_installed(key, None)` les enregistre avec
    // `installed_at` = maintenant et un tag inconnu ; `update_decision`
    // (voir github_api.rs) repère ensuite une vraie mise à jour publiée
    // après cette adoption via `installed_at`.
    {
        let mut s = state.borrow_mut();
        for port in &catalog {
            if s.get(port.key()).is_none() && core::installer::is_installed(port, &library_dir) {
                s.mark_installed(port.key(), None);
            }
        }
    }

    let mut theme_cfg = ui::theme::ThemeConfig::default();
    ui::theme::load(&bdir.join("themes.json"), &mut theme_cfg, &state.borrow().active_theme);

    let window = match AppWindow::new() {
        Ok(w) => w,
        Err(e) => {
            windows_chrome::show_startup_error(&format!("Failed to create the window: {e}"));
            return;
        }
    };

    // Seed UNIQUE, traduit selon la langue système -- voir le commentaire
    // d'`is_first_run` et celui du champ `placeholder_text` dans state.rs.
    // Rust ne peut appeler @tr() lui-même, d'où ce passage par `Tr`
    // (existe forcément à ce stade, `window` vient d'être créée).
    if is_first_run {
        let seeded = window.global::<Tr>().invoke_placeholder_default_search().to_string();
        state.borrow_mut().set_placeholder_text(seeded);
    }

    apply_theme(&window, &theme_cfg, state.borrow().border_width);
    window.set_placeholder_text(state.borrow().placeholder_text.clone().into());
    window.set_show_clock(state.borrow().show_clock);

    // Horloge de la barre de recherche -- rafraîchie chaque seconde, jamais
    // créée si "show_clock" est désactivé dans state.json. Le Timer doit
    // rester en vie jusqu'à la fin de main() : le laisser tomber hors de
    // portée l'arrêterait silencieusement.
    let _clock_timer = if state.borrow().show_clock {
        window.set_clock_text(core::clock::format_now().into());
        let timer = slint::Timer::default();
        let weak = window.as_weak();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(1000), move || {
            if let Some(w) = weak.upgrade() {
                w.set_clock_text(core::clock::format_now().into());
            }
        });
        Some(timer)
    } else {
        None
    };

    // Zone de travail du moniteur SOUS LE CURSEUR (hors barre des tâches),
    // pas la résolution brute ni forcément l'écran principal -- voir
    // windows_chrome::work_area_under_cursor, valable fenêtré ET
    // plein écran (qui n'est d'ailleurs pas exclusif : la barre des tâches
    // reste visible/utilisable).
    let (area_x, area_y, screen_w, screen_h) = windows_chrome::work_area_under_cursor();

    // Lu avant le premier show() -- voir plus bas pourquoi
    // GetDpiForMonitor plutôt que GetDpiForWindow à ce stade. Sert à la fois
    // à grid_columns et aux géométries figées des deux modes.
    let pre_show_scale = windows_chrome::scale_factor_under_cursor();

    // CARD_WIDTH/CARD_SPACING sont des pixels LOGIQUES (Slint les remet
    // lui-même à l'échelle au rendu), donc comparés à une largeur d'écran
    // elle aussi ramenée en logique : les comparer au `screen_w` physique
    // sous-compterait l'espace réellement occupé par chaque carte et
    // entasserait plus de colonnes qu'il n'en tient.
    //
    // Largeur RÉELLE à 100%, sans marge de sécurité : le bloc de cartes est
    // centré côté .slint (voir row-block-width/row-left-margin dans
    // card-grid.slint), et ce centrage n'est correct que si le nombre de
    // colonnes budgété ici correspond exactement à ce qui est affiché.
    let grid_available_width = screen_w as f32 / pre_show_scale;
    let grid_columns = core::grid::compute_grid_columns(grid_available_width);

    window.set_card_width(core::grid::CARD_WIDTH);
    window.set_card_height(core::grid::CARD_HEIGHT);
    window.set_card_spacing(core::grid::CARD_SPACING);
    window.set_grid_columns(grid_columns as i32);

    let font_family = state.borrow().font_family.clone().unwrap_or_else(|| "Segoe UI".to_string());
    window.set_font_family(font_family.clone().into());

    let big_mode = state.borrow().fullscreen;
    window.set_big_mode(big_mode);

    // Géométries FIGÉES des deux modes, calculées une seule fois pour toute
    // la session et AVANT le premier show(), avec `pre_show_scale`
    // (`GetDpiForMonitor` sur le moniteur sous le curseur) -- pas
    // `GetDpiForWindow` sur le HWND réel : interrogé juste après `show()`,
    // il donne parfois une lecture encore instable (Windows n'a pas
    // forcément fini d'associer la fenêtre à son moniteur), et les deux
    // géométries hériteraient alors définitivement d'un facteur d'échelle
    // faux. `GetDpiForMonitor` ne dépend d'aucun HWND.
    let area = (area_x, area_y, screen_w, screen_h);
    let (window_width_fraction, border_width) = { let s = state.borrow(); (s.window_width_fraction, s.border_width) };
    let normal_mode = compute_mode_geometry(&font_family, area, pre_show_scale, false, window_width_fraction, border_width);
    let fullscreen_mode = compute_mode_geometry(&font_family, area, pre_show_scale, true, window_width_fraction, border_width);
    apply_mode_geometry(&window, if big_mode { &fullscreen_mode } else { &normal_mode });

    // Montre la fenêtre MAINTENANT plutôt que via window.run() en fin de
    // main(), uniquement pour pouvoir lire ensuite
    // `slint::Window::scale_factor()` de façon fiable : il reste bloqué à
    // 1.0 tant que la fenêtre n'est pas associée à un moniteur, ce qui
    // n'arrive qu'après le retour de main() avec un run() classique.
    // Langue forcée persistée (voir StateManager::language) -- vide = suit
    // la détection automatique de la locale système, déjà faite par
    // défaut à la création du premier composant, rien à appeler dans ce cas.
    let saved_language = state.borrow().language.clone();
    if !saved_language.is_empty() {
        let _ = slint::select_bundled_translation(&saved_language);
    }

    // Bouton "Update" du footer restauré immédiatement depuis
    // `launcher_update_available` (voir son commentaire dans state.rs) --
    // sans ça, le bouton repartirait toujours de "GitHub" au démarrage
    // jusqu'au prochain vrai check, potentiellement des heures plus tard.
    if state.borrow().launcher_update_available {
        window.set_self_update_available(true);
    }

    // `ComponentHandle::run()` n'est qu'un raccourci pour show() +
    // slint::run_event_loop() + hide().
    window.show().expect("échec de l'affichage de la fenêtre");
    // Icône (voir apply_window_icon) et éligibilité Alt+Tab, DIFFÉRÉES :
    // `windows_chrome::native_hwnd()` renvoie None juste après show(), la fenêtre native
    // n'étant pas encore complètement associée. `single_shot` laisse la
    // boucle d'évènements tourner une fois avant de réessayer.
    {
        let weak = window.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            let Some(window) = weak.upgrade() else { return };
            if let Some(hwnd) = windows_chrome::native_hwnd(window.window()) {
                windows_chrome::apply_window_icon(hwnd);
                windows_chrome::force_alt_tab_visible(hwnd);
            }
        });
    }

    // Facteur d'échelle DPI pour le CONTENU de la fenêtre
    // (Theme.scale-factor). MÊME SOURCE que pre_show_scale, jamais
    // `GetDpiForWindow` : sur un écran à 200%, un désaccord entre les deux
    // lectures ferait rendre tout le contenu deux fois trop grand alors que
    // la taille de fenêtre resterait correcte.
    //
    // Valable uniquement à cet instant. Une fois la fenêtre affichée,
    // `recompute_normal_mode`/`toggle_fullscreen` relisent
    // `window.scale_factor()`, source stable que Slint maintient à jour, ce
    // qui fait suivre un changement d'échelle en cours de session dès le
    // prochain redimensionnement plutôt qu'au redémarrage.
    let scale = windows_chrome::scale_factor_under_cursor();
    window.global::<Theme>().set_scale_factor(scale);

    let app = Rc::new(AppState {
        window: window.as_weak(),
        state,
        catalog: RefCell::new(catalog),
        paths: AppPaths {
            library_dir: library_dir.clone(),
            cache_dir: cache_dir.clone(),
            config_dir: bdir.clone(),
            saves_backup_dir: bdir.join("Saves Backup"),
            themes_path: bdir.join("themes.json"),
        },
        theme: ThemeState {
            semantic: theme_cfg.semantic,
            font_family: font_family.clone(),
            // Move de theme_cfg -- doit rester APRÈS toutes les lectures de
            // ses champs ci-dessus : une struct déplacée ne peut plus voir
            // ses champs lus individuellement, même Copy.
            theme_config: RefCell::new(theme_cfg),
        },
        window_geometry: WindowGeometry {
            border_width: Cell::new(border_width),
            window_width_fraction: Cell::new(window_width_fraction),
            scale: Cell::new(scale),
            normal_mode: RefCell::new(normal_mode),
            fullscreen_mode: RefCell::new(fullscreen_mode),
        },
        grid_nav: GridNav {
            grid_columns: Cell::new(grid_columns),
            displayed_installed: RefCell::new(Vec::new()),
            grid_selected: Cell::new((0, 0)),
            grid_mouse_active: Cell::new(true),
            last_card_click: Cell::new(None),
            double_click_ms: windows_chrome::double_click_time_ms(),
            card_image_cache: RefCell::new(HashMap::new()),
        },
        windowed_nav: WindowedNav {
            displayed_windowed: RefCell::new(Vec::new()),
            windowed_selected: Cell::new(0),
            search_query: RefCell::new(String::new()),
        },
        install_runtime: InstallRuntime {
            installing: RefCell::new(HashSet::new()),
            running_processes: RefCell::new(HashMap::new()),
            launch_started_at: RefCell::new(HashMap::new()),
            pending_launch_after_install: RefCell::new(HashSet::new()),
            minimized_for_game: Cell::new(false),
        },
        dialog_nav: DialogNav {
            dialogs: RefCell::new(DialogSlot::None),
            picker_index: Cell::new(0),
            info_nav_index: Cell::new(0),
            confirm_nav_index: Cell::new(0),
            error_nav_index: Cell::new(0),
            info_dialog_port_key: RefCell::new(None),
        },
        events: events.clone(),
        stress_test: stress_test_iterations.is_some(),
    });

    // Créé avant le câblage des callbacks ci-dessous : plusieurs d'entre eux
    // (install/lancement/dialogues) en ont besoin pour pousser/dépiler leurs
    // propres cibles manette.
    let router = Rc::new(RefCell::new(GamepadRouter::new()));

    app.rebuild_windowed("");
    if big_mode {
        app.enter_fullscreen();
    }

    {
        let app = app.clone();
        window.on_search_changed(move |query| {
            *app.windowed_nav.search_query.borrow_mut() = query.to_string();
            // Filtre la vue affichée, liste OU grille selon le mode courant
            // (même branchement que refresh_current_view) -- un
            // rebuild_windowed inconditionnel laisserait la recherche sans
            // effet en plein écran.
            if app.window().get_big_mode() {
                app.rebuild_grid(true);
            } else {
                app.rebuild_windowed(&query);
            }
        });
    }

    {
        let app = app.clone();
        window.on_fullscreen_toggle_requested(move || app.toggle_fullscreen());
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_settings_requested(move || open_settings_dialog(&app, &router));
    }

    {
        let app = app.clone();
        window.on_window_size_requested(move |percent| app.set_window_size_percent(percent));
    }
    {
        let app = app.clone();
        window.on_border_adjust_requested(move |delta| app.adjust_border(delta));
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_card_activated(move |row, col| {
            let previous = app.grid_nav.grid_mouse_active.get().then(|| app.grid_nav.grid_selected.get());
            app.grid_nav.grid_selected.set((row as usize, col as usize));
            app.refresh_grid_selection(previous);
            // Un clic isolé ne fait que sélectionner, comme
            // list-row-activated en mode fenêtré (Jouer est un bouton
            // séparé). Lancer exige un vrai double-clic sur la MÊME carte
            // dans le délai Windows configuré (voir double_click_ms), pour
            // qu'un clic accidentel ne démarre pas un jeu.
            let key = (row as usize, col as usize);
            let now = Instant::now();
            if let Some((last_key, last_time)) = app.grid_nav.last_card_click.get() {
                let elapsed_ms = now.duration_since(last_time).as_millis();
                // < 50ms : doublon du MÊME clic physique, card-grid.slint
                // pouvant émettre card-activated deux fois pour un seul clic
                // (`clicked` et son repli bas-niveau). Ignoré entièrement --
                // le compter comme second clic suffirait à déclencher un
                // lancement sur un simple clic.
                if elapsed_ms < 50 {
                    return;
                }
                if last_key == key && elapsed_ms <= app.grid_nav.double_click_ms as u128 {
                    // Consommé : un 3e clic rapide doit repartir d'un
                    // double-clic complet, pas redéclencher seul.
                    app.grid_nav.last_card_click.set(None);
                    activate_selection(&app, &router);
                    return;
                }
            }
            app.grid_nav.last_card_click.set(Some((key, now)));
        });
    }

    // Survol souris -- déplace la sélection sous le curseur, comme la
    // navigation clavier/manette (voir PortWidget.hovered/PortCard.hovered).
    {
        let app = app.clone();
        window.on_list_row_hovered(move |index| {
            app.windowed_nav.windowed_selected.set(index as usize);
            app.push_selected_index();
        });
    }
    {
        let app = app.clone();
        window.on_card_hovered(move |row, col| {
            let previous = app.grid_nav.grid_mouse_active.get().then(|| app.grid_nav.grid_selected.get());
            app.grid_nav.grid_selected.set((row as usize, col as usize));
            app.grid_nav.grid_mouse_active.set(true);
            app.refresh_grid_selection(previous);
        });
    }
    // La souris quitte la grille -- efface la surbrillance sans toucher à
    // grid_selected (voir grid_mouse_active) : une carte ne reste
    // surlignée que tant que la souris est réellement dessus.
    {
        let app = app.clone();
        window.on_card_unhovered(move || {
            let previous = app.grid_nav.grid_mouse_active.get().then(|| app.grid_nav.grid_selected.get());
            app.grid_nav.grid_mouse_active.set(false);
            app.refresh_grid_selection(previous);
        });
    }

    {
        let app = app.clone();
        window.on_move_requested(move |dx, dy| {
            if app.window().get_big_mode() {
                app.move_grid_selection(dx, dy);
            } else if dx != 0 {
                // Gauche/Droite : saut de page en liste à une seule colonne
                // (voir PAGE_ROWS), pas un déplacement horizontal qui
                // n'aurait pas de sens ici.
                app.move_windowed_selection(dx * PAGE_ROWS);
            } else {
                app.move_windowed_selection(dy);
            }
        });
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_activate_requested(move || activate_selection(&app, &router));
    }

    {
        let app = app.clone();
        window.on_reveal_folder_requested(move || reveal_selected_folder(&app));
    }

    {
        let app = app.clone();
        let router = router.clone();
        window.on_github_requested(move || {
            if app.window().get_self_update_available() {
                launch_self_update(&app, &router);
            } else {
                core::launch::open_url(GITHUB_URL);
            }
        });
    }
    window.on_discord_requested(|| core::launch::open_url(DISCORD_URL));

    // Clic sur le CORPS d'une ligne -- sélectionne seulement (voir
    // list-row-activated dans app-window.slint) : lancer ou installer passe
    // par les boutons d'action de la ligne.
    {
        let app = app.clone();
        window.on_list_row_activated(move |index| {
            app.windowed_nav.windowed_selected.set(index as usize);
            app.push_selected_index();
        });
    }

    // Boutons d'action colorés -- agissent sur LEUR ligne (par index dans la
    // liste affichée), jamais sur "la sélection courante". Gardés par
    // dialog_is_open comme activate_port/delete_port : un clic sur une autre
    // ligne pendant qu'un dialogue est ouvert ne doit rien déclencher (voir
    // dialog_is_open).
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_play_requested(move |index| {
            if dialog_is_open(&app) {
                return;
            }
            with_indexed_port(&app, index, |port| launch_flow(&app, &router, &port));
        });
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_install_requested(move |index| install_row(&app, &router, index));
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_update_requested(move |index| open_update_toggle_dialog_row(&app, &router, index));
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_uninstall_requested(move |index| {
            if dialog_is_open(&app) {
                return;
            }
            with_indexed_port(&app, index, |port| open_uninstall_confirm_dialog(&app, &router, port));
        });
    }
    {
        let app = app.clone();
        // Port LOCAL uniquement (voir PortItem.is-local dans
        // app-window.slint) -- jamais de suppression, seulement l'ouverture
        // de son dossier : ses fichiers appartiennent à l'utilisateur.
        window.on_list_row_open_folder_requested(move |index| {
            with_indexed_port(&app, index, |port| {
                if let Ok(dir) = core::path_safety::safe_join(&app.paths.library_dir, &port.folder) {
                    core::launch::open_path(&dir);
                }
            });
        });
    }
    {
        let app = app.clone();
        let router = router.clone();
        window.on_list_row_info_requested(move |index| {
            if dialog_is_open(&app) {
                return;
            }
            with_indexed_port(&app, index, |port| open_info_dialog(&app, &router, &port));
        });
    }

    window.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });

    // Déplacement de la fenêtre à la souris (no-frame, Windows ne le fait
    // pas tout seul -- voir drag-area dans app-window.slint). Recalculer la
    // position à la main sur chaque évènement "moved" de Slint donne un
    // déplacement nettement plus lent que le curseur, chaque évènement
    // traversant la boucle Slint et winit -- voir windows_chrome::begin_window_drag,
    // qui délègue le glissé ENTIER à Windows.
    {
        let app = app.clone();
        window.on_window_drag_requested(move || {
            let Some(hwnd) = windows_chrome::native_hwnd(app.window().window()) else { return };
            windows_chrome::begin_window_drag(hwnd);
        });
    }

    // File d'évènements d'arrière-plan (voir AppEvent) -- toujours actif,
    // contrairement au Timer manette : une install peut être déclenchée sans
    // aucune manette branchée.
    let _event_timer = {
        let app = app.clone();
        let router = router.clone();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
            poll_app_events(&app, &router);
        });
        timer
    };

    start_self_update_check(&app);
    start_catalog_sync(&app);
    start_themes_sync(&app);

    // Manette -- un seul routeur pour toute l'appli (voir
    // ui::gamepad_router), la fenêtre principale restant tout en bas de la
    // pile. Jamais démarré si aucune manette n'est branchée ; le Timer doit
    // rester en vie jusqu'à la fin de main(), comme celui de l'horloge.
    let _gamepad_timer = if router.borrow().is_available() {
        router.borrow_mut().push_target(Rc::new(AppGamepadTarget { app: app.clone(), router: router.clone() }));
        let timer = slint::Timer::default();
        let router = router.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(ui::gamepad_router::POLL_INTERVAL_MS), move || {
            // L'emprunt de `router` s'achève à la fin de CETTE instruction :
            // `poll()` renvoie des données sans dispatcher lui-même, donc
            // `dispatch` appelé après peut rouvrir/fermer un dialogue
            // (push_dialog_target/close_current_dialog empruntent aussi
            // `router`) sans chevaucher cet emprunt. Voir GamepadRouter::poll
            // pour le crash que ça évite.
            let result = router.borrow_mut().poll();
            // `poll()` tourne même en arrière-plan pour garder `held_dirs`/
            // `held_buttons` synchronisés avec l'état RÉEL de la manette --
            // sinon reprendre le focus avec un bouton encore enfoncé le
            // lirait comme un appui neuf. Seul le DISPATCH est sauté hors
            // focus (voir foreground_window_belongs_to_us).
            if let Some(result) = result {
                if windows_chrome::foreground_window_belongs_to_us() {
                    ui::gamepad_router::dispatch(result);
                }
            }
        });
        Some(timer)
    } else {
        None
    };

    // Doit rester en vie jusqu'à la fin de main(), comme les Timers
    // ci-dessus -- None en usage normal, et toujours None en release où le
    // module stress_test n'est pas compilé.
    #[cfg(debug_assertions)]
    let _stress_driver =
        stress_test_iterations.map(|iterations| stress_test::start_visual_stress_driver(app.clone(), router.clone(), iterations));

    // window.show() a déjà été appelé plus haut -- reste la boucle
    // d'évènements, puis hide() en sortie, exactement ce qu'aurait fait
    // run(). Sous --visual-stress-test, le driver appelle lui-même
    // slint::quit_event_loop() après `iterations`.
    slint::run_event_loop().expect("échec de la boucle d'évènements");
    let _ = window.hide();
}
