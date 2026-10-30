use super::version::PROJECT_URL;
use discord_rich_presence::activity::{Activity, ActivityType, Assets, Button, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "1548801841557541065";

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const START_DELAY: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(15);

const SMALL_IMAGE_KEY: &str = "icon_discord_apps";

const MAX_TEXT_LEN: usize = 128;

fn truncate_for_discord(s: &str) -> String {
    if s.chars().count() <= MAX_TEXT_LEN {
        return s.to_string();
    }
    let mut truncated: String = s.chars().take(MAX_TEXT_LEN - 1).collect();
    truncated.push('…');
    truncated
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub(crate) struct PresenceHandle {
    stop: Arc<AtomicBool>,
}

impl PresenceHandle {
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn start(game_name: String, folder: String, large_image: Option<String>) -> PresenceHandle {
    start_activity(game_name, Some(folder), large_image)
}

pub(crate) fn start_custom(text: String, icon_url: Option<String>) -> PresenceHandle {
    start_activity(text, None, icon_url)
}

fn sleep_until_stopped(stop: &AtomicBool, duration: Duration) -> bool {
    let mut waited = Duration::ZERO;
    while waited < duration {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(POLL_INTERVAL);
        waited += POLL_INTERVAL;
    }
    stop.load(Ordering::Relaxed)
}

fn start_activity(name: String, details: Option<String>, large_image: Option<String>) -> PresenceHandle {
    let name = truncate_for_discord(&name);
    let details = details.map(|d| truncate_for_discord(&d));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    std::thread::spawn(move || {
        let started_at = now_unix();
        if sleep_until_stopped(&stop_thread, START_DELAY) {
            return;
        }
        let mut client: Option<DiscordIpcClient> = None;
        loop {
            if client.is_none() {
                let mut fresh = DiscordIpcClient::new(CLIENT_ID);
                if fresh.connect().is_ok() {
                    client = Some(fresh);
                }
            }
            if let Some(connected) = client.as_mut() {
                let mut assets = Assets::new();
                if let Some(image_url) = large_image.as_deref() {
                    assets = assets.large_image(image_url).large_text(&name).small_image(SMALL_IMAGE_KEY).small_text("Ports Launcher");
                }
                let mut activity = Activity::new()
                    .activity_type(ActivityType::Playing)
                    .name(&name)
                    .timestamps(Timestamps::new().start(started_at))
                    .assets(assets)
                    .buttons(vec![Button::new("Ports Launcher", PROJECT_URL)]);
                if let Some(details) = details.as_deref() {
                    activity = activity.details(details);
                }
                if connected.set_activity(activity).is_err() {
                    let _ = connected.close();
                    client = None;
                }
            }
            if sleep_until_stopped(&stop_thread, REFRESH_INTERVAL) {
                break;
            }
        }
        if let Some(mut connected) = client {
            let _ = connected.clear_activity();
            let _ = connected.close();
        }
    });

    PresenceHandle { stop }
}
