use super::*;
use app::dialogs::{close_current_dialog, open_error_dialog, open_message_dialog, open_progress_dialog};
use app::install_launch::{delete_port, start_install};
use app::playtime::is_port_running;
use serde_json::{json, Value};
use ui::chrome;

pub fn parse_stress_test_iterations() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    let idx = args.iter().position(|a| a == "--visual-stress-test")?;
    Some(args.get(idx + 1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(150))
}

const SYNTHETIC_PORT_COUNT: usize = 500;

const REAL_GAME_FOLDER_NAME: &str = "RealLaunchTest";

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
                "folder": format!("StressPort{i}"),
                "executable": "game.exe",
                "instructions": "Synthetic port used by --visual-stress-test.",
                "save": "Save",
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
        "last_launcher_update_check": chrono::Utc::now().to_rfc3339(),
        "installed": {},
    });
    std::fs::write(sandbox.join("state.json"), serde_json::to_string_pretty(&state).unwrap())
        .expect("échec de l'écriture du state.json de stress test");

    sandbox
}

fn copy_real_game_for_launch_test(sandbox: &Path) -> Option<Value> {
    let real_library = base_dir().join("Library");
    let source_dir = std::fs::read_dir(&real_library).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.is_dir())?;
    let folder = source_dir.file_name()?.to_string_lossy().to_string();

    let dest_dir = sandbox.join("Library").join(REAL_GAME_FOLDER_NAME);
    copy_dir_recursive(&source_dir, &dest_dir).ok()?;

    Some(json!({
        "name": format!("Stress Real Launch Test ({folder})"),
        "folder": REAL_GAME_FOLDER_NAME,
        "instructions": "Copy of a real installed port, used only to exercise a real process launch (minimize/foreground) during --visual-stress-test.",
    }))
}

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

pub struct CleanupSandboxOnDrop(pub PathBuf);

impl Drop for CleanupSandboxOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn start_visual_stress_driver(app: Rc<AppState>, router: Rc<RefCell<GamepadRouter>>, iterations: u32) -> slint::Timer {
    let timer = slint::Timer::default();
    let i = Rc::new(Cell::new(0u32));
    const QUERIES: [&str; 6] = ["stress", "port", "1", "zzz-no-match", "", "  "];
    const POSITIONS: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (0.5, 0.5), (0.0, 1.0), (1.0, 1.0)];
    const MOVES: [(i32, i32); 8] = [(0, 1), (0, -1), (1, 0), (-1, 0), (0, 5), (0, -5), (1, 1), (-1, -1)];
    const WINDOW_PERCENTS: [i32; 5] = [10, 100, 50, 30, 80];
    let real_game_port = app.catalog.borrow().iter().find(|p| p.folder == REAL_GAME_FOLDER_NAME).cloned();

    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(500), move || {
        let iter = i.get();
        if iter >= iterations {
            chrome::show_info("Ports Launcher -- stress test", &format!("{iterations} itérations terminées sans plantage."));
            let _ = slint::quit_event_loop();
            return;
        }
        i.set(iter + 1);

        if dialog_is_open(&app) {
            close_current_dialog(&app, &router);
        }

        let query = QUERIES[iter as usize % QUERIES.len()];
        *app.windowed_nav.search_query.borrow_mut() = query.to_string();
        if app.window().get_big_mode() {
            app.rebuild_grid(true);
        } else {
            app.rebuild_windowed(query);
        }

        app.toggle_fullscreen();
        if iter.is_multiple_of(9) {
            app.toggle_fullscreen();
            app.toggle_fullscreen();
        }

        if !app.window().get_big_mode() {
            let (area_x, area_y, screen_w, screen_h) = chrome::work_area_under_cursor();
            let scale = app.window_geometry.scale.get();
            let (win_w, win_h) = {
                let mode = app.window_geometry.normal_mode.borrow();
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
            let installed = core::installer::is_installed(&port, &app.paths.library_dir);
            if !installed && iter.is_multiple_of(5) {
                start_install(&app, &router, port.clone(), None, None);
            } else if installed && iter % 5 == 1 {
                start_install(&app, &router, port.clone(), None, None);
            } else if installed && iter % 5 == 2 {
                delete_port(&app, &router, &port);
            } else if installed {
                launch_flow(&app, &router, &port);
            }

            if iter.is_multiple_of(7) {
                open_info_dialog(&app, &router, &port);
                close_current_dialog(&app, &router);
            }
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

            if let Some(port) = &real_game_port {
                if iter.is_multiple_of(23) && !is_port_running(&app, port.key()) && core::installer::is_installed(port, &app.paths.library_dir) {
                    if let Ok(game_dir) = core::path_safety::safe_join(&app.paths.library_dir, &port.folder) {
                        if let Ok(exe) = core::executable_detect::resolve_executable(port.executable.as_ref(), &game_dir) {
                            if let Ok(child) = core::launch::launch(&exe) {
                                app.install_runtime.running_processes.borrow_mut().insert(port.key().to_string(), child);
                                if app.window().get_big_mode() {
                                    app.window().window().set_minimized(true);
                                    app.install_runtime.minimized_for_game.set(true);
                                }
                                if let Some(core::launch::LaunchedProcess::Native(proc_child)) =
                                    app.install_runtime.running_processes.borrow_mut().get_mut(port.key())
                                {
                                    let _ = proc_child.kill();
                                }
                            }
                        }
                    }
                }
            }

            let (mdx, mdy) = MOVES[iter as usize % MOVES.len()];
            if app.window().get_big_mode() {
                app.move_grid_selection(mdx, mdy);
            } else if mdx != 0 {
                app.move_windowed_selection(mdx * PAGE_ROWS);
            } else {
                app.move_windowed_selection(mdy);
            }
            let displayed_len =
                if app.window().get_big_mode() { app.grid_nav.displayed_installed.borrow().len() } else { app.windowed_nav.displayed_windowed.borrow().len() };
            if displayed_len > 0 {
                assert!(
                    app.current_selected_port().is_some(),
                    "stress test visuel : sélection invalide après navigation (grille={}, len={displayed_len}, mouvement={mdx:?}/{mdy:?})",
                    app.window().get_big_mode()
                );
            }

            if !app.window().get_big_mode() {
                app.set_window_size_percent(WINDOW_PERCENTS[iter as usize % WINDOW_PERCENTS.len()]);
                let border_delta = if iter.is_multiple_of(2) { 1 } else { -1 };
                app.adjust_border(border_delta);
            }
        }

        if !app.window().get_big_mode() && !app.windowed_nav.displayed_windowed.borrow().is_empty() {
            let before = app.windowed_nav.windowed_selected.get();
            app.refresh_current_view();
            assert_eq!(
                app.windowed_nav.windowed_selected.get(),
                before,
                "stress test visuel : un refresh neutre a déplacé la sélection sans changement de liste (régression)"
            );
        }
    });
    timer
}
