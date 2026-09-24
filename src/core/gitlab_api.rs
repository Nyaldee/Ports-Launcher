
use super::asset_select::{self, pick_asset, AssetSelectionError, RECENT_RELEASES_DEPTH};
use super::http::api_agent;
use serde_json::Value;

const API_BASE: &str = "https://gitlab.com/api/v4";

pub fn list_releases(project_path: &str, token: Option<&str>, limit: usize) -> Result<Vec<Value>, AssetSelectionError> {
    let url = format!("{API_BASE}/projects/{}/releases?per_page={limit}", urlencode(project_path));
    let mut req = api_agent().get(&url);
    if let Some(t) = token {
        req = req.header("PRIVATE-TOKEN", t);
    }
    let releases = match req.call() {
        Ok(mut resp) => match resp.body_mut().read_json::<Value>() {
            Ok(Value::Array(items)) => items,
            _ => Vec::new(),
        },
        Err(ureq::Error::StatusCode(404)) => {
            return Err(AssetSelectionError::Message(format!("No project or release found for {project_path}.")))
        }
        Err(e) => return Err(AssetSelectionError::Message(e.to_string())),
    };
    if releases.is_empty() {
        return Err(AssetSelectionError::Message(format!("No release found for {project_path}.")));
    }
    Ok(releases)
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
        .pointer("/assets/links")
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

pub fn pick_release_asset(release: &Value, preferred: Option<&str>) -> Result<Value, AssetSelectionError> {
    pick_asset(&release_assets(release), preferred)
}

pub fn most_recent_release(project_path: &str, token: Option<&str>) -> Result<Value, AssetSelectionError> {
    list_releases(project_path, token, 1)?
        .into_iter()
        .next()
        .ok_or_else(|| AssetSelectionError::Message(format!("No release found for {project_path}.")))
}

pub fn latest_installable_release(project_path: &str, token: Option<&str>, preferred: Option<&str>) -> Result<(Value, Value), AssetSelectionError> {
    let releases = list_releases(project_path, token, RECENT_RELEASES_DEPTH)?;
    asset_select::latest_installable_release(releases, preferred, pick_release_asset)
}

pub fn fetch_latest_tag_and_date(project_path: &str, token: Option<&str>) -> Result<(String, Option<String>), AssetSelectionError> {
    let (release, _) = latest_installable_release(project_path, token, None)?;
    let latest_tag = release.get("tag_name").and_then(Value::as_str).unwrap_or("").to_string();
    let latest_date = release
        .get("released_at")
        .or_else(|| release.get("created_at"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((latest_tag, latest_date))
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
        assert_eq!(release_assets(&release), vec![serde_json::json!({"name": "a.zip", "url": "https://x/a.zip"})]);
    }

    #[test]
    fn release_assets_vide_si_absent() {
        assert!(release_assets(&serde_json::json!({})).is_empty());
    }
}
