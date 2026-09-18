
use super::platform_resolve::get_platform_key;
use serde_json::Value;

fn os_hint_matches(platform_key: &str, name_lower: &str) -> bool {
    match platform_key {
        "windows" => name_lower.contains("win"),
        "linux" => name_lower.contains("linux"),
        _ => false,
    }
}

fn is_bad_hint(name_lower: &str) -> bool {
    let alnum_only: String = name_lower.chars().filter(|c| c.is_alphanumeric()).collect();
    alnum_only.contains("sourcecode")
        || name_lower.contains("mac")
        || name_lower.contains("osx")
        || name_lower.ends_with(".dmg")
        || name_lower.ends_with(".deb")
        || name_lower.ends_with(".rpm")
}

fn has_non_archive_extension(name_lower: &str) -> bool {
    [".txt", ".md", ".sha256", ".sha1", ".sha512", ".md5", ".sig", ".asc", ".json", ".yml", ".yaml", ".pdf", ".log"]
        .iter()
        .any(|ext| name_lower.ends_with(ext))
}

fn is_arch64_hint(name_lower: &str) -> bool {
    ["x86_64", "x86-64", "x8664", "amd64", "x64"].iter().any(|hint| name_lower.contains(hint))
}

#[derive(Debug)]
pub enum AssetSelectionError {
    Message(String),
    Ambiguous(String, Vec<Value>),
}

impl AssetSelectionError {
    pub fn message(&self) -> &str {
        match self {
            AssetSelectionError::Message(m) => m,
            AssetSelectionError::Ambiguous(m, _) => m,
        }
    }
}

fn asset_name(asset: &Value) -> &str {
    asset.get("name").and_then(Value::as_str).unwrap_or("")
}

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

    let candidates: Vec<&Value> = assets
        .iter()
        .filter(|a| {
            let name = asset_name(a).to_lowercase();
            os_hint_matches(platform_key, &name) && !is_bad_hint(&name) && !has_non_archive_extension(&name)
        })
        .collect();
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }

    if candidates.len() > 1 {
        let arch64: Vec<&&Value> = candidates.iter().filter(|a| is_arch64_hint(&asset_name(a).to_lowercase())).collect();
        if arch64.len() == 1 {
            return Ok((*arch64[0]).clone());
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
        assert_eq!(asset_name(&picked), format!("port-{}.zip", get_platform_key()));
    }

    #[test]
    fn tie_break_64_bit_avec_exactement_un_match() {
        let os = get_platform_key();
        let assets = vec![asset(&format!("pd-i686-{os}.zip")), asset(&format!("pd-x86_64-{os}.zip"))];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), format!("pd-x86_64-{os}.zip"));
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
    fn accepte_un_nom_sans_extension_comme_un_lien_gitlab() {
        let assets = vec![
            asset("ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo"),
            asset("ExtremeGRecompiled-v1.0.0-macOS-Release"),
            asset("ExtremeGRecompiled-v1.0.0-Linux-X64-Release"),
        ];
        let picked = pick_asset(&assets, None).unwrap();
        let expected = if get_platform_key() == "windows" {
            "ExtremeGRecompiled-v1.0.0-Windows-RelWithDebInfo"
        } else {
            "ExtremeGRecompiled-v1.0.0-Linux-X64-Release"
        };
        assert_eq!(asset_name(&picked), expected);
    }

    #[test]
    fn rejette_toujours_un_checksum_meme_matchant_los() {
        let os = get_platform_key();
        let assets = vec![asset(&format!("port-{os}.zip")), asset(&format!("port-{os}.sha256"))];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), format!("port-{os}.zip"));
    }

    #[test]
    fn liste_vide_est_une_erreur() {
        assert!(pick_asset(&[], None).is_err());
    }

    #[test]
    fn asset_sans_nom_est_ignore_pas_fatal() {
        let os = get_platform_key();
        let assets = vec![json!({"no_name_field": true}), asset(&format!("port-{os}.zip"))];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), format!("port-{os}.zip"));
    }

    #[test]
    fn preferred_asset_court_circuite_l_heuristique_os() {
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
