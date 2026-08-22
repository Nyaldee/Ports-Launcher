//! Recherche floue sur le catalogue -- 5 paliers de pertinence, du plus au
//! moins pertinent (nom qui commence par la requête, nom qui la contient,
//! tag qui commence par elle, tag qui la contient, puis un repli en
//! sous-séquence floue balayée de gauche à droite).

use super::models::Port;

/// Distance entre la première et la dernière lettre de `query` trouvées
/// dans `text`, DANS L'ORDRE (correspondance floue façon "sous-séquence",
/// pas forcément consécutive) -- `None` si `query` n'est pas une
/// sous-séquence de `text`. Plus la distance est petite, plus la
/// correspondance est "serrée" (classée avant une correspondance plus
/// étalée, à tier égal).
fn fuzzy_span(text: &str, query: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut first: Option<usize> = None;
    let mut last = 0usize;
    let mut start = 0usize;
    for c in query.chars() {
        let idx = chars[start..].iter().position(|&t| t == c)? + start;
        first.get_or_insert(idx);
        last = idx;
        start = idx + 1;
    }
    Some(last - first?)
}

/// `(tier, span)` -- `tier` plus petit = plus pertinent, `span` départage à
/// tier égal (seulement significatif au tier 4). `None` si `port` ne
/// correspond pas du tout à `query` (déjà mise en minuscules par
/// l'appelant, voir `filter_and_sort`).
fn search_tier(port: &Port, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }
    let name = &port.name_lower;
    if name.starts_with(query) {
        return Some((0, 0));
    }
    if name.contains(query) {
        return Some((1, 0));
    }
    if port.tags_lower.iter().any(|t| t.starts_with(query)) {
        return Some((2, 0));
    }
    if port.tags_lower.iter().any(|t| t.contains(query)) {
        return Some((3, 0));
    }
    fuzzy_span(name, query).map(|span| (4, span))
}

/// Filtre `pool` sur `query` et trie par pertinence -- tri STABLE (à tier
/// égal, l'ordre d'origine du catalogue est préservé, jamais réordonné au
/// hasard entre deux entrées à égalité).
pub fn filter_and_sort<'a>(pool: &[&'a Port], query: &str) -> Vec<&'a Port> {
    let query = query.trim().to_lowercase();
    let mut scored: Vec<((u8, usize), &Port)> =
        pool.iter().filter_map(|&p| search_tier(p, &query).map(|tier| (tier, p))).collect();
    scored.sort_by_key(|(tier, _)| *tier);
    scored.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn port(name: &str, tags: &[&str]) -> Port {
        super::super::models::port_from_value(&json!({
            "name": name, "folder_name": name, "source": "https://example.com/x.zip",
            "tags": tags,
        }))
        .unwrap()
    }

    #[test]
    fn tier_0_nom_commence_par_query() {
        let p = port("Ape Escape", &[]);
        assert_eq!(search_tier(&p, "ape"), Some((0, 0)));
    }

    #[test]
    fn tier_1_nom_contient_query() {
        let p = port("Ape Escape", &[]);
        assert_eq!(search_tier(&p, "escape"), Some((1, 0)));
    }

    #[test]
    fn tier_2_tag_commence_par_query() {
        let p = port("Ape Escape", &["Saru", "PS1"]);
        assert_eq!(search_tier(&p, "ps1"), Some((2, 0)));
    }

    #[test]
    fn tier_3_tag_contient_query() {
        let p = port("Ape Escape", &["Platformer"]);
        assert_eq!(search_tier(&p, "form"), Some((3, 0)));
    }

    #[test]
    fn tier_4_correspondance_floue_sous_sequence() {
        let p = port("GoldenEye", &[]);
        // "gde" est une sous-séquence de "goldeneye" (g...d...e...).
        assert!(matches!(search_tier(&p, "gde"), Some((4, _))));
    }

    #[test]
    fn aucune_correspondance_est_none() {
        let p = port("Ape Escape", &["Platformer"]);
        assert_eq!(search_tier(&p, "zzz"), None);
    }

    #[test]
    fn query_vide_matche_tout_au_tier_0() {
        let p = port("Ape Escape", &[]);
        assert_eq!(search_tier(&p, ""), Some((0, 0)));
    }

    #[test]
    fn filter_and_sort_est_stable_a_tier_egal() {
        let a = port("Alpha Game", &[]);
        let b = port("Alpha Racer", &[]);
        let pool: Vec<&Port> = vec![&a, &b];
        // Les deux commencent par "alpha" (tier 0) -- l'ordre d'origine
        // (a avant b) doit être préservé.
        let result = filter_and_sort(&pool, "alpha");
        assert_eq!(result[0].name, "Alpha Game");
        assert_eq!(result[1].name, "Alpha Racer");
    }

    #[test]
    fn filter_and_sort_classe_les_tiers_dans_l_ordre() {
        let exact = port("Zeta", &["tag"]);
        let contains = port("Alpha Zeta Beta", &[]);
        let tag_match = port("Gamma", &["Zeta-ish"]);
        let pool: Vec<&Port> = vec![&contains, &tag_match, &exact];
        let result = filter_and_sort(&pool, "zeta");
        assert_eq!(result[0].name, "Zeta");
        assert_eq!(result[1].name, "Alpha Zeta Beta");
        assert_eq!(result[2].name, "Gamma");
    }

    #[test]
    fn stress_requetes_adversariales_sur_un_gros_catalogue_ne_plante_jamais() {
        // Catalogue synthétique large, généré en mémoire (aucun accès
        // disque/réseau) : noms/tags variés, unicode et accentués inclus.
        let names = [
            "Ape Escape", "GoldenEye", "Pikmin Recomp", "Portal Récomp", "Zelda: 🎮 Édition",
            "", "   ", "A", "アプリ", "Ω-Game",
        ];
        let ports: Vec<Port> = (0..3000)
            .map(|i| {
                let name = format!("{} {i}", names[i % names.len()]);
                let tags = [format!("tag{}", i % 7), "Platformer".to_string()];
                port(&name, &tags.iter().map(String::as_str).collect::<Vec<_>>())
            })
            .collect();
        let pool: Vec<&Port> = ports.iter().collect();

        let adversarial_queries = [
            "".to_string(),
            "   ".to_string(),
            "x".repeat(20_000),
            "🎮".repeat(500),
            "\n\t\r".to_string(),
            "ZZZZZ-no-match-ZZZZZ".to_string(),
            "é".repeat(50),
            "a b c d e f g h i j".to_string(),
            "PIKMIN".to_string(),
            "0".to_string(),
        ];

        for query in &adversarial_queries {
            // Exigences : jamais de panique (dépassement d'index dans
            // fuzzy_span sur une requête plus longue que certains noms), et
            // un résultat toujours borné par la taille du pool.
            let result = filter_and_sort(&pool, query);
            assert!(result.len() <= pool.len(), "requête {query:?} a produit plus de résultats que le pool");
        }
    }
}
