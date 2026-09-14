//! Horloge de la barre de recherche (`themes.json: "show_clock"`) et
//! formatage de dates locales pour l'affichage (voir `format_date`).

/// Heure locale actuelle, formatée selon le format court de l'utilisateur
/// Windows (Paramètres > Heure et langue), jamais un format 24h codé en
/// dur -- rafraîchie chaque seconde par un `slint::Timer` (voir main.rs).
#[cfg(target_os = "windows")]
pub fn format_now() -> String {
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::core::PCWSTR;

    let st = unsafe { GetLocalTime() };

    let mut buf = [0u16; 64];
    let len = unsafe {
        GetTimeFormatEx(PCWSTR::null(), TIME_NOSECONDS, Some(&st as *const _), PCWSTR::null(), Some(&mut buf))
    };
    if len <= 0 {
        return String::new();
    }
    // `len` compte le zéro terminateur -- exclu explicitement plutôt que de
    // laisser String::from_utf16_lossy le convertir en caractère visible.
    String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)])
}

/// Pas d'équivalent système unique du format court Windows sous Linux
/// (chaque environnement de bureau range ce réglage à sa façon) -- 24h
/// (`chrono`, déjà une dépendance du projet) en repli fixe, comme le
/// double-clic dans `linux_chrome::double_click_time_ms`.
#[cfg(target_os = "linux")]
pub fn format_now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

/// Date locale abrégée ("Oct 3, 2023") de `local`, pour l'affichage de "Last
/// played" (voir `core::models::InstalledInfo::last_played_at`). Prend un
/// `DateTime` déjà résolu plutôt qu'un horodatage brut à parser : l'appelant
/// (voir `app::playtime::format_last_played`) en a de toute façon déjà besoin
/// pour distinguer "aujourd'hui" d'une date passée, pas la peine de parser
/// deux fois. Gabarit ("MMM d, yyyy") fixé explicitement plutôt que
/// `DATE_SHORTDATE` (qui rendrait un format numérique façon "10/3/2023",
/// ambigu jour/mois selon la locale) -- le nom du mois abrégé ("MMM") reste
/// résolu par la locale système, seul l'ORDRE des champs est figé. Chaîne
/// vide en cas d'échec de l'API, comme `format_now()`.
#[cfg(target_os = "windows")]
pub fn format_date(local: chrono::DateTime<chrono::Local>) -> String {
    use chrono::{Datelike, Timelike};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::{GetDateFormatEx, ENUM_DATE_FORMATS_FLAGS};

    let st = SYSTEMTIME {
        wYear: local.year() as u16,
        wMonth: local.month() as u16,
        wDayOfWeek: local.weekday().num_days_from_sunday() as u16,
        wDay: local.day() as u16,
        wHour: local.hour() as u16,
        wMinute: local.minute() as u16,
        wSecond: local.second() as u16,
        wMilliseconds: 0,
    };

    // dwFlags à 0 -- requis par l'API dès que lpFormat est fourni (les
    // indicateurs DATE_SHORTDATE/DATE_LONGDATE ne s'appliquent qu'en son
    // absence).
    let format: Vec<u16> = "MMM d, yyyy".encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 64];
    let len = unsafe {
        GetDateFormatEx(
            PCWSTR::null(),
            ENUM_DATE_FORMATS_FLAGS(0),
            Some(&st as *const _),
            PCWSTR(format.as_ptr()),
            Some(&mut buf),
            PCWSTR::null(),
        )
    };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)])
}

/// Pas d'équivalent système unique du format court Windows sous Linux --
/// même repli fixe que `format_now()`, mois abrégé toujours en anglais
/// (`chrono` sans le nom du mois localisé) plutôt qu'un format numérique
/// ambigu jour/mois selon la locale.
#[cfg(target_os = "linux")]
pub fn format_date(local: chrono::DateTime<chrono::Local>) -> String {
    local.format("%b %-d, %Y").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_now_renvoie_quelque_chose_de_plausible() {
        // La chaîne exacte dépend de la locale/du format court de la
        // machine (voir le commentaire du module) : seul un résultat non
        // vide et raisonnablement court est vérifiable ici.
        let s = format_now();
        assert!(!s.is_empty());
        assert!(s.len() < 20);
    }

    #[test]
    fn format_date_suit_le_gabarit_mois_jour_annee() {
        // Le nom du mois suit la locale système (voir le commentaire de la
        // fonction) : seule la FORME est vérifiable ici, pas le texte exact.
        let local = chrono::DateTime::parse_from_rfc3339("2026-08-26T20:32:28Z").unwrap().with_timezone(&chrono::Local);
        let s = format_date(local);
        assert!(s.contains(", 2026"), "s={s}");
        assert!(s.len() < 20);
    }
}
