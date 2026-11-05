use super::asset_select::{self, AssetSelectionError, RECENT_RELEASES_DEPTH};
use super::models::SourceType;
use super::{github_api, gitlab_api};
use serde_json::Value;

#[derive(Clone, Copy)]
pub enum Forge {
    Github,
    Gitlab,
}

impl Forge {
    pub fn of(source_type: SourceType) -> Option<Forge> {
        match source_type {
            SourceType::Github => Some(Forge::Github),
            SourceType::Gitlab => Some(Forge::Gitlab),
            SourceType::DirectUrl | SourceType::None => None,
        }
    }

    pub fn token<'a>(self, github_token: Option<&'a str>, gitlab_token: Option<&'a str>) -> Option<&'a str> {
        match self {
            Forge::Github => github_token,
            Forge::Gitlab => gitlab_token,
        }
    }

    pub fn list_releases(self, repo: &str, token: Option<&str>, limit: usize) -> Result<Vec<Value>, AssetSelectionError> {
        match self {
            Forge::Github => github_api::list_releases(repo, token, limit),
            Forge::Gitlab => gitlab_api::list_releases(repo, token, limit),
        }
    }

    pub fn pick_release_asset(self, release: &Value, preferred: Option<&str>) -> Result<Value, AssetSelectionError> {
        match self {
            Forge::Github => github_api::pick_release_asset(release, preferred),
            Forge::Gitlab => gitlab_api::pick_release_asset(release, preferred),
        }
    }

    pub fn url_field(self) -> &'static str {
        match self {
            Forge::Github => "browser_download_url",
            Forge::Gitlab => "url",
        }
    }

    fn release_date(self, release: &Value, asset: Option<&Value>) -> Option<String> {
        let date = match self {
            Forge::Github => asset.and_then(|a| a.get("updated_at")).or_else(|| release.get("published_at")),
            Forge::Gitlab => release.get("released_at").or_else(|| release.get("created_at")),
        };
        date.and_then(Value::as_str).map(str::to_string)
    }

    pub fn most_recent_release(self, repo: &str, token: Option<&str>) -> Result<Value, AssetSelectionError> {
        self.list_releases(repo, token, 1)?
            .into_iter()
            .next()
            .ok_or_else(|| AssetSelectionError::Message(format!("No release found for {repo}.")))
    }

    pub fn latest_installable_release(self, repo: &str, token: Option<&str>, preferred: Option<&str>) -> Result<(Value, Value), AssetSelectionError> {
        let releases = self.list_releases(repo, token, RECENT_RELEASES_DEPTH)?;
        asset_select::latest_installable_release(releases, preferred, |release, preferred| self.pick_release_asset(release, preferred))
    }

    pub fn fetch_latest_tag_and_date(self, repo: &str, token: Option<&str>, preferred: Option<&str>) -> Result<(String, Option<String>), AssetSelectionError> {
        let releases = self.list_releases(repo, token, RECENT_RELEASES_DEPTH)?;
        let (release, asset) = asset_select::release_for_update_check(releases, preferred, |release, preferred| self.pick_release_asset(release, preferred))?;
        let latest_tag = release.get("tag_name").and_then(Value::as_str).unwrap_or("").to_string();
        Ok((latest_tag, self.release_date(&release, asset.as_ref())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn date_github_prefere_celle_de_l_asset() {
        let release = json!({"published_at": "2026-01-01T00:00:00Z"});
        let asset = json!({"updated_at": "2026-02-01T00:00:00Z"});
        assert_eq!(Forge::Github.release_date(&release, Some(&asset)).as_deref(), Some("2026-02-01T00:00:00Z"));
        assert_eq!(Forge::Github.release_date(&release, None).as_deref(), Some("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn date_gitlab_ignore_l_asset() {
        let release = json!({"created_at": "2026-01-01T00:00:00Z", "released_at": "2026-01-05T00:00:00Z"});
        let asset = json!({"updated_at": "2026-02-01T00:00:00Z"});
        assert_eq!(Forge::Gitlab.release_date(&release, Some(&asset)).as_deref(), Some("2026-01-05T00:00:00Z"));
    }

    #[test]
    fn forge_et_jeton_selon_la_source() {
        assert!(Forge::of(SourceType::DirectUrl).is_none());
        assert!(Forge::of(SourceType::None).is_none());
        assert_eq!(Forge::of(SourceType::Gitlab).map(|f| f.token(Some("gh"), Some("gl"))), Some(Some("gl")));
        assert_eq!(Forge::of(SourceType::Github).map(|f| f.token(Some("gh"), Some("gl"))), Some(Some("gh")));
    }
}
