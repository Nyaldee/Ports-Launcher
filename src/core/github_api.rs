use super::asset_select::{pick_asset, AssetSelectionError};
use super::http::api_agent;
use serde_json::Value;

const API_BASE: &str = "https://api.github.com";

pub fn list_releases(repo: &str, token: Option<&str>, limit: usize) -> Result<Vec<Value>, AssetSelectionError> {
    let url = format!("{API_BASE}/repos/{repo}/releases?per_page={limit}");
    let mut req = api_agent().get(&url).header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let releases = match req.call() {
        Ok(mut resp) => match resp.body_mut().read_json::<Value>() {
            Ok(Value::Array(items)) => items,
            _ => Vec::new(),
        },
        Err(ureq::Error::StatusCode(403 | 429)) => {
            return Err(AssetSelectionError::Message("GitHub API rate limit reached. Add a token in state.json.".to_string()))
        }
        Err(ureq::Error::StatusCode(code)) => return Err(AssetSelectionError::Message(format!("GitHub error (HTTP {code})"))),
        Err(e) => return Err(AssetSelectionError::Message(e.to_string())),
    };
    if releases.is_empty() {
        return Err(AssetSelectionError::Message(format!("No release found for {repo}.")));
    }
    Ok(releases)
}

pub fn pick_release_asset(release: &Value, preferred: Option<&str>) -> Result<Value, AssetSelectionError> {
    let assets = release.get("assets").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    pick_asset(assets, preferred)
}
