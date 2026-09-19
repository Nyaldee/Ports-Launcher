
pub const APP_VERSION: &str = env!("APP_BUILD_DATE");

pub fn is_newer_date(latest: &str, current: &str) -> bool {
    latest.get(..10).unwrap_or(latest) > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_date_detecte_une_date_posterieure() {
        assert!(is_newer_date("2026-09-18T09:33:01Z", "2026-09-17"));
    }

    #[test]
    fn is_newer_date_ignore_un_build_du_meme_jour() {
        assert!(!is_newer_date("2026-09-18T09:33:01Z", "2026-09-18"));
    }

    #[test]
    fn is_newer_date_gere_le_changement_d_annee() {
        assert!(is_newer_date("2027-01-01T00:00:00Z", "2026-12-31"));
    }
}
