
use super::version::PROJECT_URL;
use discord_rich_presence::activity::{Activity, ActivityType, Assets, Button, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "1548801841557541065";

const POLL_INTERVAL: Duration = Duration::from_millis(500);

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
    let game_name = truncate_for_discord(&game_name);
    let folder = truncate_for_discord(&folder);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    std::thread::spawn(move || {
        let mut client = DiscordIpcClient::new(CLIENT_ID);
        if client.connect().is_err() {
            return;
        }

        let mut assets = Assets::new();
        if let Some(image_url) = large_image.as_deref() {
            assets = assets.large_image(image_url).large_text(&game_name).small_image(SMALL_IMAGE_KEY).small_text("Ports Launcher");
        }
        let activity = Activity::new()
            .activity_type(ActivityType::Playing)
            .name(&game_name)
            .details(&folder)
            .timestamps(Timestamps::new().start(now_unix()))
            .assets(assets)
            .buttons(vec![Button::new("Ports Launcher", PROJECT_URL)]);
        let _ = client.set_activity(activity);

        while !stop_thread.load(Ordering::Relaxed) {
            std::thread::sleep(POLL_INTERVAL);
        }
        let _ = client.clear_activity();
        let _ = client.close();
    });

    PresenceHandle { stop }
}
