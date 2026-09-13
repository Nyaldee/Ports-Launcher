//! Résolution des valeurs multi-plateforme (`executable`/`save_folder`/
//! `source` quand ce sont des objets `{"windows": ..., "linux": ...}`) et
//! expansion des `%VARIABLE%` Windows dans un chemin.

use serde_json::Value;
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use windows::core::GUID;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
    SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

#[cfg(target_os = "windows")]
pub fn get_platform_key() -> &'static str {
    "windows"
}

#[cfg(target_os = "linux")]
pub fn get_platform_key() -> &'static str {
    "linux"
}

pub fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `value` : une valeur simple, ou un objet `{"windows": ..., "linux": ...}`.
/// Retourne la valeur qui correspond à la plateforme courante.
pub fn resolve_per_platform(value: &Value) -> Option<Value> {
    let Some(obj) = value.as_object() else {
        return Some(value.clone());
    };
    if obj.is_empty() {
        // Aucune plateforme définie -- traité comme "valeur absente",
        // cohérent avec le repli d'un champ optionnel manquant.
        return None;
    }
    let key = get_platform_key();
    if let Some(v) = obj.get(key) {
        return Some(v.clone());
    }
    if key.starts_with("linux") {
        if let Some(v) = obj.get("linux") {
            return Some(v.clone());
        }
    }
    if let Some(v) = obj.get("windows") {
        return Some(v.clone());
    }
    // `serde_json` avec la feature "preserve_order" conserve l'ordre
    // d'insertion -- premier élément par ordre d'insertion, dernier recours.
    obj.values().next().cloned()
}

/// Dossiers "connus" de Windows individuellement redirigeables (Propriétés
/// → Emplacement dans l'Explorateur, ou OneDrive "Gérer la sauvegarde") --
/// `%USERPROFILE%\<un de ces noms>` ne vit pas forcément où sa
/// concaténation littérale le suggère. Liste FERMÉE : ce sont les seuls
/// dossiers que Windows peut individuellement déplacer ailleurs -- un
/// sous-dossier arbitraire du profil (ex: `%USERPROFILE%\New folder`) n'a
/// structurellement pas ce problème (pas de GUID, jamais proposé comme
/// redirigeable), sa concaténation littérale est déjà correcte.
#[cfg(target_os = "windows")]
const KNOWN_FOLDERS: &[(&str, GUID)] = &[
    ("Desktop", FOLDERID_Desktop),
    ("Documents", FOLDERID_Documents),
    ("Downloads", FOLDERID_Downloads),
    ("Music", FOLDERID_Music),
    ("Pictures", FOLDERID_Pictures),
    ("Videos", FOLDERID_Videos),
];

/// Chemin réel d'un dossier connu de Windows (`id` = une des GUID de
/// KNOWN_FOLDERS), PAS sa concaténation littérale sous `%USERPROFILE%` :
/// OneDrive (Known Folder Move) ou un déplacement manuel (voir
/// "Emplacement" dans les Propriétés du dossier) le redirigent couramment
/// ailleurs (autre lettre de lecteur, sous-dossier OneDrive...). `None` si
/// l'appel échoue -- laissé au repli de l'appelant.
#[cfg(target_os = "windows")]
fn known_folder(id: &GUID) -> Option<String> {
    unsafe {
        let pwstr = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let result = pwstr.to_string().ok();
        CoTaskMemFree(Some(pwstr.0 as *const _));
        result
    }
}

/// Si `rest` commence par un séparateur suivi du composant de chemin
/// complet d'un des noms de KNOWN_FOLDERS (pas juste ce préfixe --
/// "Documentsfoo" ne compte pas), retourne sa GUID et ce qui suit ce
/// composant. Sert à repérer `%USERPROFILE%\<dossier connu>\...` dans
/// expand_env_path pour le résoudre via le vrai dossier plutôt qu'une
/// concaténation littérale.
#[cfg(target_os = "windows")]
fn strip_known_folder_component(rest: &str) -> Option<(GUID, &str)> {
    let after_sep = rest.strip_prefix(['\\', '/'])?;
    for (name, guid) in KNOWN_FOLDERS {
        let Some((head, tail)) = after_sep.split_at_checked(name.len()) else { continue };
        if head.eq_ignore_ascii_case(name) && (tail.is_empty() || tail.starts_with(['\\', '/'])) {
            return Some((*guid, tail));
        }
    }
    None
}

/// Repli `%USERPROFILE%\<dossier connu>` (voir KNOWN_FOLDERS), résolu via le
/// vrai dossier plutôt qu'une concaténation littérale, pour survivre à une
/// redirection -- concept propre à Windows. Sous Linux, `%VARIABLE%` est
/// déjà une simple expansion d'environnement générique (voir
/// `expand_env_path`) ; aucun dossier n'a besoin d'une résolution à part.
#[cfg(target_os = "windows")]
fn known_folder_override<'a>(var_name: &str, after_var: &'a str) -> Option<(String, &'a str)> {
    if !var_name.eq_ignore_ascii_case("USERPROFILE") {
        return None;
    }
    let (guid, remainder) = strip_known_folder_component(after_var)?;
    let folder = known_folder(&guid)?;
    Some((folder, remainder))
}

#[cfg(target_os = "linux")]
fn known_folder_override<'a>(_var_name: &str, _after_var: &'a str) -> Option<(String, &'a str)> {
    None
}

/// Étend les `%VARIABLE%` d'un chemin (`save_folder` de `ports.json`
/// contient souvent `%APPDATA%`/`%LOCALAPPDATA%`/`%USERPROFILE%`).
/// Une variable inconnue est laissée telle quelle, jamais une erreur : un
/// `save_folder` qui ne s'applique pas à cette machine doit juste échouer
/// le `.exists()` qui en dépend ensuite, pas planter l'affichage du
/// dialogue d'info.
pub fn expand_env_path(path: &str) -> PathBuf {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        let (before, after_percent) = rest.split_at(start);
        out.push_str(before);
        let after_percent = &after_percent[1..];
        match after_percent.find('%') {
            Some(end) => {
                let var_name = &after_percent[..end];
                let after_var = &after_percent[end + 1..];
                if let Some((folder, remainder)) = known_folder_override(var_name, after_var) {
                    out.push_str(&folder);
                    rest = remainder;
                    continue;
                }
                match std::env::var(var_name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var_name);
                        out.push('%');
                    }
                }
                rest = after_var;
            }
            None => {
                // '%' non refermé -- laissé tel quel, reste du chemin copié
                // ci-dessous une fois la boucle terminée.
                out.push('%');
                rest = after_percent;
                break;
            }
        }
    }
    out.push_str(rest);
    PathBuf::from(out)
}

/// Résout `save_folder` (per-plateforme + variables d'environnement) en un
/// chemin ABSOLU -- un `save_folder` sans `%VARIABLE%` (ex: "save", "Save",
/// "saves") est RELATIF AU DOSSIER DU JEU (`game_dir`), jamais au dossier
/// courant du processus. Le joindre ici est nécessaire pour que le bouton
/// "Save folder" trouve une sauvegarde stockée à côté de l'exécutable, ET
/// pour qu'`installer::uninstall_port` détecte qu'elle vit DANS le dossier
/// sur le point d'être supprimé.
pub fn resolve_save_folder(save_folder: &Value, game_dir: &Path) -> Option<PathBuf> {
    let resolved = resolve_per_platform(save_folder)?;
    let s = resolved.as_str()?;
    if s.is_empty() {
        return None;
    }
    let expanded = expand_env_path(s);
    if expanded.is_absolute() {
        Some(expanded)
    } else {
        Some(game_dir.join(expanded))
    }
}

/// Résout `preferred_asset` (per-plateforme, même mécanisme que
/// `save_folder`/`executable` ci-dessus) en la sous-chaîne à chercher dans
/// le nom des assets d'une release (voir `asset_select::pick_asset`).
pub fn resolve_preferred_asset(preferred_asset: &Value) -> Option<String> {
    let resolved = resolve_per_platform(preferred_asset)?;
    let s = resolved.as_str()?;
    if s.is_empty() {
        return None;
    }
    // `pick_asset` traite `preferred` comme absolu : présent, il court-
    // circuite l'heuristique OS générique même sans aucune correspondance.
    // Une valeur plate en ".exe" ferait donc échouer le port à coup sûr sous
    // Linux -- ignorée pour laisser l'heuristique reprendre la main. Un objet
    // {"windows": ..., "linux": ...} reste lui honoré tel quel même en ".exe"
    // (déclaration délibérée par port, ex: pas de build native mais jouable
    // sous Wine) : seule une valeur PLATE peut être un reliquat Windows
    // jamais adapté à Linux.
    #[cfg(target_os = "linux")]
    if !preferred_asset.is_object() && s.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_per_platform_valeur_simple_est_inchangee() {
        assert_eq!(resolve_per_platform(&json!("x")), Some(json!("x")));
    }

    #[test]
    fn resolve_per_platform_objet_vide_est_none() {
        assert_eq!(resolve_per_platform(&json!({})), None);
    }

    #[test]
    fn resolve_per_platform_cle_exacte_puis_repli_premiere_valeur() {
        // La clé exacte de la plateforme courante gagne toujours.
        let mut obj = serde_json::Map::new();
        obj.insert(get_platform_key().to_string(), json!("w"));
        obj.insert("other-os".to_string(), json!("o"));
        assert_eq!(resolve_per_platform(&Value::Object(obj)), Some(json!("w")));
        // Aucune clé de plateforme connue -- repli sur la 1ère valeur par
        // ordre d'insertion.
        assert_eq!(resolve_per_platform(&json!({"other-os-1": "l", "other-os-2": "m"})), Some(json!("l")));
    }

    #[test]
    fn expand_env_path_etend_une_variable_connue() {
        std::env::set_var("PL_TEST_VAR", "C:\\Somewhere");
        assert_eq!(expand_env_path("%PL_TEST_VAR%\\save"), PathBuf::from("C:\\Somewhere\\save"));
        std::env::remove_var("PL_TEST_VAR");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn expand_env_path_userprofile_documents_utilise_le_dossier_connu() {
        // Pas de chemin en dur : %USERPROFILE%\Documents\... doit se
        // résoudre via le même dossier connu que known_folder() renvoie,
        // pas via une concaténation littérale de %USERPROFILE% -- vrai même
        // quand Documents est redirigé (OneDrive, déplacement manuel...).
        let documents = known_folder(&FOLDERID_Documents).expect("SHGetKnownFolderPath(FOLDERID_Documents) a échoué");
        assert_eq!(expand_env_path("%USERPROFILE%\\Documents\\eternalsonata"), PathBuf::from(documents).join("eternalsonata"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn expand_env_path_userprofile_reconnait_les_autres_dossiers_connus() {
        // Même mécanisme que Documents, sur un autre dossier de
        // KNOWN_FOLDERS : la résolution vaut pour toute la liste.
        let downloads = known_folder(&FOLDERID_Downloads).expect("SHGetKnownFolderPath(FOLDERID_Downloads) a échoué");
        assert_eq!(expand_env_path("%USERPROFILE%\\Downloads\\file.zip"), PathBuf::from(downloads).join("file.zip"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn expand_env_path_userprofile_documentsfoo_nest_pas_confondu_avec_documents() {
        // "Documentsfoo" n'est pas le composant "Documents" -- doit rester
        // une concaténation littérale de %USERPROFILE%, pas le dossier connu.
        let original = std::env::var("USERPROFILE").ok();
        std::env::set_var("USERPROFILE", "C:\\Users\\test");
        assert_eq!(expand_env_path("%USERPROFILE%\\Documentsfoo\\save"), PathBuf::from("C:\\Users\\test\\Documentsfoo\\save"));
        match original {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
    }

    #[test]
    fn expand_env_path_laisse_une_variable_inconnue_telle_quelle() {
        assert_eq!(expand_env_path("%PL_DOES_NOT_EXIST%\\save"), PathBuf::from("%PL_DOES_NOT_EXIST%\\save"));
    }

    #[test]
    fn expand_env_path_sans_variable_est_inchange() {
        assert_eq!(expand_env_path("save"), PathBuf::from("save"));
    }

    #[test]
    fn resolve_save_folder_relatif_se_joint_au_dossier_du_jeu() {
        let game_dir = Path::new("C:\\Library\\MyGame");
        let save_folder = Value::String("Save".into());
        assert_eq!(resolve_save_folder(&save_folder, game_dir), Some(game_dir.join("Save")));
    }

    #[test]
    fn resolve_save_folder_avec_variable_reste_absolu() {
        // Chemin absolu propre à l'OS courant -- `Path::is_absolute()` (dont
        // dépend `resolve_save_folder`) n'accepte un chemin à la Windows
        // (lettre de lecteur) que sous Windows, et inversement pour un
        // chemin à la Unix (`/`) sous Linux.
        #[cfg(target_os = "windows")]
        let absolute = "C:\\Users\\me\\AppData";
        #[cfg(target_os = "linux")]
        let absolute = "/home/me/.local/share";

        std::env::set_var("PL_TEST_SAVE_VAR", absolute);
        let game_dir = Path::new("C:\\Library\\MyGame");
        // `expand_env_path` ne fait qu'une substitution de chaîne -- le
        // séparateur après %VAR% doit être celui de l'OS courant, pas
        // normalisé automatiquement.
        #[cfg(target_os = "windows")]
        let save_folder = Value::String("%PL_TEST_SAVE_VAR%\\MyGame".into());
        #[cfg(target_os = "linux")]
        let save_folder = Value::String("%PL_TEST_SAVE_VAR%/MyGame".into());
        let expected = PathBuf::from(absolute).join("MyGame");
        assert_eq!(resolve_save_folder(&save_folder, game_dir), Some(expected));
        std::env::remove_var("PL_TEST_SAVE_VAR");
    }

    #[test]
    fn resolve_preferred_asset_chaine_simple_toute_plateforme() {
        assert_eq!(resolve_preferred_asset(&Value::String("Full.zip".into())), Some("Full.zip".to_string()));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn resolve_preferred_asset_valeur_plate_en_exe_est_ignoree_sous_linux() {
        // Port jamais mis à jour avec un "preferred_asset" par plateforme --
        // ".exe" ne peut de toute façon jamais matcher un asset Linux, donc
        // traité comme absent plutôt que de faire échouer `pick_asset` à
        // coup sûr (voir le commentaire de la fonction).
        assert_eq!(resolve_preferred_asset(&Value::String(".exe".into())), None);
        assert_eq!(resolve_preferred_asset(&Value::String("Installer.EXE".into())), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn resolve_preferred_asset_objet_explicite_en_exe_est_honore_sous_linux() {
        // Contrairement au cas plat ci-dessus : un objet par plateforme est
        // une déclaration délibérée (ex: pas de build Linux native, mais
        // l'.exe tourne sous Wine) -- jamais ignoré, même si la valeur
        // "linux" vaut ".exe".
        assert_eq!(resolve_preferred_asset(&json!({"windows": ".exe", "linux": ".exe"})), Some(".exe".to_string()));
    }

    #[test]
    fn resolve_preferred_asset_par_plateforme() {
        let expected = if get_platform_key() == "windows" { ".exe" } else { ".AppImage" };
        assert_eq!(resolve_preferred_asset(&json!({"windows": ".exe", "linux": ".AppImage"})), Some(expected.to_string()));
    }

    #[test]
    fn resolve_preferred_asset_chaine_vide_est_none() {
        assert_eq!(resolve_preferred_asset(&Value::String(String::new())), None);
    }

    #[test]
    fn resolve_preferred_asset_objet_sans_cle_pour_cette_plateforme_est_none() {
        // Ni "windows" ni la clé de la plateforme courante -- repli sur la
        // seule valeur présente ("linux": null), qui reste None une fois
        // convertie en chaîne, peu importe la plateforme d'exécution du test.
        assert_eq!(resolve_preferred_asset(&json!({"linux": null})), None);
    }
}
