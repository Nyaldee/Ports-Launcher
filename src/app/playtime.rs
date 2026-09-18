
use super::dialogs::DialogSlot;
use super::state::AppState;
use crate::Tr;
use slint::ComponentHandle;
use std::time::Instant;

pub(crate) fn format_playtime(seconds: u64) -> String {
    format!("{:.1}", seconds as f64 / 3600.0)
}

pub(crate) enum LastPlayed {
    Never,
    Today,
    Date(String),
}

pub(crate) fn format_last_played(last_played_at: &str) -> LastPlayed {
    if last_played_at.is_empty() {
        return LastPlayed::Never;
    }
    let Ok(played) = chrono::DateTime::parse_from_rfc3339(last_played_at) else { return LastPlayed::Never };
    let played_local = played.with_timezone(&chrono::Local);
    if played_local.date_naive() == chrono::Local::now().date_naive() {
        return LastPlayed::Today;
    }
    LastPlayed::Date(crate::core::clock::format_date(played_local))
}

pub(crate) fn record_playtime(app: &AppState, key: &str) {
    if let Some(started_at) = app.install_runtime.launch_started_at.borrow_mut().remove(key) {
        app.state.borrow_mut().add_playtime(key, started_at.elapsed().as_secs());
    }
    if let Some(handle) = app.install_runtime.discord_presence.borrow_mut().remove(key) {
        handle.stop();
    }
    app.state.borrow_mut().mark_played(key);
    app.refresh_current_view();
    app.refresh_tray_recent_games();
}

pub(crate) fn is_port_running(app: &AppState, key: &str) -> bool {
    let mut processes = app.install_runtime.running_processes.borrow_mut();
    let Some(process) = processes.get_mut(key) else { return false };
    if process.is_running() {
        true
    } else {
        processes.remove(key);
        drop(processes);
        record_playtime(app, key);
        false
    }
}

pub(crate) fn any_process_running(app: &AppState) -> bool {
    let exited: Vec<String> = {
        let mut processes = app.install_runtime.running_processes.borrow_mut();
        let mut exited = Vec::new();
        for (key, process) in processes.iter_mut() {
            if !process.is_running() {
                exited.push(key.clone());
            }
        }
        for key in &exited {
            processes.remove(key);
        }
        exited
    };
    for key in &exited {
        record_playtime(app, key);
    }
    !app.install_runtime.running_processes.borrow().is_empty()
}

const PLAYTIME_CHECKPOINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub(crate) fn checkpoint_playtime(app: &AppState) {
    let mut started_at = app.install_runtime.launch_started_at.borrow_mut();
    for (key, started) in started_at.iter_mut() {
        let elapsed = started.elapsed();
        if elapsed >= PLAYTIME_CHECKPOINT_INTERVAL {
            *started = Instant::now();
            app.state.borrow_mut().add_playtime(key, elapsed.as_secs());
        }
    }
}

pub(crate) fn refresh_live_playtime_display(app: &AppState) {
    let Some(key) = app.dialog_nav.info_dialog_port_key.borrow().clone() else { return };
    let Some(started) = app.install_runtime.launch_started_at.borrow().get(&key).copied() else { return };
    let dialogs = app.dialog_nav.dialogs.borrow();
    let DialogSlot::Info(dialog) = &*dialogs else { return };
    let base = app.state.borrow().get(&key).map(|i| i.playtime_seconds).unwrap_or(0);
    let live_seconds = base + started.elapsed().as_secs();
    let text = dialog.global::<Tr>().invoke_playtime_status(format_playtime(live_seconds).into());
    if dialog.get_playtime_status_text() != text {
        dialog.set_playtime_status_text(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_playtime_une_decimale() {
        assert_eq!(format_playtime(0), "0.0");
        assert_eq!(format_playtime(3600), "1.0");
        assert_eq!(format_playtime(3600 + 1800), "1.5");
        assert_eq!(format_playtime(595_800), "165.5");
    }

    #[test]
    fn format_last_played_vide_est_never() {
        assert!(matches!(format_last_played(""), LastPlayed::Never));
    }

    #[test]
    fn format_last_played_invalide_est_never() {
        assert!(matches!(format_last_played("not a date"), LastPlayed::Never));
    }

    #[test]
    fn format_last_played_maintenant_est_today() {
        let now = chrono::Utc::now().to_rfc3339();
        assert!(matches!(format_last_played(&now), LastPlayed::Today));
    }

    #[test]
    fn format_last_played_hier_est_une_date() {
        let yesterday = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        assert!(matches!(format_last_played(&yesterday), LastPlayed::Date(_)));
    }
}
