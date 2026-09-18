
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

pub fn run_extra_install(port: &Port, library_dir: &Path, on_progress: &mut dyn FnMut(&str)) -> Result<(), String> {
    installer::install_extra_only(port, library_dir, Some(on_progress)).map_err(|e| match e {
        InstallError::Message(m) => m,
        InstallError::Ambiguous(..) => "This \"extra\" archive has an unexpected layout.".to_string(),
    })
}
