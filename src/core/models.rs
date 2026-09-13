//! Modèle de données du catalogue (`ports.json`) -- validation tolérante
//! champ par champ (un champ optionnel mal typé retombe silencieusement à
//! son défaut, un champ requis manquant/mal typé rejette l'entrée entière,
//! jamais un plantage plus loin sur un `.to_lowercase()`/chemin invalide).

use serde_json::Value;

/// "github.com/owner/repo..." n'importe où dans l'URL (insensible à la
/// casse) -- `owner`/`repo` sont les deux premiers segments de chemin après
/// le nom d'hôte. Comparaison de sous-chaînes plutôt qu'une `regex` : le
/// motif recherché est une simple séquence de segments de chemin.
fn parse_github_source(s: &str) -> Option<(String, String)> {
    let idx = s.to_lowercase().find("github.com/")?;
    let rest = &s[idx + "github.com/".len()..];
    let mut segments = rest.splitn(3, '/');
    let owner = segments.next()?;
    let repo = segments.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// "gitlab.com/..." jusqu'à la fin de l'URL (n'importe où dedans, insensible
/// à la casse), moins un éventuel "/" final unique.
fn parse_gitlab_source(s: &str) -> Option<String> {
    let idx = s.to_lowercase().find("gitlab.com/")?;
    let rest = &s[idx + "gitlab.com/".len()..];
    let path = rest.strip_suffix('/').unwrap_or(rest);
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Première URL http(s) trouvée dans un texte libre (voir `instructions_link`)
/// -- délimitée par les espaces ; la ponctuation de fin de phrase la plus
/// courante collée juste après (`.,;:!?)]}'"`) est retirée pour ne pas
/// inclure la fin d'une phrase dans le lien.
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
    /// Pas de "source" du tout -- entrée d'un catalogue LOCAL (voir
    /// `config::load_local_config`), où l'utilisateur a déjà placé les
    /// fichiers du jeu lui-même dans son propre dossier de `Library`, rien
    /// à télécharger. `start_install` affiche un message clair plutôt que
    /// de tenter un téléchargement sur une source absente.
    Local,
}

/// Déduit (type, repo) à partir du champ "source" : un lien github/gitlab
/// (page du dépôt) donne le type et le "owner/projet" associé ; toute autre
/// URL (ou un objet multi-plateforme) est un lien de téléchargement direct.
fn parse_source(source: &Value) -> (SourceType, Option<String>) {
    let Some(s) = source.as_str() else {
        return (SourceType::DirectUrl, None);
    };
    if let Some((owner, repo)) = parse_github_source(s) {
        let repo = repo.strip_suffix(".git").unwrap_or(&repo).to_string();
        return (SourceType::Github, Some(format!("{owner}/{repo}")));
    }
    if let Some(path) = parse_gitlab_source(s) {
        // Les routes UI GitLab (page Releases, un fichier/une branche...)
        // utilisent toutes "/-/" comme séparateur avant la partie qui n'est
        // PAS le chemin du projet -- couper là plutôt que de laisser ça
        // s'agglutiner dans le nom du projet (un sous-groupe imbriqué
        // légitime, lui, n'a jamais "/-/" dans son propre chemin, donc ce
        // split est un no-op pour ce cas).
        let path = path.split("/-/").next().unwrap_or(&path);
        let repo = path.strip_suffix(".git").unwrap_or(path);
        return (SourceType::Gitlab, Some(repo.to_string()));
    }
    (SourceType::DirectUrl, None)
}

#[derive(Debug, Clone)]
pub struct Port {
    pub name: String,
    /// Minuscule, calculée une seule fois ici plutôt qu'à chaque frappe
    /// dans la barre de recherche (voir `search::search_tier`) -- `name`
    /// ne change jamais après le chargement du catalogue, donc rien à
    /// invalider : la valeur reste correcte pour toute la durée de vie de
    /// ce `Port`.
    pub name_lower: String,
    /// Casse d'origine de `ports.json` (voir `tags_lower` pour la recherche)
    /// -- réservée pour un affichage des tags dans l'UI, seule la version en
    /// minuscule est utilisée pour l'instant.
    #[allow(dead_code)]
    pub tags: Vec<String>,
    /// Voir `name_lower` -- même raison, une entrée par tag.
    pub tags_lower: Vec<String>,
    /// Chaîne (lien github/gitlab/direct) ou objet multi-plateforme
    /// `{"windows": ..., "linux": ...}` -- voir `platform_resolve::resolve_per_platform`.
    pub source: Value,
    pub folder: String,
    /// `None` -> auto-détection du seul exécutable trouvé dans le dossier installé.
    pub executable: Option<Value>,
    pub website: Option<String>,
    pub instructions: String,
    pub mods: Option<String>,
    pub image: Option<String>,
    pub save: Option<Value>,
    /// Second emplacement de sauvegarde optionnel -- pour un jeu dont la
    /// version portable ET la version "normale" ont chacune leur propre
    /// dossier de save (voir `save_backup::preserve_before_uninstall`/
    /// `restore_after_install`, sauvegardés dans des sous-dossiers SÉPARÉS
    /// de `Saves Backup/Pending Restore` pour ne jamais écraser l'un avec
    /// l'autre même si les deux contiennent des fichiers de même nom).
    pub save2: Option<Value>,
    pub source_type: SourceType,
    pub repo: Option<String>,
    /// `None` -> comportement normal (défaut, immense majorité des ports).
    /// `Some(name)` -> l'archive (n'importe quel format connu, voir
    /// `installer::extract`) contient plus que le jeu lui-même à sa racine
    /// (ex: un installeur NSIS, un zip déguisé en `.exe`) -- `name` est le
    /// nom (sous-chaîne, n'importe quelle extension, pas nécessairement un
    /// `.exe`) d'un fichier qui sert d'ANCRE : `installer::find_exe_folder`
    /// localise son dossier conteneur et NE GARDE QUE celui-ci (avec tout
    /// ce qu'il contient), écartant le reste (prérequis, outils, autres
    /// dossiers annexes). Toujours un nom explicite depuis que l'ancienne
    /// auto-détection (choisir seule quand un seul dossier contient un
    /// `.exe`) a été retirée -- trop ambiguë dès que plusieurs `.exe`
    /// existent dans des dossiers différents.
    pub exe_is_archive: Option<String>,
    /// Sous-chaîne du nom de l'asset à télécharger en priorité (voir
    /// `platform_resolve::resolve_preferred_asset`, `asset_select::pick_asset`)
    /// -- pour les releases où rien ne mentionne l'OS dans le nom, où
    /// l'heuristique générique ne peut pas deviner. Per-plateforme comme
    /// `executable`/`save` (`{"windows": ..., "linux": ...}` ou une simple
    /// chaîne). `None` -> heuristique standard, inchangée.
    pub preferred_asset: Option<Value>,
    /// URL d'un fichier ou d'une archive téléchargé(e) APRÈS l'install
    /// principale et fusionné(e) dans le dossier du port -- les fichiers de
    /// même nom écrasent ceux déjà posés par `source` (voir
    /// `installer::install_extra`). Ignoré silencieusement en cas d'échec
    /// (lien mort, hors-ligne...) : n'empêche jamais l'install principale de
    /// réussir. Pour un contenu additionnel qui ne fait pas partie des
    /// releases officielles du jeu (scripts de lancement, correctifs...).
    pub extra: Option<String>,
}

impl Port {
    /// Page d'accueil affichable : `website` explicite, sinon l'URL source
    /// elle-même si c'est une page github/gitlab navigable, sinon rien
    /// (un lien de téléchargement direct n'est pas une page à visiter).
    pub fn website_url(&self) -> Option<&str> {
        if let Some(w) = &self.website {
            return Some(w);
        }
        match self.source_type {
            SourceType::Github | SourceType::Gitlab => self.source.as_str(),
            SourceType::DirectUrl | SourceType::Local => None,
        }
    }

    /// Clé de suivi dans `state.json` : le repo (identique entre deux
    /// installs) si connu, sinon le nom de dossier (repli pour une source
    /// `direct_url`, qui n'a pas de notion de dépôt).
    pub fn key(&self) -> &str {
        self.repo.as_deref().unwrap_or(&self.folder)
    }

    /// Première URL trouvée dans `instructions`, si `Info` doit proposer un
    /// lien cliquable en plus du texte (voir dialogs.slint) -- `None` si le
    /// champ n'en contient aucune, auquel cas rien n'est affiché.
    pub fn instructions_link(&self) -> Option<&str> {
        find_first_url(&self.instructions)
    }
}

/// `config::load_config` ignore silencieusement toute entrée qui échoue ici
/// (voir son docstring) -- le message n'est donc lu par aucun appelant
/// actuel, gardé pour le `Debug` (diagnostic) et une éventuelle
/// journalisation future.
#[derive(Debug)]
pub struct PortParseError(#[allow(dead_code)] pub String);

/// Construit un `Port` depuis une entrée décodée de `ports.json`/
/// `ports.local.json`. `name`/`folder` requis et strictement typés
/// (erreur -> l'appelant rejette cette seule entrée, voir
/// `config::load_config`/`load_local_config`) ; `source`, lui, est
/// OPTIONNEL -- absent, l'entrée devient `SourceType::Local` (voir sa
/// docstring), présent il reste strictement typé (chaîne ou objet). Tous
/// les autres champs optionnels retombent silencieusement à leur défaut si
/// absents/mal typés plutôt que de propager une erreur pour une donnée non
/// critique.
pub fn port_from_value(d: &Value) -> Result<Port, PortParseError> {
    let obj = d.as_object().ok_or_else(|| PortParseError("entry is not an object".to_string()))?;

    let tags: Vec<String> = obj
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| PortParseError("\"name\" missing or invalid".to_string()))?
        .to_string();
    let folder = obj
        .get("folder")
        .and_then(Value::as_str)
        .ok_or_else(|| PortParseError("\"folder\" missing or invalid".to_string()))?
        .to_string();
    // Absente -- entrée d'un catalogue LOCAL (voir SourceType::Local), les
    // fichiers du jeu sont déjà placés à la main par l'utilisateur, rien à
    // télécharger. Présente mais mal typée : une vraie erreur de saisie, pas
    // une absence volontaire, donc rejetée.
    let source = match obj.get("source") {
        None => None,
        Some(v) if v.is_string() || v.is_object() => Some(v.clone()),
        Some(_) => return Err(PortParseError("\"source\" must be a string or an object".to_string())),
    };

    let website = obj.get("website").and_then(Value::as_str).map(str::to_string);
    let instructions = obj.get("instructions").and_then(Value::as_str).unwrap_or("").to_string();
    let mods = obj.get("mods").and_then(Value::as_str).map(str::to_string);
    let image = obj.get("image").and_then(Value::as_str).map(str::to_string);
    let executable = obj.get("executable").cloned();
    let save = obj.get("save").cloned();
    let save2 = obj.get("save2").cloned();
    let exe_is_archive = obj.get("exe_is_archive").and_then(Value::as_str).map(str::to_string);
    let preferred_asset = obj.get("preferred_asset").cloned();
    let extra = obj.get("extra").and_then(Value::as_str).map(str::to_string);

    let (source_type, repo, source) = match source {
        Some(source) => {
            let (source_type, repo) = parse_source(&source);
            (source_type, repo, source)
        }
        None => (SourceType::Local, None, Value::Null),
    };

    let name_lower = name.to_lowercase();
    let tags_lower = tags.iter().map(|t| t.to_lowercase()).collect();

    Ok(Port {
        name,
        name_lower,
        tags,
        tags_lower,
        source,
        folder,
        executable,
        website,
        instructions,
        mods,
        image,
        save,
        save2,
        source_type,
        repo,
        exe_is_archive,
        preferred_asset,
        extra,
    })
}

#[derive(Debug, Clone)]
pub struct InstalledInfo {
    pub installed_tag: Option<String>,
    /// Horloge locale au moment de l'install/adoption (voir
    /// `StateManager::mark_installed`) -- sert AUSSI de repli pour
    /// `check_update_available` quand le tag seul ne suffit pas (projet qui
    /// recycle toujours le même tag "latest") : toujours connue, alors qu'une
    /// date de release/asset est absente pour certaines sources.
    pub installed_at: String,
    /// Chemin de l'exécutable choisi explicitement par l'utilisateur pour ce
    /// port (voir le bouton sous "Select version" dans InfoDialog) -- RELATIF
    /// au dossier du jeu (`game_dir` dans `launch_flow`/
    /// `open_favorite_exe_picker`), jamais absolu : un chemin absolu
    /// casserait silencieusement tous les favoris déjà choisis dès que
    /// `library_dir` ou le dossier de Ports Launcher est déplacé.
    /// Si présent, `launch_flow` le rejoint à `game_dir` et le lance
    /// directement sans repasser par `resolve_executable`/le picker
    /// d'ambiguïté. `None` (défaut, ou après "Ask every time") redemande à
    /// chaque Play.
    pub favorite_exe: Option<String>,
    /// Auto-MAJ activée pour CE port (voir le bouton "Update" d'InfoDialog,
    /// StateManager::set_port_update) -- true par défaut. Si false,
    /// `launch_with_update_check` ne tape jamais l'API à ce Play : ce port
    /// n'est alors plus JAMAIS vérifié par l'appli (voir
    /// PortItem.auto-update-off côté main.rs::to_port_items, purement l'état
    /// de ce bouton, aucune requête réseau derrière).
    pub update: bool,
    /// Temps de jeu cumulé, en secondes -- voir main.rs::record_playtime,
    /// alimenté à chaque fois qu'un process lancé pour ce port est détecté
    /// terminé. Préservé par `mark_installed` comme `favorite_exe`/`update`
    /// (une MAJ ne doit jamais remettre ce compteur à zéro) ; seul le
    /// bouton "Reset Game Time" d'InfoDialog le fait repartir de zéro (voir
    /// `StateManager::reset_playtime`).
    pub playtime_seconds: u64,
}

impl Default for InstalledInfo {
    fn default() -> Self {
        InstalledInfo {
            installed_tag: None,
            installed_at: String::new(),
            favorite_exe: None,
            update: true,
            playtime_seconds: 0,
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
    fn from_value_source_manquante_est_un_port_local() {
        let p = port_from_value(&json!({"name": "n", "folder": "f"})).unwrap();
        assert_eq!(p.source_type, SourceType::Local);
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
        assert!(p.tags.is_empty());
        assert_eq!(p.website, None);
        assert_eq!(p.mods, None);
        assert_eq!(p.image, None);
    }

    #[test]
    fn from_value_filtre_les_tags_non_chaine() {
        let p = port_from_value(&json!({
            "name": "n", "folder": "f", "source": "s", "tags": ["a", 5, "b", null]
        }))
        .unwrap();
        assert_eq!(p.tags, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn key_utilise_repo_sinon_folder() {
        let p = port_from_value(&json!({"name": "n", "folder": "f", "source": "https://github.com/o/p"})).unwrap();
        assert_eq!(p.key(), "o/p");
        let p = port_from_value(&json!({"name": "n", "folder": "f", "source": "https://example.com/x.zip"})).unwrap();
        assert_eq!(p.key(), "f");
    }
}
