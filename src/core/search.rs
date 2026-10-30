pub const MAX_QUERY_CHARS: usize = 100;

pub fn truncate_query(query: &str) -> Option<String> {
    query.char_indices().nth(MAX_QUERY_CHARS).map(|(end, _)| query[..end].to_string())
}

pub type Rank = (u8, usize);

pub fn normalize(s: &str) -> String {
    s.to_lowercase().chars().map(strip_accent).collect()
}

fn strip_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

fn fuzzy_window(name: &[char], term: &[char]) -> Option<usize> {
    let (&last, before) = term.split_last()?;
    let mut best: Option<usize> = None;
    for (end, _) in name.iter().enumerate().filter(|&(_, &c)| c == last) {
        let mut pos = end;
        let matched = before.iter().rev().all(|&c| match name[..pos].iter().rposition(|&x| x == c) {
            Some(p) => {
                pos = p;
                true
            }
            None => false,
        });
        if matched {
            let span = end - pos;
            best = Some(best.map_or(span, |b| b.min(span)));
        }
    }
    best
}

fn term_rank(name: &str, tags: &[String], term: &str) -> Option<Rank> {
    if name.starts_with(term) {
        return Some((0, 0));
    }
    if name.contains(term) {
        return Some((1, 0));
    }
    if tags.iter().any(|t| t.starts_with(term)) {
        return Some((2, 0));
    }
    if tags.iter().any(|t| t.contains(term)) {
        return Some((3, 0));
    }
    let name: Vec<char> = name.chars().collect();
    let term: Vec<char> = term.chars().collect();
    fuzzy_window(&name, &term).map(|span| (4, span))
}

pub fn rank(name: &str, tags: &[String], query: &str) -> Option<Rank> {
    let mut worst_tier = 0u8;
    let mut total_span = 0usize;
    for term in query.split_whitespace() {
        let (tier, span) = term_rank(name, tags, term)?;
        worst_tier = worst_tier.max(tier);
        total_span += span;
    }
    Some((worst_tier, total_span))
}

pub fn filter_and_sort<'a, T: 'a>(items: impl IntoIterator<Item = &'a T>, query: &str, key: impl Fn(&T) -> (&str, &[String])) -> Vec<&'a T> {
    let query = normalize(query.trim());
    let mut ranked: Vec<(Rank, &T)> = items
        .into_iter()
        .filter_map(|item| {
            let (name, tags) = key(item);
            rank(name, tags, &query).map(|r| (r, item))
        })
        .collect();
    ranked.sort_by_key(|&(r, _)| r);
    ranked.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::super::models::Port;
    use super::*;
    use serde_json::json;

    fn port(name: &str, tags: &[&str]) -> Port {
        super::super::models::port_from_value(&json!({
            "name": name, "folder": name, "source": "https://example.com/x.zip",
            "tags": tags,
        }))
        .unwrap()
    }

    fn port_rank(port: &Port, query: &str) -> Option<Rank> {
        rank(&port.name_lower, &port.tags_lower, &normalize(query))
    }

    fn search<'a>(pool: &[&'a Port], query: &str) -> Vec<&'a Port> {
        filter_and_sort(pool.iter().copied(), query, |p| (p.name_lower.as_str(), p.tags_lower.as_slice()))
    }

    #[test]
    fn tier_0_nom_commence_par_query() {
        assert_eq!(port_rank(&port("Ape Escape", &[]), "ape"), Some((0, 0)));
    }

    #[test]
    fn tier_1_nom_contient_query() {
        assert_eq!(port_rank(&port("Ape Escape", &[]), "escape"), Some((1, 0)));
    }

    #[test]
    fn tier_2_tag_commence_par_query() {
        assert_eq!(port_rank(&port("Ape Escape", &["Saru", "PS1"]), "ps1"), Some((2, 0)));
    }

    #[test]
    fn tier_3_tag_contient_query() {
        assert_eq!(port_rank(&port("Ape Escape", &["Platformer"]), "form"), Some((3, 0)));
    }

    #[test]
    fn tier_4_correspondance_floue_sous_sequence() {
        assert!(matches!(port_rank(&port("GoldenEye", &[]), "gde"), Some((4, _))));
    }

    #[test]
    fn aucune_correspondance_est_none() {
        assert_eq!(port_rank(&port("Ape Escape", &["Platformer"]), "zzz"), None);
    }

    #[test]
    fn query_vide_matche_tout_au_tier_0() {
        assert_eq!(port_rank(&port("Ape Escape", &[]), ""), Some((0, 0)));
    }

    #[test]
    fn plusieurs_mots_dans_n_importe_quel_ordre() {
        let shipwright = port("The Legend of Zelda: Ocarina of Time (Shipwright)", &[]);
        assert!(port_rank(&shipwright, "zelda ship").is_some());
        assert!(port_rank(&shipwright, "shipwright ocarina").is_some());
        assert!(port_rank(&shipwright, "zelda zzz").is_none());
    }

    #[test]
    fn plusieurs_mots_pire_palier_des_mots() {
        assert_eq!(port_rank(&port("Ape Escape", &["PS1"]), "ape ps1"), Some((2, 0)));
    }

    #[test]
    fn accents_ignores_dans_les_deux_sens() {
        assert!(port_rank(&port("Pokémon Emerald", &[]), "pokemon").is_some());
        assert!(port_rank(&port("Pokemon Emerald", &[]), "pokémon").is_some());
    }

    #[test]
    fn plus_petite_fenetre_et_non_premiere_occurrence() {
        assert_eq!(port_rank(&port("acronis cyber protect", &[]), "cp"), Some((4, 6)));
    }

    #[test]
    fn flou_le_plus_serre_passe_devant() {
        let tight = port("a_c", &[]);
        let loose = port("a__c", &[]);
        let result = search(&[&loose, &tight], "ac");
        assert_eq!(result[0].name, "a_c");
    }

    #[test]
    fn truncate_query_coupe_au_caractere_pres() {
        assert_eq!(truncate_query("zelda"), None);
        assert_eq!(truncate_query(&"a".repeat(MAX_QUERY_CHARS)), None);
        assert_eq!(truncate_query(&"a".repeat(MAX_QUERY_CHARS + 1)).unwrap().chars().count(), MAX_QUERY_CHARS);
        assert_eq!(truncate_query(&"é".repeat(MAX_QUERY_CHARS * 3)).unwrap(), "é".repeat(MAX_QUERY_CHARS));
    }

    #[test]
    fn filter_and_sort_est_stable_a_tier_egal() {
        let a = port("Alpha Game", &[]);
        let b = port("Alpha Racer", &[]);
        let result = search(&[&a, &b], "alpha");
        assert_eq!(result[0].name, "Alpha Game");
        assert_eq!(result[1].name, "Alpha Racer");
    }

    #[test]
    fn filter_and_sort_classe_les_tiers_dans_l_ordre() {
        let exact = port("Zeta", &["tag"]);
        let contains = port("Alpha Zeta Beta", &[]);
        let tag_match = port("Gamma", &["Zeta-ish"]);
        let result = search(&[&contains, &tag_match, &exact], "zeta");
        assert_eq!(result[0].name, "Zeta");
        assert_eq!(result[1].name, "Alpha Zeta Beta");
        assert_eq!(result[2].name, "Gamma");
    }

    #[test]
    fn requetes_extremes_sur_un_gros_catalogue_ne_paniquent_pas() {
        let names = ["Ape Escape", "GoldenEye", "Pikmin Recomp", "Portal Récomp", "Zelda: 🎮 Édition", "", "   ", "A", "アプリ", "Ω-Game"];
        let ports: Vec<Port> = (0..3000)
            .map(|i| {
                let tag = format!("tag{}", i % 7);
                port(&format!("{} {i}", names[i % names.len()]), &[tag.as_str(), "Platformer"])
            })
            .collect();
        let pool: Vec<&Port> = ports.iter().collect();

        let queries = [
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
        for query in &queries {
            assert!(search(&pool, query).len() <= pool.len());
        }
    }
}
