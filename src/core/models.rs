
use serde_json::Value;

fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

fn parse_github_source(s: &str) -> Option<(String, String)> {
    let idx = find_ascii_ci(s, "github.com/")?;
    let mut segments = s[idx + "github.com/".len()..].splitn(3, '/');
    let owner = segments.next()?;
    let repo = segments.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn parse_gitlab_source(s: &str) -> Option<String> {
    let idx = find_ascii_ci(s, "gitlab.com/")?;
    let rest = &s[idx + "gitlab.com/".len()..];
    let path = rest.strip_suffix('/').unwrap_or(rest);
    (!path.is_empty()).then(|| path.to_string())
}

fn strip_trailing_annotation(name: &str) -> &str {
    let trimmed = name.trim_end();
    if !trimmed.ends_with(')') {
        return trimmed;
    }
    match trimmed.rfind('(') {
        Some(open) if !trimmed[open + 1..trimmed.len() - 1].contains(['(', ')']) => trimmed[..open].trim_end(),
        _ => trimmed,
    }
}

fn find_first_url(text: &str) -> Option<&str> {
    let start = [text.find("http://"), text.find("https://")].into_iter().flatten().min()?;
    let rest = &text[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"']))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Github,
    Gitlab,
    DirectUrl,
    None,
}

fn parse_source(source: &Value) -> (SourceType, Option<String>) {
    let Some(s) = source.as_str() else {
        return (SourceType::DirectUrl, None);
    };
    if let Some((owner, repo)) = parse_github_source(s) {
        let repo = repo.strip_suffix(".git").unwrap_or(&repo).to_string();
        return (SourceType::Github, Some(format!("{owner}/{repo}")));
    }
    if let Some(path) = parse_gitlab_source(s) {
        let path = path.split("/-/").next().unwrap_or(&path);
        let repo = path.strip_suffix(".git").unwrap_or(path);
        return (SourceType::Gitlab, Some(repo.to_string()));
    }
    (SourceType::DirectUrl, None)
}

#[derive(Debug, Clone)]
pub struct Port {
    pub name: String,
    pub name_lower: String,
    pub tags_lower: Vec<String>,
    pub source: Value,
    pub folder: String,
    pub executable: Option<Value>,
    pub website: Option<String>,
    pub instructions: String,
    pub mods: Option<String>,
    pub image: Option<String>,
    pub icon: Option<String>,
    pub save: Option<Value>,
    pub save2: Option<Value>,
    pub source_type: SourceType,
    pub repo: Option<String>,
    pub exe_is_archive: Option<String>,
    pub preferred_asset: Option<Value>,
    pub extra: Option<String>,
    pub user_managed: bool,
}

impl Port {
    pub fn website_url(&self) -> Option<&str> {
        if let Some(w) = &self.website {
            return Some(w);
        }
        match self.source_type {
            SourceType::Github | SourceType::Gitlab => self.source.as_str(),
            SourceType::DirectUrl | SourceType::None => None,
        }
    }

    pub fn key(&self) -> &str {
        self.repo.as_deref().unwrap_or(&self.folder)
    }

    pub fn instructions_link(&self) -> Option<&str> {
        find_first_url(&self.instructions)
    }

    pub fn display_name(&self) -> &str {
        strip_trailing_annotation(&self.name)
    }

    pub fn is_android_only(&self) -> bool {
        self.tags_lower.iter().any(|t| t == "android_only")
    }
}

#[derive(Debug)]
pub struct PortParseError;

pub fn port_from_value(d: &Value) -> Result<Port, PortParseError> {
    let obj = d.as_object().ok_or(PortParseError)?;
    let string = |key: &str| obj.get(key).and_then(Value::as_str).map(str::to_string);

    let name = string("name").ok_or(PortParseError)?;
    let folder = string("folder").ok_or(PortParseError)?;
    let (source_type, repo, source) = match obj.get("source") {
        None => (SourceType::None, None, Value::Null),
        Some(v) if v.is_string() || v.is_object() => {
            let (source_type, repo) = parse_source(v);
            (source_type, repo, v.clone())
        }
        Some(_) => return Err(PortParseError),
    };
    let tags_lower = obj
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_lowercase).collect())
        .unwrap_or_default();

    Ok(Port {
        name_lower: name.to_lowercase(),
        name,
        tags_lower,
        source,
        folder,
        executable: obj.get("executable").cloned(),
        website: string("website"),
        instructions: string("instructions").unwrap_or_default(),
        mods: string("mods"),
        image: string("image"),
        icon: string("icon"),
        save: obj.get("save").cloned(),
        save2: obj.get("save2").cloned(),
        source_type,
        repo,
        exe_is_archive: string("exe_is_archive"),
        preferred_asset: obj.get("preferred_asset").cloned(),
        extra: string("extra"),
        user_managed: false,
    })
}

#[derive(Debug, Clone)]
pub struct InstalledInfo {
    pub installed_tag: Option<String>,
    pub installed_at: String,
    pub favorite_exe: Option<String>,
    pub update: bool,
    pub playtime_seconds: u64,
    pub last_played_at: String,
}

impl Default for InstalledInfo {
    fn default() -> Self {
        InstalledInfo {
            installed_tag: None,
            installed_at: String::new(),
            favorite_exe: None,
            update: true,
            playtime_seconds: 0,
            last_played_at: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_github_avec_et_sans_git() {
        let (t, r) = parse_source(&json!("https://github.com/owner/repo"));
        assert_eq!(t, SourceType::Github);
        assert_eq!(r.as_deref(), Some("owner/repo"));
        let (t, r) = parse_source(&json!("https://github.com/owner/repo.git"));
        assert_eq!(t, SourceType::Github);
        assert_eq!(r.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn source_github_insensible_a_la_casse_meme_avec_un_prefixe_unicode_qui_change_de_longueur_en_minuscule() {
        let (t, r) = parse_source(&json!("İ https://GITHUB.com/owner/repo"));
        assert_eq!(t, SourceType::Github);
        assert_eq!(r.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn source_gitlab_coupe_les_routes_ui() {
        let (t, r) = parse_source(&json!("https://gitlab.com/group/project"));
        assert_eq!(t, SourceType::Gitlab);
        assert_eq!(r.as_deref(), Some("group/project"));
        let (t, r) = parse_source(&json!("https://gitlab.com/group/project/-/releases"));
        assert_eq!(t, SourceType::Gitlab);
        assert_eq!(r.as_deref(), Some("group/project"));
    }

    #[test]
    fn find_first_url_extrait_depuis_du_texte_libre() {
        assert_eq!(find_first_url("Suivre le guide ici : https://example.com/guide avant d'installer."), Some("https://example.com/guide"));
        assert_eq!(find_first_url("Aucune URL ici."), None);
    }

    #[test]
    fn find_first_url_retire_la_ponctuation_de_fin_de_phrase() {
        assert_eq!(find_first_url("Voir (https://example.com/guide)."), Some("https://example.com/guide"));
        assert_eq!(find_first_url("Lien: https://example.com/guide, puis continuer."), Some("https://example.com/guide"));
    }

    #[test]
    fn find_first_url_garde_la_premiere_quand_il_y_en_a_plusieurs() {
        assert_eq!(find_first_url("https://a.example.com puis https://b.example.com"), Some("https://a.example.com"));
    }

    #[test]
    fn source_dict_est_toujours_direct_url() {
        let (t, r) = parse_source(&json!({"windows": "https://github.com/o/p"}));
        assert_eq!(t, SourceType::DirectUrl);
        assert_eq!(r, None);
    }

    #[test]
    fn source_url_quelconque_est_direct_url() {
        let (t, r) = parse_source(&json!("https://example.com/file.zip"));
        assert_eq!(t, SourceType::DirectUrl);
        assert_eq!(r, None);
    }

    #[test]
    fn from_value_rejette_name_manquant_ou_invalide() {
        assert!(port_from_value(&json!({"folder": "f", "source": "s"})).is_err());
        assert!(port_from_value(&json!({"name": 5, "folder": "f", "source": "s"})).is_err());
    }

    #[test]
    fn from_value_rejette_folder_manquant() {
        assert!(port_from_value(&json!({"name": "n", "source": "s"})).is_err());
    }

    #[test]
    fn from_value_source_manquante_donne_source_type_none() {
        let p = port_from_value(&json!({"name": "n", "folder": "f"})).unwrap();
        assert_eq!(p.source_type, SourceType::None);
        assert_eq!(p.repo, None);
    }

    #[test]
    fn from_value_rejette_source_mal_typee() {
        assert!(port_from_value(&json!({"name": "n", "folder": "f", "source": 5})).is_err());
    }

    #[test]
    fn from_value_tolere_champs_optionnels_mal_types() {
        let p = port_from_value(&json!({
            "name": "n", "folder": "f", "source": "s",
            "tags": "not-a-list", "website": 5, "mods": [], "image": {}
        }))
        .unwrap();
        assert!(p.tags_lower.is_empty());
        assert_eq!(p.website, None);
        assert_eq!(p.mods, None);
        assert_eq!(p.image, None);
    }

    #[test]
    fn from_value_filtre_les_tags_non_chaine() {
        let p = port_from_value(&json!({
            "name": "n", "folder": "f", "source": "s", "tags": ["A", 5, "b", null]
        }))
        .unwrap();
        assert_eq!(p.tags_lower, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn key_utilise_repo_sinon_folder() {
        let p = port_from_value(&json!({"name": "n", "folder": "f", "source": "https://github.com/o/p"})).unwrap();
        assert_eq!(p.key(), "o/p");
        let p = port_from_value(&json!({"name": "n", "folder": "f", "source": "https://example.com/x.zip"})).unwrap();
        assert_eq!(p.key(), "f");
    }

    #[test]
    fn display_name_retire_l_annotation_finale() {
        let p = port_from_value(&json!({"name": "Game (fork-name)", "folder": "f"})).unwrap();
        assert_eq!(p.display_name(), "Game");
        let p = port_from_value(&json!({"name": "Game (a (b))", "folder": "f"})).unwrap();
        assert_eq!(p.display_name(), "Game (a (b))");
    }
}
