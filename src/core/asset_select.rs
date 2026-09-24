
use super::platform_resolve::get_platform_key;
use serde_json::Value;

fn os_hint_matches(platform_key: &str, name_lower: &str) -> bool {
    match platform_key {
        "windows" => name_lower.contains("win"),
        "linux" => name_lower.contains("linux"),
        _ => false,
    }
}

fn foreign_os_hints(platform_key: &str) -> &'static [&'static str] {
    match platform_key {
        "windows" => &["linux", "android", "mac", "osx"],
        "linux" => &["win", "android", "mac", "osx"],
        _ => &[],
    }
}

fn has_preferred_extension(platform_key: &str, name_lower: &str) -> bool {
    let extensions: &[&str] = match platform_key {
        "windows" => &[".zip", ".exe", ".7z", ".rar"],
        "linux" => &[".tar.gz", ".appimage", ".7z", ".rar"],
        _ => &[],
    };
    extensions.iter().any(|ext| name_lower.ends_with(ext))
}

fn is_unusable_format(name_lower: &str) -> bool {
    let alnum_only: String = name_lower.chars().filter(|c| c.is_alphanumeric()).collect();
    alnum_only.contains("sourcecode") || [".dmg", ".deb", ".rpm", ".apk"].iter().any(|ext| name_lower.ends_with(ext))
}

fn has_non_archive_extension(name_lower: &str) -> bool {
    [".txt", ".md", ".sha256", ".sha1", ".sha512", ".md5", ".sig", ".asc", ".json", ".yml", ".yaml", ".pdf", ".log"]
        .iter()
        .any(|ext| name_lower.ends_with(ext))
}

fn is_arch64_hint(name_lower: &str) -> bool {
    ["x86_64", "x86-64", "x8664", "amd64", "x64"].iter().any(|hint| name_lower.contains(hint))
}

fn asset_tier(platform_key: &str, name_lower: &str) -> u8 {
    if os_hint_matches(platform_key, name_lower) && has_preferred_extension(platform_key, name_lower) {
        1
    } else if foreign_os_hints(platform_key).iter().any(|hint| name_lower.contains(hint)) {
        3
    } else {
        2
    }
}

#[derive(Debug)]
pub enum AssetSelectionError {
    Message(String),
    Ambiguous(Vec<Value>),
}

impl AssetSelectionError {
    pub fn message(&self) -> &str {
        match self {
            AssetSelectionError::Message(m) => m,
            AssetSelectionError::Ambiguous(_) => "Multiple files match -- please choose one manually.",
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
            _ => Err(AssetSelectionError::Ambiguous(matches.into_iter().cloned().collect())),
        };
    }

    let platform_key = get_platform_key();
    let ranked: Vec<(&Value, u8)> = assets
        .iter()
        .filter_map(|a| {
            let name = asset_name(a).to_lowercase();
            (!is_unusable_format(&name) && !has_non_archive_extension(&name)).then(|| (a, asset_tier(platform_key, &name)))
        })
        .collect();
    let Some(best_rank) = ranked.iter().map(|(_, tier)| *tier).min() else {
        return Err(AssetSelectionError::Ambiguous(assets.to_vec()));
    };
    let best: Vec<&Value> = ranked.into_iter().filter(|(_, tier)| *tier == best_rank).map(|(a, _)| a).collect();
    if best.len() == 1 {
        return Ok(best[0].clone());
    }

    let arch64: Vec<&&Value> = best.iter().filter(|a| is_arch64_hint(&asset_name(a).to_lowercase())).collect();
    if arch64.len() == 1 {
        return Ok((*arch64[0]).clone());
    }
    Err(AssetSelectionError::Ambiguous(best.into_iter().cloned().collect()))
}

pub const RECENT_RELEASES_DEPTH: usize = 5;

pub fn latest_installable_release(
    releases: Vec<Value>,
    preferred: Option<&str>,
    pick_release_asset: impl Fn(&Value, Option<&str>) -> Result<Value, AssetSelectionError>,
) -> Result<(Value, Value), AssetSelectionError> {
    let mut first_error = None;
    for release in releases {
        match pick_release_asset(&release, preferred) {
            Ok(asset) => return Ok((release, asset)),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| AssetSelectionError::Message("No release found.".to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asset(name: &str) -> Value {
        json!({"name": name})
    }

    fn preferred_ext(os: &str) -> &'static str {
        if os == "windows" {
            "zip"
        } else {
            "tar.gz"
        }
    }

    fn other_os() -> &'static str {
        if get_platform_key() == "windows" {
            "linux"
        } else {
            "windows"
        }
    }

    #[test]
    fn un_seul_asset_usable_est_retourne_meme_si_os_etranger() {
        let assets = vec![asset(&format!("anything-{}.zip", other_os()))];
        assert!(pick_asset(&assets, None).is_ok());
    }

    #[test]
    fn format_incompatible_reste_exclu_meme_seul_present() {
        let assets = vec![asset("anything-mac.dmg")];
        assert!(pick_asset(&assets, None).is_err());
    }

    #[test]
    fn filtre_os_rejet_et_extension() {
        let assets = vec![asset("port-windows.zip"), asset("port-source-code.zip"), asset("port-macos.dmg"), asset("port-linux.tar.gz")];
        let picked = pick_asset(&assets, None).unwrap();
        let os = get_platform_key();
        assert_eq!(asset_name(&picked), format!("port-{os}.{}", preferred_ext(os)));
    }

    #[test]
    fn tier1_nom_plus_extension_bat_tier2_nom_seul() {
        let os = get_platform_key();
        let assets = vec![asset(&format!("port-{os}-a.unknownext")), asset(&format!("port-{os}-b.{}", preferred_ext(os)))];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), format!("port-{os}-b.{}", preferred_ext(os)));
    }

    #[test]
    fn tag_os_etranger_perd_face_a_un_candidat_neutre() {
        let assets = vec![asset(&format!("port-{}-explicit.zip", other_os())), asset("port-neutral.zip")];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), "port-neutral.zip");
    }

    #[test]
    fn extensions_preferees_reconnues_par_plateforme() {
        let os = get_platform_key();
        let extensions: &[&str] = if os == "windows" { &["zip", "exe", "7z", "rar"] } else { &["tar.gz", "AppImage", "7z", "rar"] };
        for ext in extensions {
            let assets = vec![asset(&format!("port-{os}.{ext}")), asset(&format!("port-{os}-alt.unknownext"))];
            let picked = pick_asset(&assets, None).unwrap();
            assert_eq!(asset_name(&picked), format!("port-{os}.{ext}"), "l'extension {ext} devrait être de rang 1");
        }
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
        assert!(matches!(pick_asset(&assets, None), Err(AssetSelectionError::Ambiguous(_))));
    }

    #[test]
    fn erreur_ambigue_si_toujours_plusieurs_candidats() {
        let assets = vec![asset("port-windows-a.zip"), asset("port-windows-b.zip")];
        match pick_asset(&assets, None) {
            Err(AssetSelectionError::Ambiguous(a)) => assert_eq!(a.len(), 2),
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
        let assets = vec![asset(&format!("port-{os}.{}", preferred_ext(os))), asset(&format!("port-{os}.sha256"))];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), format!("port-{os}.{}", preferred_ext(os)));
    }

    #[test]
    fn liste_vide_est_une_erreur() {
        assert!(pick_asset(&[], None).is_err());
    }

    #[test]
    fn asset_sans_nom_est_ignore_pas_fatal() {
        let os = get_platform_key();
        let assets = vec![json!({"no_name_field": true}), asset(&format!("port-{os}.{}", preferred_ext(os)))];
        let picked = pick_asset(&assets, None).unwrap();
        assert_eq!(asset_name(&picked), format!("port-{os}.{}", preferred_ext(os)));
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
            Err(AssetSelectionError::Ambiguous(a)) => assert_eq!(a.len(), 2),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    fn release_with_assets(names: &[&str]) -> Value {
        json!({"assets": names.iter().map(|n| asset(n)).collect::<Vec<_>>()})
    }

    fn pick_from_release(release: &Value, preferred: Option<&str>) -> Result<Value, AssetSelectionError> {
        let assets = release.get("assets").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        pick_asset(assets, preferred)
    }

    #[test]
    fn latest_installable_release_saute_une_release_sans_asset_valide() {
        let os = get_platform_key();
        let releases = vec![release_with_assets(&["app-android.apk", "app-android.apk.sha256"]), release_with_assets(&[&format!("app-{os}.zip")])];
        let (release, asset) = latest_installable_release(releases.clone(), None, pick_from_release).unwrap();
        assert_eq!(asset_name(&asset), format!("app-{os}.zip"));
        assert_eq!(release, releases[1]);
    }

    #[test]
    fn latest_installable_release_prend_la_plus_recente_si_elle_convient() {
        let os = get_platform_key();
        let releases = vec![release_with_assets(&[&format!("app-{os}.zip")]), release_with_assets(&["app-android.apk"])];
        let (release, _) = latest_installable_release(releases.clone(), None, pick_from_release).unwrap();
        assert_eq!(release, releases[0]);
    }

    #[test]
    fn latest_installable_release_renvoie_l_erreur_de_la_plus_recente_si_aucune_ne_convient() {
        let releases = vec![release_with_assets(&["app-android.apk"]), release_with_assets(&["app-mac.dmg"])];
        match latest_installable_release(releases, None, pick_from_release) {
            Err(AssetSelectionError::Ambiguous(a)) => assert_eq!(a.len(), 1),
            other => panic!("attendu Ambiguous, obtenu {other:?}"),
        }
    }

    #[test]
    fn latest_installable_release_erreur_claire_sur_une_liste_vide() {
        assert!(latest_installable_release(vec![], None, pick_from_release).is_err());
    }
}
