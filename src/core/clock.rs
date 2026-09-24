
#[cfg(target_os = "windows")]
pub fn format_now() -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
    use windows::Win32::System::SystemInformation::GetLocalTime;

    let st = unsafe { GetLocalTime() };
    let mut buf = [0u16; 64];
    let len = unsafe { GetTimeFormatEx(PCWSTR::null(), TIME_NOSECONDS, Some(&st as *const _), PCWSTR::null(), Some(&mut buf)) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)])
}

#[cfg(target_os = "linux")]
pub fn format_now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

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

#[cfg(target_os = "linux")]
pub fn format_date(local: chrono::DateTime<chrono::Local>) -> String {
    local.format("%b %-d, %Y").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_now_renvoie_quelque_chose_de_plausible() {
        let s = format_now();
        assert!(!s.is_empty());
        assert!(s.len() < 20);
    }

    #[test]
    fn format_date_suit_le_gabarit_mois_jour_annee() {
        let local = chrono::DateTime::parse_from_rfc3339("2026-08-26T20:32:28Z").unwrap().with_timezone(&chrono::Local);
        let s = format_date(local);
        assert!(s.contains(", 2026"), "s={s}");
        assert!(s.len() < 20);
    }
}
