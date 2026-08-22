//! Client HTTP partagé -- point de construction unique pour tous les appels
//! réseau du launcher (GitHub, GitLab, téléchargement de release, jaquettes,
//! `ports.json`).
//!
//! Le User-Agent explicite est requis par l'API GitHub, et un trafic sortant
//! identifié comme venant d'une application nommée réduit les faux positifs
//! des scanners comportementaux.

use std::time::Duration;

const USER_AGENT: &str = concat!("Ports-Launcher/", env!("CARGO_PKG_VERSION"));

pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).user_agent(USER_AGENT).build().into()
}
