
use super::installer::{self, InstallError, InstallOverrides, InstallPaths};
use super::models::{Port, SourceType};
use super::{github_api, gitlab_api};
use serde_json::Value;
use std::path::Path;

pub enum InstallOutcome {
    Done { tag: Option<String> },
    AssetAmbiguous { assets: Vec<Value> },
    Error(String),
}

pub fn run_install(
    port: &Port,
    paths: InstallPaths,
    github_token: Option<&str>,
    gitlab_token: Option<&str>,
    overrides: InstallOverrides,
    on_progress: &mut dyn FnMut(&str),
) -> InstallOutcome {
    match installer::install_port(port, paths, github_token, gitlab_token, overrides, on_progress) {
        Ok(tag) => InstallOutcome::Done { tag },
        Err(InstallError::Ambiguous(assets)) => InstallOutcome::AssetAmbiguous { assets },
        Err(InstallError::Message(message)) => InstallOutcome::Error(message),
    }
}

pub fn update_decision(installed_tag: Option<&str>, installed_at: &str, latest_tag: &str, latest_date: Option<&str>) -> bool {
    let tag_changed = installed_tag.is_some_and(|tag| tag != latest_tag);
    let date_newer = latest_date.is_some_and(|latest| latest > installed_at);
    tag_changed || date_newer
}

pub fn run_update_check(
    port: &Port,
    installed_tag: Option<&str>,
    installed_at: &str,
    github_token: Option<&str>,
    gitlab_token: Option<&str>,
) -> bool {
    let repo = port.repo.as_deref().unwrap_or_default();
    let latest = match port.source_type {
        SourceType::Github => github_api::fetch_latest_tag_and_date(repo, github_token),
        SourceType::Gitlab => gitlab_api::fetch_latest_tag_and_date(repo, gitlab_token),
        SourceType::DirectUrl | SourceType::None => return false,
    };
    latest.is_ok_and(|(tag, date)| update_decision(installed_tag, installed_at, &tag, date.as_deref()))
}

pub fn run_extra_install(port: &Port, library_dir: &Path, on_progress: &mut dyn FnMut(&str)) -> Result<(), String> {
    installer::install_extra(port, library_dir, on_progress).map_err(|e| match e {
        InstallError::Message(m) => m,
        InstallError::Ambiguous(_) => "This \"extra\" archive has an unexpected layout.".to_string(),
    })
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
    fn tag_inconnu_sans_date_plus_recente_pas_de_maj() {
        assert!(!update_decision(None, "2026-01-01T00:00:00Z", "v1.0", Some("2025-12-01T00:00:00Z")));
        assert!(!update_decision(None, "2026-01-01T00:00:00Z", "v1.0", None));
    }

    #[test]
    fn tag_inconnu_avec_date_plus_recente_signale_une_maj() {
        assert!(update_decision(None, "2026-01-01T00:00:00Z", "v1.0", Some("2026-02-01T00:00:00Z")));
    }
}
