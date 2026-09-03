//! Corps des tâches lancées en arrière-plan (thread dédié à une install,
//! pool de threads pour les vérifications de mise à jour) -- volontairement
//! sans aucune dépendance à Slint : seules des données simples (Send) entrent
//! et sortent, le fil d'exécution appelant (voir main.rs) décide seul quoi
//! en faire côté UI, sans jamais bloquer le thread UI lui-même le temps
//! d'un appel réseau/disque.

use super::installer::{self, InstallError, InstallPaths};
use super::models::{Port, SourceType};
use super::{github_api, gitlab_api};
use serde_json::Value;
use std::path::Path;

pub enum InstallOutcome {
    Done { tag: Option<String> },
    AssetAmbiguous { assets: Vec<Value> },
    Error(String),
}

/// `on_progress` est appelé sur CE thread (celui qui exécute `run_install`,
/// jamais celui de l'UI) -- à l'appelant de ne faire que des choses
/// thread-safe dedans (voir main.rs : empile juste le message dans une file
/// partagée, ne touche jamais directement un composant Slint depuis ici).
pub fn run_install(
    port: &Port,
    paths: InstallPaths,
    github_token: Option<&str>,
    gitlab_token: Option<&str>,
    overrides: installer::InstallOverrides,
    on_progress: &mut dyn FnMut(&str),
) -> InstallOutcome {
    match installer::install_port(port, paths, github_token, gitlab_token, overrides, Some(on_progress)) {
        Ok(tag) => InstallOutcome::Done { tag },
        Err(InstallError::Ambiguous(_, assets)) => InstallOutcome::AssetAmbiguous { assets },
        Err(InstallError::Message(message)) => InstallOutcome::Error(message),
    }
}

/// `Ok(true/false)` : disponibilité déterminée sans erreur réseau/API.
/// `direct_url`/`local` n'ont pas de notion de release à comparer --
/// toujours `false`, et jamais vérifiés en pratique (l'appelant ne retient
/// que github/gitlab dans `trackable`).
pub fn run_update_check(
    port: &Port,
    installed_tag: Option<&str>,
    installed_at: &str,
    github_token: Option<&str>,
    gitlab_token: Option<&str>,
) -> Result<bool, String> {
    let repo = port.repo.as_deref().unwrap_or_default();
    match port.source_type {
        SourceType::Github => github_api::check_update_available(repo, installed_tag, installed_at, github_token)
            .map(|(available, _, _)| available)
            .map_err(|e| format!("{}: {}", port.name, e.message())),
        SourceType::Gitlab => gitlab_api::check_update_available(repo, installed_tag, installed_at, gitlab_token)
            .map(|(available, _, _)| available)
            .map_err(|e| format!("{}: {}", port.name, e.message())),
        SourceType::DirectUrl | SourceType::Local => Ok(false),
    }
}

/// Corps du thread du bouton "Install extras" d'InfoDialog (voir
/// `app::install_launch::start_extra_install`) -- même principe que
/// `run_install`/`run_update_check` : aucune dépendance Slint, que des
/// données `Send`, `on_progress` appelé sur CE thread. `Err(message)` = lien
/// injoignable/vide ou archive illisible ; rien n'a alors été touché dans le
/// dossier du port (voir `installer::install_extra_only`).
pub fn run_extra_install(port: &Port, library_dir: &Path, on_progress: &mut dyn FnMut(&str)) -> Result<(), String> {
    installer::install_extra_only(port, library_dir, Some(on_progress)).map_err(|e| match e {
        InstallError::Message(m) => m,
        // `install_extra` extrait toujours sans ancre (voir `extract_to_staging`)
        // -- il ne peut jamais remonter d'ambiguïté d'asset.
        InstallError::Ambiguous(..) => "This \"extra\" archive has an unexpected layout.".to_string(),
    })
}
