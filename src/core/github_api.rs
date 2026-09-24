
use super::asset_select::{self, pick_asset, AssetSelectionError, RECENT_RELEASES_DEPTH};
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

pub fn most_recent_release(repo: &str, token: Option<&str>) -> Result<Value, AssetSelectionError> {
    list_releases(repo, token, 1)?.into_iter().next().ok_or_else(|| AssetSelectionError::Message(format!("No release found for {repo}.")))
}

pub fn latest_installable_release(repo: &str, token: Option<&str>, preferred: Option<&str>) -> Result<(Value, Value), AssetSelectionError> {
    let releases = list_releases(repo, token, RECENT_RELEASES_DEPTH)?;
    asset_select::latest_installable_release(releases, preferred, pick_release_asset)
}

pub fn fetch_latest_tag_and_date(repo: &str, token: Option<&str>) -> Result<(String, Option<String>), AssetSelectionError> {
    let (release, asset) = latest_installable_release(repo, token, None)?;
    let latest_tag = release.get("tag_name").and_then(Value::as_str).unwrap_or("").to_string();
    let latest_date = asset
        .get("updated_at")
        .or_else(|| release.get("published_at"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((latest_tag, latest_date))
}
