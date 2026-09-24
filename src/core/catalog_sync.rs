
use super::{config, http};
use serde_json::Value;
use std::time::Duration;

const PORTS_RAW_URL: &str = "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/ports.json";
const THEMES_RAW_URL: &str = "https://raw.githubusercontent.com/Nyaldee/Ports-Launcher/main/themes.json";

pub enum CatalogUpdate {
    NotModified,
    Updated { text: String, etag: String },
}

fn fetch_if_changed(url: &str, known_etag: &str, validate: impl FnOnce(&str) -> Result<(), String>) -> Result<CatalogUpdate, String> {
    let agent = http::agent(Duration::from_secs(30));
    let mut req = agent.get(url);
    if !known_etag.is_empty() {
        req = req.header("If-None-Match", known_etag);
    }
    match req.call() {
        Ok(mut resp) => {
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
            validate(&text)?;
            Ok(CatalogUpdate::Updated { text, etag })
        }
        Err(ureq::Error::StatusCode(304)) => Ok(CatalogUpdate::NotModified),
        Err(e) => Err(e.to_string()),
    }
}

pub fn fetch_ports_if_changed(known_etag: &str) -> Result<CatalogUpdate, String> {
    fetch_if_changed(PORTS_RAW_URL, known_etag, |text| config::parse_catalog(text).map(|_| ()).map_err(|e| e.to_string()))
}

pub fn fetch_themes_if_changed(known_etag: &str) -> Result<CatalogUpdate, String> {
    fetch_if_changed(THEMES_RAW_URL, known_etag, |text| {
        let data: Value = serde_json::from_str(config::strip_bom(text)).map_err(|e| e.to_string())?;
        let has_themes = data.get("themes").and_then(Value::as_object).is_some_and(|o| !o.is_empty());
        if has_themes { Ok(()) } else { Err("no usable \"themes\" object".to_string()) }
    })
}
