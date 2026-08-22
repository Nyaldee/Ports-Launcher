//! Harnais de stress test visuel (`--visual-stress-test`) -- séparé de
//! main.rs pour ne pas y noyer la logique applicative sous le pilotage de
//! test. Ce module est un ENFANT de main.rs au sens des modules Rust, donc
//! `use super::*;` lui donne accès à tout ce qui y est défini, même sans
//! `pub` (AppState, GamepadRouter, start_install/delete_port/launch_flow,
//! base_dir, PAGE_ROWS, l'import `slint`...).
use super::*;
use ui::windows_chrome;

/// Argument de lancement caché, absent de toute aide utilisateur -- réservé
/// au harnais de stress test visuel (voir start_visual_stress_driver) : une
/// vraie fenêtre pilotée par code sur un catalogue synthétique isolé, réseau
/// et lancement de process neutralisés, jamais les vrais
/// Library/state.json/cache/ports.json de l'utilisateur. Usage :
/// `ports_launcher.exe --visual-stress-test [iterations]` (150 par défaut).
pub fn parse_stress_test_iterations() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    let idx = args.iter().position(|a| a == "--visual-stress-test")?;
    Some(args.get(idx + 1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(150))
}

/// Nombre de ports synthétiques -- largement au-dessus du vrai `ports.json`
/// (~70 entrées) pour stresser colonnes, défilement virtualisé et recherche
/// bien au-delà de l'usage réel. Sans effet sur le réseau : toutes les URLs
/// pointent sur `https://example.invalid/...` (TLD réservé, RFC 2606,
/// garanti de ne jamais résoudre), donc chaque install échoue localement et
/// instantanément. Volontairement sous les 3000 du fuzzing pur de
/// `core::search::tests::stress_requetes_adversariales_sur_un_gros_catalogue_ne_plante_jamais`,
/// qui n'a aucune fenêtre à rendre.
const SYNTHETIC_PORT_COUNT: usize = 500;

/// Dossier (dans le sandbox) d'une copie d'une VRAIE install trouvée dans la
/// bibliothèque réelle de l'utilisateur -- voir
/// `copy_real_game_for_launch_test`.
const REAL_GAME_FOLDER_NAME: &str = "RealLaunchTest";

/// Sandbox isolé pour `--visual-stress-test`. Le `ports.json` synthétique
/// omet volontairement "website"/"mods_url"/"image_url" : ces champs
/// déclencheraient un vrai ShellExecuteW ou téléchargement d'image s'ils
/// étaient exercés, autant qu'ils soient structurellement absents.
/// `themes.json` est copié du vrai dossier s'il existe, pour un rendu réel
/// plutôt qu'un thème de repli. `state.json` part avec `last_release_check` à
/// "maintenant", ce qui désactive `should_check_releases()` : ni
/// `start_update_checks` ni `start_self_update_check` n'atteignent le réseau
/// pendant ce run.
///
/// Jamais les vrais Library/state.json/cache/ports.json de l'utilisateur --
/// seule exception, `copy_real_game_for_launch_test` COPIE (sans jamais le
/// référencer) un port déjà installé.
pub fn build_stress_sandbox() -> PathBuf {
    let sandbox = std::env::temp_dir().join(format!("ports_launcher_stress_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(&sandbox).expect("échec de la création du sandbox de stress test");

    let mut ports: Vec<Value> = (0..SYNTHETIC_PORT_COUNT)
        .map(|i| {
            json!({
                "name": format!("Stress Port {i}"),
                "tags": ["stress", format!("tag{}", i % 3)],
                "source": format!("https://example.invalid/stress-port-{i}.zip"),
                "folder_name": format!("StressPort{i}"),
                "executable": "game.exe",
                "instructions": "Synthetic port used by --visual-stress-test.",
                "save_folder": "Save",
            })
        })
        .collect();
    if let Some(real_game) = copy_real_game_for_launch_test(&sandbox) {
        ports.push(real_game);
    }
    std::fs::write(sandbox.join("ports.json"), serde_json::to_string_pretty(&json!({ "ports": ports })).unwrap())
        .expect("échec de l'écriture du ports.json de stress test");

    let real_themes = base_dir().join("themes.json");
    if real_themes.exists() {
        let _ = std::fs::copy(&real_themes, sandbox.join("themes.json"));
    }

    let state = json!({
        "github_token": null,
        "gitlab_token": null,
        "fullscreen": false,
        "last_release_check": chrono::Utc::now().to_rfc3339(),
        "installed": {},
    });
    std::fs::write(sandbox.join("state.json"), serde_json::to_string_pretty(&state).unwrap())
        .expect("échec de l'écriture du state.json de stress test");

    sandbox
}

/// COPIE (jamais un lien : `delete_port` tourne aussi dans le sandbox et ne
/// doit pas pouvoir toucher les vrais fichiers) le premier port réellement
/// installé de la vraie bibliothèque (`base_dir()/Library`) vers
/// `sandbox/Library/RealLaunchTest`, et renvoie l'entrée `ports.json`
/// correspondante. `None` si rien n'est installé -- le test de lancement
/// réel est alors simplement sauté, jamais fatal.
///
/// Nécessaire parce que les ports synthétiques pointent tous sur un
/// "game.exe" placeholder au contenu arbitraire, que `launch_executable`
/// refuse de lancer (voir sa garde `app.stress_test`) : Windows passerait
/// par NTVDM et afficherait une popup système qu'aucun clic automatisé ne
/// doit fermer. Une vraie install a un `.exe` valide, ce qui permet
/// d'exercer pour de bon le cycle lancement/minimisation/retour au premier
/// plan.
fn copy_real_game_for_launch_test(sandbox: &Path) -> Option<Value> {
    let real_library = base_dir().join("Library");
    let source_dir = std::fs::read_dir(&real_library).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.is_dir())?;
    let folder_name = source_dir.file_name()?.to_string_lossy().to_string();

    let dest_dir = sandbox.join("Library").join(REAL_GAME_FOLDER_NAME);
    copy_dir_recursive(&source_dir, &dest_dir).ok()?;

    Some(json!({
        "name": format!("Stress Real Launch Test ({folder_name})"),
        "folder_name": REAL_GAME_FOLDER_NAME,
        "instructions": "Copy of a real installed port, used only to exercise a real process launch (minimize/foreground) during --visual-stress-test.",
    }))
}

/// Copie récursive minimale -- `std::fs` n'a pas d'équivalent, et une
/// dépendance externe serait disproportionnée pour ce seul usage.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Supprime récursivement le sandbox de stress test au drop (voir
/// build_stress_sandbox). Un Drop plutôt qu'un `remove_dir_all` en fin de
/// `main()` : une assertion qui panique en cours de route déroule la pile et
/// nettoie quand même, tant que le binaire n'est pas compilé avec `panic =
/// "abort"` (profil `release`, voir Cargo.toml), où le process s'arrête sans
/// dérouler et laisse le dossier dans %TEMP%. Contrepartie assumée : un
/// dossier résiduel dans %TEMP% après un plantage réel, jamais les vrais
/// fichiers de l'utilisateur.
pub struct CleanupSandboxOnDrop(pub PathBuf);

impl Drop for CleanupSandboxOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Pilote une vraie fenêtre visible à travers une séquence bornée d'actions
/// -- recherche, bascules plein écran martelées, navigation clavier/manette
/// sur la liste ET la grille, redimensionnement Ctrl+chiffres, bordure
/// Ctrl+-/Ctrl+=, déplacement à l'écran, install/update/désinstall/
/// lancement, ouverture et fermeture de chaque type de dialogue -- pour
/// tenter de la faire dérailler visuellement.
///
/// Jamais de SendInput/PostMessage : aucune simulation d'évènement
/// clavier/souris au niveau OS, chaque action appelle directement la
/// fonction Rust qu'un vrai clic ou une vraie touche aurait appelée.
/// S'arrête d'elle-même après `iterations`, jamais une boucle infinie non
/// supervisée, et ferme la fenêtre comme le ferait l'utilisateur.
pub fn start_visual_stress_driver(app: Rc<AppState>, router: Rc<RefCell<GamepadRouter>>, iterations: u32) -> slint::Timer {
    let timer = slint::Timer::default();
    let i = Rc::new(Cell::new(0u32));
    const QUERIES: [&str; 6] = ["stress", "port", "1", "zzz-no-match", "", "  "];
    // Fractions (x, y) de l'espace disponible -- coins puis centre, pour
    // que la fenêtre se déplace visiblement d'une itération à l'autre.
    const POSITIONS: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (0.5, 0.5), (0.0, 1.0), (1.0, 1.0)];
    // Directions de navigation clavier/manette -- même dispatch que
    // on_move_requested : dx pour gauche/droite (page en liste, colonne en
    // grille), dy pour haut/bas, avec des sauts de plusieurs lignes pour
    // vraiment faire défiler.
    const MOVES: [(i32, i32); 8] = [(0, 1), (0, -1), (1, 0), (-1, 0), (0, 5), (0, -5), (1, 1), (-1, -1)];
    // Ctrl+1..9/0 -- tailles de fenêtre représentatives, bornes incluses.
    const WINDOW_PERCENTS: [i32; 5] = [10, 100, 50, 30, 80];
    // Résolu une seule fois, le catalogue ne changeant pas pendant le run --
    // absent si rien n'est réellement installé (voir
    // copy_real_game_for_launch_test), le test de lancement est alors sauté.
    let real_game_port = app.catalog.borrow().iter().find(|p| p.folder_name == REAL_GAME_FOLDER_NAME).cloned();

    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(500), move || {
        let iter = i.get();
        if iter >= iterations {
            windows_chrome::show_info("Ports Launcher -- stress test", &format!("{iterations} itérations terminées sans plantage."));
            let _ = slint::quit_event_loop();
            return;
        }
        i.set(iter + 1);

        // Un dialogue peut s'être ouvert tout seul depuis le tour précédent
        // (AppEvent::InstallError sur l'URL factice du sandbox, par
        // exemple), sans que ce driver l'ait demandé. Fermé d'entrée de jeu,
        // exactement comme le ferait Entrée/Échap, pour qu'il ne bloque pas
        // le reste du tour.
        if dialog_is_open(&app) {
            close_current_dialog(&app, &router);
        }

        // Recherche -- exactement le corps de on_search_changed, jamais une
        // frappe clavier simulée.
        let query = QUERIES[iter as usize % QUERIES.len()];
        *app.search_query.borrow_mut() = query.to_string();
        if app.window().get_big_mode() {
            app.rebuild_grid(true);
        } else {
            app.rebuild_windowed(query);
        }

        // Martelage du plein écran -- une bascule chaque tour et,
        // régulièrement, plusieurs dans le MÊME tick, sans laisser le temps
        // à quoi que ce soit de se stabiliser entre les deux. Volontairement
        // hors de la garde dialog_is_open : un vrai Alt+Entrée n'atteindrait
        // pas la fenêtre principale pendant qu'un dialogue modal a le focus,
        // mais rien n'empêche structurellement `toggle_fullscreen` d'être
        // appelée dans cet état, ce qui en fait une séquence valable à
        // stresser au niveau Rust.
        app.toggle_fullscreen();
        if iter.is_multiple_of(9) {
            app.toggle_fullscreen();
            app.toggle_fullscreen();
        }

        // Déplace la fenêtre à l'écran, jamais en plein écran où sa
        // géométrie couvre déjà tout -- même API qu'apply_mode_geometry.
        if !app.window().get_big_mode() {
            let (area_x, area_y, screen_w, screen_h) = windows_chrome::work_area_under_cursor();
            let scale = app.scale.get();
            let (win_w, win_h) = {
                let mode = app.normal_mode.borrow();
                ((mode.logical_width * scale) as i32, (mode.logical_height * scale) as i32)
            };
            let (fx, fy) = POSITIONS[iter as usize % POSITIONS.len()];
            let x = area_x + ((screen_w - win_w).max(0) as f32 * fx) as i32;
            let y = area_y + ((screen_h - win_h).max(0) as f32 * fy) as i32;
            app.window().window().set_position(slint::WindowPosition::Physical(slint::PhysicalPosition { x, y }));
        }

        if !dialog_is_open(&app) {
            let port = {
                let catalog = app.catalog.borrow();
                catalog[iter as usize % catalog.len()].clone()
            };
            let installed = core::installer::is_installed(&port, &app.library_dir);
            if !installed && iter.is_multiple_of(5) {
                start_install(&app, &router, port.clone(), None, None);
            } else if installed && iter % 5 == 1 {
                start_install(&app, &router, port.clone(), None, None); // Update -- même chemin qu'Install.
            } else if installed && iter % 5 == 2 {
                delete_port(&app, &router, &port);
            } else if installed {
                launch_flow(&app, &router, &port);
            }

            if iter.is_multiple_of(7) {
                open_info_dialog(&app, &router, &port);
                close_current_dialog(&app, &router);
            }
            // Les autres types de dialogue (voir DialogSlot) -- ouverts puis
            // refermés immédiatement, sur des tours différents pour ne
            // jamais se marcher dessus. Message/Error mesurent un texte réel
            // (voir ui::dialog_geometry::message_dialog_size), d'où deux
            // longueurs : une courte, une qui replie sur plusieurs lignes.
            if iter.is_multiple_of(13) {
                let message = if iter.is_multiple_of(26) {
                    "Short stress message."
                } else {
                    "A much longer stress test message, deliberately verbose, meant to wrap onto several lines and exercise message_dialog_size's real text measurement rather than a short one-liner."
                };
                open_message_dialog(&app, &router, "Stress Message", message);
                close_current_dialog(&app, &router);
            }
            if iter.is_multiple_of(17) {
                open_error_dialog(&app, &router, port.clone());
                close_current_dialog(&app, &router);
            }
            if iter.is_multiple_of(19) {
                open_progress_dialog(&app, &router, "Stress Progress", "Simulated progress status...");
                close_current_dialog(&app, &router);
            }

            // Test de lancement RÉEL (jamais le faux "game.exe", voir
            // copy_real_game_for_launch_test) -- exerce le cycle
            // minimisation-au-lancement/retour au premier plan de
            // launch_executable/poll_app_events. Cadence large pour laisser
            // ce cycle se dérouler entre deux tentatives. Sauté
            // silencieusement si rien n'a pu être copié ou si l'entrée a été
            // supprimée entre-temps par le brassage ci-dessus.
            if let Some(port) = &real_game_port {
                if iter.is_multiple_of(23) && !is_port_running(&app, port.key()) && core::installer::is_installed(port, &app.library_dir) {
                    if let Ok(game_dir) = core::platform_utils::safe_join(&app.library_dir, &port.folder_name) {
                        if let Ok(exe) = core::platform_utils::resolve_executable(port.executable.as_ref(), &game_dir) {
                            if let Ok(child) = core::launch::launch(&exe) {
                                app.running_processes.borrow_mut().insert(port.key().to_string(), child);
                                if app.window().get_big_mode() {
                                    app.window().window().set_minimized(true);
                                    app.minimized_for_game.set(true);
                                }
                                // Tué tout de suite : il s'agit de prouver
                                // que le cycle lancement -> minimisation ->
                                // process terminé -> retour au premier plan
                                // fonctionne, pas de laisser tourner un vrai
                                // jeu dont la durée échappe au driver.
                                if let Some(core::launch::LaunchedProcess::Native(proc_child)) =
                                    app.running_processes.borrow_mut().get_mut(port.key())
                                {
                                    let _ = proc_child.kill();
                                }
                            }
                        }
                    }
                }
            }

            // Navigation clavier/manette -- fait défiler liste ET grille
            // (voir move_grid_selection/move_windowed_selection), même
            // dispatch que on_move_requested. Cycle sur haut/bas/gauche/
            // droite/diagonale et sauts de plusieurs lignes, dans les deux
            // modes au fil des bascules plein écran ci-dessus.
            let (mdx, mdy) = MOVES[iter as usize % MOVES.len()];
            if app.window().get_big_mode() {
                app.move_grid_selection(mdx, mdy);
            } else if mdx != 0 {
                app.move_windowed_selection(mdx * PAGE_ROWS);
            } else {
                app.move_windowed_selection(mdy);
            }
            // Invariant : tant que la liste/grille affichée n'est pas vide,
            // une navigation doit TOUJOURS retomber sur un port réel -- la
            // classe de bug que couvre core::grid::next_grid_position (une
            // carte seule sur une dernière ligne incomplète).
            let displayed_len =
                if app.window().get_big_mode() { app.displayed_installed.borrow().len() } else { app.displayed_windowed.borrow().len() };
            if displayed_len > 0 {
                assert!(
                    app.current_selected_port().is_some(),
                    "stress test visuel : sélection invalide après navigation (grille={}, len={displayed_len}, mouvement={mdx:?}/{mdy:?})",
                    app.window().get_big_mode()
                );
            }

            // Ctrl+redimensionnement / Ctrl+bordure -- FENÊTRÉ UNIQUEMENT,
            // même garde !big-mode que window-size-requested/
            // border-adjust-requested dans app-window.slint.
            if !app.window().get_big_mode() {
                app.set_window_size_percent(WINDOW_PERCENTS[iter as usize % WINDOW_PERCENTS.len()]);
                let border_delta = if iter.is_multiple_of(2) { 1 } else { -1 };
                app.adjust_border(border_delta);
            }
        }

        // Invariant (voir rebuild_windowed/previously_selected_key) : un
        // refresh neutre -- même recherche, même liste -- ne doit jamais
        // déplacer la sélection.
        if !app.window().get_big_mode() && !app.displayed_windowed.borrow().is_empty() {
            let before = app.windowed_selected.get();
            app.refresh_current_view();
            assert_eq!(
                app.windowed_selected.get(),
                before,
                "stress test visuel : un refresh neutre a déplacé la sélection sans changement de liste (régression)"
            );
        }
    });
    timer
}
