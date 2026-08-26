//! Horloge de la barre de recherche (`themes.json: "show_clock"`) -- format
//! court de l'heure réglé par l'utilisateur Windows (Paramètres > Heure et
//! langue), jamais un format 24h codé en dur.

use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::PCWSTR;

/// Heure locale actuelle, formatée selon la locale/le format court de la
/// machine -- rafraîchie chaque seconde par un `slint::Timer` (voir main.rs).
pub fn format_now() -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_now_renvoie_quelque_chose_de_plausible() {
        // La chaîne exacte dépend de la locale/du format court de la machine
        // (voir le commentaire du module) : seul un résultat non vide et
        // raisonnablement court est vérifiable ici.
        let s = format_now();
        assert!(!s.is_empty());
        assert!(s.len() < 20);
    }
}
