//! Rich Presence Discord, best-effort : toute la connexion IPC et son cycle
//! de vie tournent sur un thread dédié par partie lancée, jamais sur le
//! thread UI -- une connexion qui traîne ou une erreur de la crate ne doit
//! jamais geler/tuer le launcher (voir `Cargo.toml`, `panic = "unwind"`
//! choisi précisément pour isoler ce genre de thread annexe). Désactivé par
//! défaut (voir `StateManager::discord_rpc_enabled`) ; pas de réglage par
//! port -- si un port a son propre Rich Presence avec invitation,
//! l'utilisateur désactive ce réglage le temps d'y jouer.
//!
//! `Activity::name` remplace le "Playing <nom de l'appli>" par défaut de
//! Discord par le nom du jeu (voir `Port::display_name`) -- limite dure de
//! 128 caractères côté Discord, très large pour ce catalogue. `details`
//! affiche le dossier du port (`Port::folder`) juste en dessous : l'identifiant
//! technique du projet, utile à qui suit la scène des ports/recomps sans
//! répéter le nom déjà affiché en en-tête.
//!
//! `large_image` : icône carrée du port (voir `Port::icon`) si connue, sinon
//! sa jaquette verticale (recadrée en carré par Discord), sinon omis -- dans
//! ce dernier cas Discord affiche automatiquement l'icône de l'appli à la
//! place (comportement natif, rien à coder). Quand une image de port est
//! bien affichée, un petit badge Ports Launcher est superposé en coin (voir
//! `small_image`).

use discord_rich_presence::activity::{Activity, ActivityType, Assets, Button, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Identifiant public de l'appli "Ports Launcher" enregistrée sur
/// https://discord.com/developers/applications -- pas un secret (voir
/// discussion de conception), sans risque à committer.
const CLIENT_ID: &str = "1548801841557541065";

/// Le thread de fond revérifie cet indicateur à ce rythme -- assez court
/// pour fermer la présence sans délai perceptible dès la fin de partie,
/// assez long pour ne jamais solliciter le CPU pour rien.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Bouton discret sous le statut -- un simple lien, pas une invitation à
/// rejoindre (voir `party`, jamais utilisé ici pour ne pas entrer en
/// conflit avec le Rich Presence propre d'un port, voir la discussion de
/// conception).
const GITHUB_URL: &str = "https://github.com/Nyaldee/Ports-Launcher";

/// Badge en coin de l'icône du jeu -- une clé d'asset (pas une URL), pointant
/// vers une image uploadée une fois sur le portail développeur (Rich
/// Presence > Art Assets) sous ce même nom. Remplacer l'image là-bas sous ce
/// nom suffit à la mettre à jour -- rien à changer ici.
const SMALL_IMAGE_KEY: &str = "icon_discord_apps";

/// Limite dure de Discord sur `name`/`details`/`state` (voir doc du module) --
/// au-delà, Discord rejette l'activité entière plutôt que de la tronquer.
const MAX_TEXT_LEN: usize = 128;

/// Coupe à `MAX_TEXT_LEN` caractères avec un "…" final si besoin -- aucune
/// entrée du catalogue actuel n'en approche, garde-fou pour une future entrée
/// au nom démesuré plutôt qu'un statut qui disparaît silencieusement.
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

/// Poignée d'une présence active. `stop()` demande au thread de nettoyer
/// (clear_activity + déconnexion) et de se terminer ; n'attend jamais ce
/// nettoyage (appelable depuis le thread UI sans jamais le bloquer).
pub(crate) struct PresenceHandle {
    stop: Arc<AtomicBool>,
}

impl PresenceHandle {
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Démarre la présence pour une partie qui vient d'être lancée -- retourne
/// immédiatement, tout le travail réseau/IPC se fait sur le thread généré.
pub(crate) fn start(game_name: String, folder: String, large_image: Option<String>) -> PresenceHandle {
    let game_name = truncate_for_discord(&game_name);
    let folder = truncate_for_discord(&folder);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    std::thread::spawn(move || {
        let mut client = DiscordIpcClient::new(CLIENT_ID);
        // Discord absent/IPC indisponible : rien à afficher pour cette
        // partie, on abandonne silencieusement sans retenter.
        if client.connect().is_err() {
            return;
        }

        let mut assets = Assets::new();
        if let Some(image_url) = large_image.as_deref() {
            assets = assets
                .large_image(image_url)
                .large_text(&game_name)
                .small_image(SMALL_IMAGE_KEY)
                .small_text("Ports Launcher");
        }
        let activity = Activity::new()
            .activity_type(ActivityType::Playing)
            .name(&game_name)
            .details(&folder)
            .timestamps(Timestamps::new().start(now_unix()))
            .assets(assets)
            .buttons(vec![Button::new("Ports Launcher", GITHUB_URL)]);
        let _ = client.set_activity(activity);

        while !stop_thread.load(Ordering::Relaxed) {
            std::thread::sleep(POLL_INTERVAL);
        }

        let _ = client.clear_activity();
        let _ = client.close();
    });

    PresenceHandle { stop }
}
