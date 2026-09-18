
use super::asset_select::{pick_asset, AssetSelectionError};
use super::http::api_agent;
use serde_json::Value;

const API_BASE: &str = "https://gitlab.com/api/v4";

pub type GitLabError = AssetSelectionError;

pub fn get_latest_release(project_path: &str, token: Option<&str>) -> Result<Value, GitLabError> {
    list_releases(project_path, token, 1)?.into_iter().next().ok_or_else(|| GitLabError::Message(format!("No release found for {project_path}.")))
}

pub fn list_releases(project_path: &str, token: Option<&str>, limit: usize) -> Result<Vec<Value>, GitLabError> {
    let encoded = urlencode(project_path);
    let url = format!("{API_BASE}/projects/{encoded}/releases?per_page={limit}");
    let mut req = api_agent().get(&url);
    if let Some(t) = token {
        req = req.header("PRIVATE-TOKEN", t);
    }
    match req.call() {
        Ok(mut resp) => {
            let releases = match resp.body_mut().read_json::<Value>() {
                Ok(Value::Array(items)) => items,
                _ => Vec::new(),
            };
            if releases.is_empty() {
                Err(GitLabError::Message(format!("No release found for {project_path}.")))
            } else {
                Ok(releases)
            }
        }
        Err(ureq::Error::StatusCode(404)) => {
            Err(GitLabError::Message(format!("No project or release found for {project_path}.")))
        }
        Err(e) => Err(GitLabError::Message(e.to_string())),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn release_assets(release: &Value) -> Vec<Value> {
    release
        .get("assets")
        .and_then(|a| a.get("links"))
        .and_then(Value::as_array)
        .map(|links| {
            links
                .iter()
                .filter_map(|link| {
                    let name = link.get("name")?.as_str()?;
                    let url = link.get("url")?.as_str()?;
                    Some(serde_json::json!({"name": name, "url": url}))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn pick_release_asset(release: &Value, preferred: Option<&str>) -> Result<Value, GitLabError> {
    let assets = release_assets(release);
    pick_asset(&assets, preferred)
}

pub fn check_update_available(
    project_path: &str,
    installed_tag: Option<&str>,
    installed_at: &str,
    token: Option<&str>,
) -> Result<(bool, String, Option<String>), GitLabError> {
    let release = get_latest_release(project_path, token)?;
    let latest_tag = release.get("tag_name").and_then(Value::as_str).unwrap_or("").to_string();
    let latest_date = release
        .get("released_at")
        .and_then(Value::as_str)
        .or_else(|| release.get("created_at").and_then(Value::as_str))
        .map(str::to_string);

    let available = super::github_api::update_decision(installed_tag, installed_at, &latest_tag, latest_date.as_deref());
    Ok((available, latest_tag, latest_date))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_encode_les_slashs() {
        assert_eq!(urlencode("group/project"), "group%2Fproject");
    }

    #[test]
    fn release_assets_ne_garde_que_name_et_url() {
        let release = serde_json::json!({
            "assets": {"links": [{"name": "a.zip", "url": "https://x/a.zip", "id": 1}]}
        });
        let assets = release_assets(&release);
        assert_eq!(assets, vec![serde_json::json!({"name": "a.zip", "url": "https://x/a.zip"})]);
    }

    #[test]
    fn release_assets_vide_si_absent() {
        assert!(release_assets(&serde_json::json!({})).is_empty());
    }
}
