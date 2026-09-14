//! Temps de jeu : accumulation à la sortie d'un process, checkpoint
//! périodique anti-crash, affichage en direct dans l'InfoDialog ouvert, et
//! horodatage "dernière partie" (voir `record_playtime`).

use super::dialogs::DialogSlot;
use super::state::AppState;
use crate::Tr;
use slint::ComponentHandle;
use std::time::Instant;

/// Décimale façon Steam ("165.5", "0.0") -- insérée dans le gabarit traduit
/// "{} hours" côté .slint (voir Tr.playtime-status), jamais de suffixe
/// concaténé ici pour rester localisable. Une seule valeur décimale se
/// compare d'un coup d'œil entre deux jeux.
pub(crate) fn format_playtime(seconds: u64) -> String {
    format!("{:.1}", seconds as f64 / 3600.0)
}

/// Trois façons d'afficher "Last played" (voir `Tr.last-played-status*`) --
/// un simple `Option<String>` ne suffit plus dès qu'"aujourd'hui" mérite son
/// propre gabarit plutôt qu'une date répétant ce qu'on sait déjà.
pub(crate) enum LastPlayed {
    Never,
    Today,
    Date(String),
}

/// Décide comment afficher `last_played_at` (voir
/// `InstalledInfo::last_played_at`/`StateManager::mark_played`) --
/// `LastPlayed::Never` si jamais joué (champ vide) ou si le format ne parse
/// pas (`state.json` corrompu/édité à la main), `Today` si la date locale
/// coïncide avec celle du jour, sinon la date formatée (voir
/// `core::clock::format_date`).
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

/// Ajoute au temps de jeu persisté (voir `StateManager::add_playtime`) la
/// durée écoulée depuis le lancement OU le dernier checkpoint (voir
/// `checkpoint_playtime`) -- appelée UNIQUEMENT au moment où
/// `is_port_running`/`any_process_running` détectent qu'un process vient de
/// se terminer, jamais pendant qu'il tourne encore (l'affichage EN DIRECT
/// pendant qu'il tourne se fait ailleurs, voir `poll_app_events`, sans
/// jamais toucher au disque). No-op silencieux si aucun horodatage n'est
/// connu pour `key` (ne devrait pas arriver, chaque entrée de
/// `running_processes` a la sienne, voir `launch_executable`).
///
/// Pose aussi `last_played_at` à MAINTENANT et rafraîchit la vue affichée --
/// à la FIN de la partie, jamais au lancement : "dernière fois joué" désigne
/// le dernier instant réellement joué, pas le premier. Un crash du launcher/
/// PC en cours de partie laisse au pire cette date sur l'avant-dernière
/// session -- sans conséquence pour `playtime_seconds` (une vraie donnée,
/// jamais en jeu ici), ce n'est qu'un ordre d'affichage qui met un tour de
/// plus à se rafraîchir. Même point de convergence que l'arrêt de la
/// présence Discord juste en dessous : tout ce qui doit se passer "cette
/// session vient de se terminer" vit ici.
///
/// Arrête aussi la présence Discord de cette partie si elle en avait une
/// (voir `core::discord_presence`), symétriquement à son démarrage dans
/// `launch_executable`.
pub(crate) fn record_playtime(app: &AppState, key: &str) {
    if let Some(started_at) = app.install_runtime.launch_started_at.borrow_mut().remove(key) {
        app.state.borrow_mut().add_playtime(key, started_at.elapsed().as_secs());
    }
    if let Some(handle) = app.install_runtime.discord_presence.borrow_mut().remove(key) {
        handle.stop();
    }
    app.state.borrow_mut().mark_played(key);
    app.refresh_current_view();
}

/// Vrai si un process lancé pour `key` tourne ENCORE -- vérifié
/// paresseusement à chaque action concernée (lancement/install/
/// désinstallation) plutôt que par un timer dédié. Nettoie l'entrée au
/// passage : sans ce `try_wait`, un process terminé resterait zombie dans
/// la table.
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

/// Vrai si AU MOINS UN port lancé tourne encore -- sert uniquement à savoir
/// quand remonter la fenêtre minimisée après un lancement en plein écran
/// (voir poll_app_events). Plusieurs jeux peuvent tourner en même temps,
/// donc la remontée n'a lieu qu'une fois TOUS terminés, jamais dès que l'un
/// d'eux se ferme. Nettoie les entrées terminées, comme is_port_running.
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

/// Borne la perte de temps de jeu en cas de crash (PC ou launcher) à cet
/// intervalle plutôt qu'à la session entière -- seule une sortie PROPRE du
/// jeu (voir `record_playtime`) persistait quoi que ce soit sur le disque
/// avant l'ajout de ce checkpoint.
const PLAYTIME_CHECKPOINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Appelée à chaque tick de `poll_app_events`, jamais coûteuse : ne compare
/// que des `Instant` pour chaque partie en cours, une écriture disque
/// n'arrive que tous les `PLAYTIME_CHECKPOINT_INTERVAL`. Remet l'horodatage
/// à `Instant::now()` juste après avoir persisté son écart -- le prochain
/// checkpoint (ou le `record_playtime` du vrai exit) ne recompte donc jamais
/// deux fois le même intervalle.
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

/// Rafraîchit "Play time: {} hours" de l'InfoDialog actuellement ouvert
/// PENDANT qu'une partie tourne pour ce port -- purement en mémoire (temps
/// persisté plus l'écart depuis le lancement/dernier checkpoint), jamais
/// d'écriture disque ici (voir `checkpoint_playtime` pour ça). No-op si
/// aucun InfoDialog n'est ouvert, ou s'il ne correspond à aucune partie en
/// cours (voir `info_dialog_port_key`).
pub(crate) fn refresh_live_playtime_display(app: &AppState) {
    let Some(key) = app.dialog_nav.info_dialog_port_key.borrow().clone() else { return };
    let Some(started) = app.install_runtime.launch_started_at.borrow().get(&key).copied() else { return };
    let dialogs = app.dialog_nav.dialogs.borrow();
    let DialogSlot::Info(dialog) = &*dialogs else { return };
    let base = app.state.borrow().get(&key).map(|i| i.playtime_seconds).unwrap_or(0);
    let live_seconds = base + started.elapsed().as_secs();
    let text = dialog.global::<Tr>().invoke_playtime_status(format_playtime(live_seconds).into());
    // format_playtime n'a qu'une décimale d'heure (~6 minutes) de
    // granularité -- comparer avant d'écrire évite de déclencher un repaint
    // à chaque tick de 100ms pour rien.
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
        assert_eq!(format_playtime(595_800), "165.5"); // 165.5 * 3600
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
