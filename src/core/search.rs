
use super::models::Port;

fn fuzzy_span(text: &str, query: &str) -> Option<usize> {
    let mut chars = text.chars().enumerate();
    let mut first: Option<usize> = None;
    let mut last = 0usize;
    for c in query.chars() {
        let (idx, _) = chars.by_ref().find(|&(_, t)| t == c)?;
        first.get_or_insert(idx);
        last = idx;
    }
    Some(last - first?)
}

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
            "name": name, "folder": name, "source": "https://example.com/x.zip",
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
            let result = filter_and_sort(&pool, query);
            assert!(result.len() <= pool.len(), "requête {query:?} a produit plus de résultats que le pool");
        }
    }
}
