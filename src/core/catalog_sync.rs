//! Rafraîchissement de `ports.json` depuis GitHub, en tâche de fond -- ne
//! bloque JAMAIS le démarrage de l'appli sur le réseau (voir main(), qui
//! charge TOUJOURS la copie locale/embarquée en premier, de façon
//! synchrone, avant même de créer la fenêtre ; ce module ne fait que la
//! rafraîchir APRÈS, exactement comme les vérifications de mise à jour des
//! ports/du launcher lui-même, voir start_update_checks/
//! start_self_update_check).
//!
//! `raw.githubusercontent.com` plutôt que l'API GitHub (`api.github.com`) :
//! ce n'est pas l'API, juste un CDN de fichiers -- pas soumis au même quota
//! non authentifié (60 req/h, voir state.rs::RELEASE_CHECK_INTERVAL_HOURS)
//! que les vérifications de release, d'où son propre throttle ENTIÈREMENT
//! séparé (CATALOG_CHECK_INTERVAL_HOURS, sans rapport avec ce quota). Pas
//! besoin de jeton non plus, et une requête conditionnelle (`If-None-Match`)
//! coûte quasiment rien (`304`, pas de corps) quand le catalogue n'a pas
//! changé -- pas la peine de retélécharger tout le fichier à chaque
//! vérification pour rien.

use super::{config, http};
use std::time::Duration;

const RAW_URL: &str = "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/ports.json";

pub enum CatalogUpdate {
    /// `304 Not Modified` -- rien à faire, le catalogue local est déjà à
    /// jour. Distinct de `Updated` pour que l'appelant sache qu'il n'a même
    /// pas besoin de réécrire le fichier local.
    NotModified,
    /// `200` avec un nouveau contenu, DÉJÀ VALIDÉ comme un catalogue
    /// exploitable (voir `config::parse_catalog`, appelé ici avant de
    /// renvoyer quoi que ce soit) -- prêt à écrire sur disque tel quel.
    Updated { text: String, etag: String },
}

/// `known_etag` vide -- première vérification, ou ETag jamais mémorisé --
/// envoie quand même la requête sans `If-None-Match` plutôt que d'échouer :
/// se comporte alors comme un simple GET, ce qui est le comportement
/// correct pour "je ne sais pas encore ce que j'ai".
pub fn fetch_if_changed(known_etag: &str) -> Result<CatalogUpdate, String> {
    let agent = http::agent(Duration::from_secs(30));
    let mut req = agent.get(RAW_URL);
    if !known_etag.is_empty() {
        req = req.header("If-None-Match", known_etag);
    }
    match req.call() {
        Ok(mut resp) => {
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
            // Valide AVANT de renvoyer quoi que ce soit : l'appelant
            // n'écrase la copie locale que si ceci réussit. Un JSON cassé
            // côté distant (erreur du mainteneur, réponse tronquée) ne doit
            // jamais rendre le launcher inutilisable au prochain lancement.
            config::parse_catalog(&text).map_err(|e| e.to_string())?;
            Ok(CatalogUpdate::Updated { text, etag })
        }
        // ureq traite tout statut hors 200-299 comme une erreur typée plutôt
        // qu'une réponse Ok -- y compris 304, ici le cas nominal.
        Err(ureq::Error::StatusCode(304)) => Ok(CatalogUpdate::NotModified),
        Err(e) => Err(e.to_string()),
    }
}
