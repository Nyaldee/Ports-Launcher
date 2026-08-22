//! Client GitHub minimal : dernière release publique d'un dépôt + choix de
//! l'asset adapté à la plateforme courante.

use super::asset_select::{pick_asset, AssetSelectionError};
use serde_json::Value;

const API_BASE: &str = "https://api.github.com";

#[derive(Debug)]
pub enum GitHubError {
    Message(String),
    Ambiguous(String, Vec<Value>),
}

impl GitHubError {
    pub fn message(&self) -> &str {
        match self {
            GitHubError::Message(m) => m,
            GitHubError::Ambiguous(m, _) => m,
        }
    }
}

/// Timeout unique et généreux : seul un blocage réellement anormal (connexion
/// qui ne répond jamais) doit être coupé, un délai large sur un appel API n'a
/// pas d'effet observable.
fn headers_agent() -> ureq::Agent {
    super::http::agent(std::time::Duration::from_secs(30))
}

/// Reprend le tableau d'un corps de réponse déjà possédé, sans le recopier
/// (contrairement à `as_array().cloned()`) -- une liste de releases porte
/// tous les assets de chacune, ce qui en fait un clone loin d'être gratuit.
fn into_array(body: Option<Value>) -> Vec<Value> {
    match body {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn get_json(url: &str, token: Option<&str>) -> Result<(u16, Option<Value>), String> {
    let mut req = headers_agent().get(url).header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    match req.call() {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            let body = resp.body_mut().read_json::<Value>().ok();
            Ok((status, body))
        }
        Err(ureq::Error::StatusCode(code)) => Ok((code, None)),
        Err(e) => Err(e.to_string()),
    }
}

pub fn get_latest_release(repo: &str, token: Option<&str>) -> Result<Value, GitHubError> {
    let url = format!("{API_BASE}/repos/{repo}/releases/latest");
    let (status, body) = get_json(&url, token).map_err(GitHubError::Message)?;
    match status {
        403 => Err(GitHubError::Message("GitHub API rate limit reached. Add a token in settings.".to_string())),
        // /releases/latest ignore les prereleases/drafts -- un projet qui
        // ne publie jamais de release "stable" obtient un 404 ici alors que
        // des releases existent bel et bien. Repli sur la liste complète.
        404 => latest_from_release_list(repo, token),
        200..=299 => body.ok_or_else(|| GitHubError::Message("Invalid GitHub response".to_string())),
        other => Err(GitHubError::Message(format!("GitHub error (HTTP {other})"))),
    }
}

fn latest_from_release_list(repo: &str, token: Option<&str>) -> Result<Value, GitHubError> {
    let url = format!("{API_BASE}/repos/{repo}/releases");
    let (status, body) = get_json(&url, token).map_err(GitHubError::Message)?;
    match status {
        403 => Err(GitHubError::Message("GitHub API rate limit reached. Add a token in settings.".to_string())),
        200..=299 => {
            // GitHub trie déjà par date de création décroissante.
            into_array(body)
                .into_iter()
                .next()
                .ok_or_else(|| GitHubError::Message(format!("No release found for {repo}.")))
        }
        other => Err(GitHubError::Message(format!("GitHub error (HTTP {other})"))),
    }
}

/// Jusqu'à `limit` releases les plus récentes (même tri que `/releases`,
/// décroissant) -- pour laisser l'utilisateur choisir une version
/// antérieure à installer (voir main.rs::open_version_picker) plutôt que
/// toujours la dernière. Inclut les prereleases comme `latest_from_release_list`
/// (mêmes raisons), jamais les drafts (invisibles sans droits push).
pub fn list_releases(repo: &str, token: Option<&str>, limit: usize) -> Result<Vec<Value>, GitHubError> {
    let url = format!("{API_BASE}/repos/{repo}/releases?per_page={limit}");
    let (status, body) = get_json(&url, token).map_err(GitHubError::Message)?;
    match status {
        403 => Err(GitHubError::Message("GitHub API rate limit reached. Add a token in settings.".to_string())),
        200..=299 => {
            let releases = into_array(body);
            if releases.is_empty() {
                Err(GitHubError::Message(format!("No release found for {repo}.")))
            } else {
                Ok(releases)
            }
        }
        other => Err(GitHubError::Message(format!("GitHub error (HTTP {other})"))),
    }
}

pub fn pick_release_asset(release: &Value, preferred: Option<&str>) -> Result<Value, GitHubError> {
    let assets = release.get("assets").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    pick_asset(assets, preferred).map_err(|e| match e {
        AssetSelectionError::Message(m) => GitHubError::Message(m),
        AssetSelectionError::Ambiguous(m, a) => GitHubError::Ambiguous(m, a),
    })
}

fn fetch_latest_tag_and_date(repo: &str, token: Option<&str>) -> Result<(String, Option<String>), GitHubError> {
    let release = get_latest_release(repo, token)?;
    let latest_tag = release.get("tag_name").and_then(Value::as_str).unwrap_or("").to_string();
    let mut latest_date = release.get("published_at").and_then(Value::as_str).map(str::to_string);
    // None -- cette date sert juste à affiner update_decision, pas à
    // télécharger : pas la peine de faire dépendre le check de mise à jour
    // de preferred_asset, qui ne concerne que l'install.
    if let Ok(asset) = pick_release_asset(&release, None) {
        if let Some(updated) = asset.get("updated_at").and_then(Value::as_str) {
            latest_date = Some(updated.to_string());
        }
    }
    Ok((latest_tag, latest_date))
}

/// Décision pure (testable sans réseau) : compare tag/date installés aux
/// tag/date les plus récents. Un tag CONNU et différent signale une mise à
/// jour de façon exacte et bon marché. Certains projets recyclent le même
/// tag ("latest") pour publier de nouveaux builds -- repli sur
/// `installed_at` (horloge locale au moment de l'install/adoption, voir
/// `InstalledInfo`), toujours connue quelle que soit la source. Ce repli
/// couvre aussi bien un tag recyclé qu'un tag totalement inconnu (port
/// adopté depuis le disque, ou `state.json` reconstruit de zéro) : un tag
/// inconnu ne signale jamais, à lui seul, une mise à jour permanente.
pub fn update_decision(installed_tag: Option<&str>, installed_at: &str, latest_tag: &str, latest_date: Option<&str>) -> bool {
    let tag_changed = matches!(installed_tag, Some(tag) if tag != latest_tag);
    let date_newer = latest_date.is_some_and(|latest| latest > installed_at);
    tag_changed || date_newer
}

pub fn check_update_available(
    repo: &str,
    installed_tag: Option<&str>,
    installed_at: &str,
    token: Option<&str>,
) -> Result<(bool, String, Option<String>), GitHubError> {
    let (latest_tag, latest_date) = fetch_latest_tag_and_date(repo, token)?;
    let available = update_decision(installed_tag, installed_at, &latest_tag, latest_date.as_deref());
    Ok((available, latest_tag, latest_date))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_identique_sans_date_plus_recente_pas_de_maj() {
        assert!(!update_decision(Some("v1.0"), "2026-02-01T00:00:00Z", "v1.0", Some("2026-01-01T00:00:00Z")));
    }

    #[test]
    fn tag_identique_avec_date_plus_recente_signale_une_maj() {
        assert!(update_decision(Some("latest"), "2026-01-01T00:00:00Z", "latest", Some("2026-02-01T00:00:00Z")));
    }

    #[test]
    fn tag_different_signale_une_maj_meme_sans_date() {
        assert!(update_decision(Some("v1.0"), "2026-01-01T00:00:00Z", "v2.0", None));
    }

    #[test]
    fn tag_identique_et_date_plus_ancienne_pas_de_maj() {
        assert!(!update_decision(Some("latest"), "2026-02-01T00:00:00Z", "latest", Some("2026-01-01T00:00:00Z")));
    }

    // Port adopté depuis le disque (voir main.rs) ou state.json reconstruit
    // de zéro -- installed_tag inconnu (voir le commentaire de
    // update_decision) : aucune nouvelle date publiée depuis l'adoption ->
    // pas de MAJ signalée en permanence, mais une vraie nouvelle
    // publication après l'adoption est bien détectée via installed_at.
    #[test]
    fn tag_inconnu_sans_date_plus_recente_pas_de_maj() {
        assert!(!update_decision(None, "2026-01-01T00:00:00Z", "v1.0", Some("2025-12-01T00:00:00Z")));
        assert!(!update_decision(None, "2026-01-01T00:00:00Z", "v1.0", None));
    }

    #[test]
    fn tag_inconnu_avec_date_plus_recente_signale_une_maj() {
        assert!(update_decision(None, "2026-01-01T00:00:00Z", "v1.0", Some("2026-02-01T00:00:00Z")));
    }
}
