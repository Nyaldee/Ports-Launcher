use super::{config, http};
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_CATALOG_URL: &str = "https://ports-launcher.nyaruhodo.com/catalog.json";
pub const DEFAULT_THEMES_URL: &str = "https://ports-launcher.nyaruhodo.com/themes.json";
pub const DEFAULT_ANNOUNCEMENT_URL: &str = "https://ports-launcher.nyaruhodo.com/announcement.json";
const UPDATE_INFO_URL: &str = "https://ports-launcher.nyaruhodo.com/update.json";

const ANNOUNCEMENT_MAX_CHARS: usize = 500;

#[derive(Debug, Default, PartialEq)]
pub struct Announcement {
    pub text: String,
    pub url: String,
}

fn parse_announcement(json: &str) -> Result<Announcement, String> {
    let data: Value = serde_json::from_str(config::strip_bom(json)).map_err(|e| e.to_string())?;
    let obj = data.as_object().ok_or("the root of the announcement is not an object")?;
    let field = |key: &str| obj.get(key).and_then(Value::as_str).unwrap_or("");
    let line = field("text").lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let url = field("url").trim();
    Ok(Announcement {
        text: line.chars().filter(|c| !c.is_control()).take(ANNOUNCEMENT_MAX_CHARS).collect(),
        url: if url.to_ascii_lowercase().starts_with("https://") { url.to_string() } else { String::new() },
    })
}

pub fn fetch_announcement(url: &str) -> Result<Announcement, String> {
    match http::agent(Duration::from_secs(30)).get(url).call() {
        Ok(mut resp) => parse_announcement(&resp.body_mut().read_to_string().map_err(|e| e.to_string())?),
        Err(ureq::Error::StatusCode(404)) => Ok(Announcement::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn parse_update_date(json: &str, platform: &str) -> Result<String, String> {
    let data: Value = serde_json::from_str(config::strip_bom(json)).map_err(|e| e.to_string())?;
    let date = data.get(platform).and_then(Value::as_str).unwrap_or("").trim();
    let well_formed = date.len() == 10 && date.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
    if well_formed { Ok(date.to_string()) } else { Err(format!("no usable date for {platform}")) }
}

pub fn fetch_latest_build_date(platform: &str) -> Result<String, String> {
    let mut resp = http::agent(Duration::from_secs(30)).get(UPDATE_INFO_URL).call().map_err(|e| e.to_string())?;
    parse_update_date(&resp.body_mut().read_to_string().map_err(|e| e.to_string())?, platform)
}

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
        Ok(resp) if resp.status() == ureq::http::StatusCode::NOT_MODIFIED => Ok(CatalogUpdate::NotModified),
        Ok(mut resp) => {
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
            validate(&text)?;
            Ok(CatalogUpdate::Updated { text, etag })
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn fetch_catalog_if_changed(url: &str, known_etag: &str) -> Result<CatalogUpdate, String> {
    fetch_if_changed(url, known_etag, |text| config::parse_catalog(text).map(|_| ()).map_err(|e| e.to_string()))
}

pub fn fetch_themes_if_changed(url: &str, known_etag: &str) -> Result<CatalogUpdate, String> {
    fetch_if_changed(url, known_etag, |text| {
        let data: Value = serde_json::from_str(config::strip_bom(text)).map_err(|e| e.to_string())?;
        let has_themes = data.get("themes").and_then(Value::as_object).is_some_and(|o| !o.is_empty());
        if has_themes { Ok(()) } else { Err("no usable \"themes\" object".to_string()) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn announcement(text: &str, url: &str) -> Announcement {
        parse_announcement(&serde_json::json!({ "text": text, "url": url }).to_string()).unwrap()
    }

    #[test]
    fn l_annonce_garde_le_texte_et_le_lien() {
        assert_eq!(announcement("Hello", "https://example.com/a"), Announcement { text: "Hello".into(), url: "https://example.com/a".into() });
    }

    #[test]
    fn l_annonce_accepte_un_fichier_avec_bom_et_sans_lien() {
        assert_eq!(parse_announcement("\u{FEFF}{\"text\": \"Hello\"}").unwrap(), Announcement { text: "Hello".into(), url: String::new() });
    }

    #[test]
    fn l_annonce_garde_la_premiere_ligne_non_vide() {
        assert_eq!(announcement("\r\n\n  Hello world  \r\nSecond", "").text, "Hello world");
    }

    #[test]
    fn une_annonce_vide_ne_donne_aucun_texte() {
        assert_eq!(announcement(" \r\n\t\n", "").text, "");
        assert_eq!(parse_announcement("{}").unwrap(), Announcement::default());
    }

    #[test]
    fn un_lien_sans_https_est_ignore_mais_le_texte_reste() {
        for url in ["http://example.com", "file:///C:/a.exe", "javascript:alert(1)", "example.com"] {
            let parsed = announcement("Hello", url);
            assert_eq!(parsed.text, "Hello");
            assert_eq!(parsed.url, "", "{url}");
        }
    }

    #[test]
    fn un_json_invalide_est_une_erreur() {
        assert!(parse_announcement("{\"text\": ").is_err());
        assert!(parse_announcement("[1, 2]").is_err());
        assert!(parse_announcement("<html></html>").is_err());
    }

    #[test]
    fn update_json_donne_la_date_de_la_plateforme() {
        let json = "\u{FEFF}{\"windows\": \"2026-11-05\", \"linux\": \" 2026-11-04 \"}";
        assert_eq!(parse_update_date(json, "windows").unwrap(), "2026-11-05");
        assert_eq!(parse_update_date(json, "linux").unwrap(), "2026-11-04");
    }

    #[test]
    fn update_json_refuse_une_date_absente_ou_mal_formee() {
        for json in ["{}", "{\"windows\": \"\"}", "{\"windows\": \"2026-1-5\"}", "{\"windows\": \"z026-11-05\"}", "{\"windows\": \"2026/11/05\"}", "{\"windows\": \"2026-11-05T10:00:00Z\"}", "{\"windows\": 20261105}", "<html></html>"] {
            assert!(parse_update_date(json, "windows").is_err(), "{json}");
        }
    }

    #[test]
    fn l_annonce_retire_les_caracteres_de_controle_et_reste_bornee() {
        assert_eq!(announcement("a\tb\u{7}c", "").text, "abc");
        assert_eq!(announcement(&"é".repeat(ANNOUNCEMENT_MAX_CHARS + 50), "").text.chars().count(), ANNOUNCEMENT_MAX_CHARS);
    }
}
