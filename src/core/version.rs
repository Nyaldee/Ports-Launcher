
pub const APP_VERSION: &str = env!("APP_BUILD_DATE");

pub fn tag_is_newer(latest: &str, current: &str) -> bool {
    fn parse(tag: &str) -> Option<(u32, u32, u32)> {
        let mut parts = tag.split('/');
        let mm: u32 = parts.next()?.parse().ok()?;
        let dd: u32 = parts.next()?.parse().ok()?;
        let yy: u32 = parts.next()?.parse().ok()?;
        Some((yy, mm, dd))
    }
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest != current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_is_newer_detecte_une_date_posterieure() {
        assert!(tag_is_newer("09/15/26", "09/14/26"));
    }

    #[test]
    fn tag_is_newer_ignore_un_build_local_en_avance() {
        assert!(!tag_is_newer("09/14/26", "09/15/26"));
    }

    #[test]
    fn tag_is_newer_gere_le_changement_d_annee() {
        assert!(tag_is_newer("01/01/27", "12/31/26"));
    }

    #[test]
    fn tag_is_newer_retombe_sur_l_inegalite_si_format_inattendu() {
        assert!(tag_is_newer("v1.0", "09/15/26"));
        assert!(!tag_is_newer("09/15/26", "09/15/26"));
    }
}
