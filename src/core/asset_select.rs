//! Heuristique de sélection d'asset partagée entre GitHub et GitLab : à
//! partir d'une liste d'assets nommés, choisit celui qui correspond à la
//! plateforme courante. Si l'heuristique ne peut pas trancher seule,
//! l'appelant peut proposer un choix manuel via `AssetSelectionError::Ambiguous`.
//!
//! Comparaisons de sous-chaînes sur le nom déjà mis en minuscules, jamais de
//! `regex` -- les motifs recherchés (extensions de fichier, mots-clés d'OS/
//! d'architecture) sont tous des sous-chaînes simples, aucun n'a besoin d'un
//! moteur d'expressions régulières complet.

use super::platform_resolve::get_platform_key;
use serde_json::Value;

/// Vrai si `name_lower` (déjà en minuscules) semble correspondre à
/// `platform_key`. Pas d'ancrage sur un mot entier : "win" doit matcher
/// "win32"/"win64"/"windows", et un faux positif isolé est de toute façon
/// départagé par `is_bad_hint`/`has_archive_extension` juste après.
fn os_hint_matches(platform_key: &str, name_lower: &str) -> bool {
    match platform_key {
        "windows" => name_lower.contains("win"),
        "linux" => name_lower.contains("linux"),
        "linux_arm64" => name_lower.contains("linux") && (name_lower.contains("arm64") || name_lower.contains("aarch64")),
        _ => false,
    }
}

/// Assets à exclure quelle que soit la plateforme demandée -- archives
/// source auto-générées par GitHub/GitLab, et binaires clairement pour une
/// autre plateforme (Mac).
fn is_bad_hint(name_lower: &str) -> bool {
    // Retire la ponctuation pour couvrir "source code"/"source-code"/
    // "source_code"/"sourcecode" en une seule recherche de sous-chaîne.
    let alnum_only: String = name_lower.chars().filter(|c| c.is_alphanumeric()).collect();
    alnum_only.contains("sourcecode")
        || name_lower.contains("mac")
        || name_lower.contains("osx")
        || name_lower.ends_with(".dmg")
        || name_lower.ends_with(".deb")
        || name_lower.ends_with(".rpm")
}

fn has_archive_extension(name_lower: &str) -> bool {
    [".zip", ".7z", ".rar", ".exe", ".tar.gz", ".appimage"].iter().any(|ext| name_lower.ends_with(ext))
}

/// Plusieurs mentions possibles pour "architecture 64-bit" selon la
/// convention de nommage du mainteneur.
fn is_arch64_hint(name_lower: &str) -> bool {
    ["x86_64", "x86-64", "x8664", "amd64", "x64"].iter().any(|hint| name_lower.contains(hint))
}

#[derive(Debug)]
pub enum AssetSelectionError {
    Message(String),
    Ambiguous(String, Vec<Value>),
}

fn asset_name(asset: &Value) -> &str {
    asset.get("name").and_then(Value::as_str).unwrap_or("")
}

/// `preferred` -- voir `Port::preferred_asset` -- sous-chaîne du nom déclarée
/// explicitement par le port, court-circuite TOUTE l'heuristique OS/arch
/// ci-dessous quand elle est présente : pour une release où rien ne
/// mentionne l'OS dans le nom (ex: SRB2, dont le vrai build Windows
/// s'appelle juste "Full.zip"), aucune heuristique générique ne peut deviner
/// juste -- mieux vaut une déclaration explicite par port qu'une règle
/// partagée de plus en plus tordue pour deviner tous les cas particuliers.
///
/// `assets`: liste d'objets avec au moins une clé "name".
pub fn pick_asset(assets: &[Value], preferred: Option<&str>) -> Result<Value, AssetSelectionError> {
    if assets.is_empty() {
        return Err(AssetSelectionError::Message("This release doesn't contain any downloadable file.".to_string()));
    }

    if let Some(preferred) = preferred {
        let preferred_lower = preferred.to_lowercase();
        let matches: Vec<&Value> = assets.iter().filter(|a| asset_name(a).to_lowercase().contains(&preferred_lower)).collect();
        return match matches.len() {
            0 => Err(AssetSelectionError::Message(format!("No release file matches \"{preferred}\" (see \"preferred_asset\" for this port)."))),
            1 => Ok(matches[0].clone()),
            _ => Err(AssetSelectionError::Ambiguous(
                format!("Multiple release files match \"{preferred}\" -- please choose one manually."),
                matches.into_iter().cloned().collect(),
            )),
        };
    }

    if assets.len() == 1 {
        return Ok(assets[0].clone());
    }

    let platform_key = get_platform_key();

    let mut candidates: Vec<&Value> = assets
        .iter()
        .filter(|a| {
            let name = asset_name(a).to_lowercase();
            os_hint_matches(platform_key, &name) && !is_bad_hint(&name) && has_archive_extension(&name)
        })
        .collect();
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }

    if candidates.len() > 1 {
        // Plusieurs builds pour le même OS (souvent 32-bit vs 64-bit) : le
        // 64-bit convient à toute machine moderne, préféré plutôt que de
        // renvoyer une ambiguïté à l'utilisateur.
        let arch64: Vec<&&Value> = candidates.iter().filter(|a| is_arch64_hint(&asset_name(a).to_lowercase())).collect();
        if arch64.len() == 1 {
            return Ok((*arch64[0]).clone());
        }
    }

    if platform_key == "linux_arm64" && candidates.is_empty() {
        candidates = assets
            .iter()
            .filter(|a| {
                let name = asset_name(a).to_lowercase();
                os_hint_matches("linux", &name) && !is_bad_hint(&name)
            })
            .collect();
        if candidates.len() == 1 {
            return Ok(candidates[0].clone());
        }
    }

    Err(AssetSelectionError::Ambiguous(
        "Couldn't automatically determine which file to download. Please choose one manually.".to_string(),
        assets.to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asset(name: &str) -> Value {
        json!({"name": name})
    }

    #[test]
    fn un_seul_asset_est_retourne_direct() {
        let assets = vec![asset("anything-mac.dmg")];
        assert!(pick_asset(&assets, None).is_ok());
    }

    #[test]
    fn filtre_os_rejet_et_extension() {
        let assets = vec![
            asset("port-windows.zip"),
            asset("port-source-code.zip"),
            asset("port-macos.dmg"),
            asset("port-linux.zip"),
        ];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), "port-windows.zip");
    }

    #[test]
    fn tie_break_64_bit_avec_exactement_un_match() {
        // "win32"/"win64" ne matchent pas is_arch64_hint (qui cherche
        // x86_64/amd64/x64) -- nommage réel : pd-i686-windows.zip /
        // pd-x86_64-windows.zip.
        let assets = vec![asset("pd-i686-windows.zip"), asset("pd-x86_64-windows.zip")];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), "pd-x86_64-windows.zip");
    }

    #[test]
    fn win32_win64_restent_ambigus_car_aucun_ne_matche_arch64_hint() {
        let assets = vec![asset("port-win32.zip"), asset("port-win64.zip")];
        assert!(matches!(pick_asset(&assets, None), Err(AssetSelectionError::Ambiguous(_, _))));
    }

    #[test]
    fn erreur_ambigue_si_toujours_plusieurs_candidats() {
        let assets = vec![asset("port-windows-a.zip"), asset("port-windows-b.zip")];
        match pick_asset(&assets, None) {
            Err(AssetSelectionError::Ambiguous(_, a)) => assert_eq!(a.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    #[test]
    fn liste_vide_est_une_erreur() {
        assert!(pick_asset(&[], None).is_err());
    }

    #[test]
    fn asset_sans_nom_est_ignore_pas_fatal() {
        let assets = vec![json!({"no_name_field": true}), asset("port-windows.zip")];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), "port-windows.zip");
    }

    #[test]
    fn preferred_asset_court_circuite_l_heuristique_os() {
        // Reproduit SRB2 : aucun asset ne mentionne l'OS, l'heuristique seule
        // ne peut pas deviner -- "Full.zip" en sous-chaîne suffit.
        let assets = vec![asset("SRB2-2.2.15-macOS-Installer.dmg"), asset("SRB2-v2215-Full.zip"), asset("SRB2-v2215-Installer.exe")];
        let picked = pick_asset(&assets, Some("Full.zip")).unwrap();
        assert_eq!(asset_name(&picked), "SRB2-v2215-Full.zip");
    }

    #[test]
    fn preferred_asset_insensible_a_la_casse() {
        let assets = vec![asset("SRB2-v2215-Full.zip"), asset("SRB2-v2215-Installer.exe")];
        let picked = pick_asset(&assets, Some("full.ZIP")).unwrap();
        assert_eq!(asset_name(&picked), "SRB2-v2215-Full.zip");
    }

    #[test]
    fn preferred_asset_sans_correspondance_est_une_erreur_claire() {
        let assets = vec![asset("SRB2-v2215-Full.zip")];
        assert!(matches!(pick_asset(&assets, Some("Portable.zip")), Err(AssetSelectionError::Message(_))));
    }

    #[test]
    fn preferred_asset_avec_plusieurs_correspondances_reste_ambigu() {
        let assets = vec![asset("Game-Full-x86.zip"), asset("Game-Full-x64.zip")];
        match pick_asset(&assets, Some("Full")) {
            Err(AssetSelectionError::Ambiguous(_, a)) => assert_eq!(a.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }
}
