
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::RECT;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetTextMetricsW, SelectObject, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CALCRECT, DT_NOPREFIX, DT_SINGLELINE, DT_WORDBREAK, FF_DONTCARE,
    FW_NORMAL, HDC, OUT_DEFAULT_PRECIS, TEXTMETRICW,
};

#[cfg(target_os = "windows")]
unsafe fn with_font<T>(family: &str, px: i32, f: impl FnOnce(HDC) -> T) -> Option<T> {
    let hdc = CreateCompatibleDC(None);
    if hdc.is_invalid() {
        return None;
    }
    let wide: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let hfont = CreateFontW(
        -px,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
        PCWSTR(wide.as_ptr()),
    );
    if hfont.is_invalid() {
        let _ = DeleteDC(hdc);
        return None;
    }
    let old = SelectObject(hdc, hfont.into());
    let result = f(hdc);
    SelectObject(hdc, old);
    let _ = DeleteObject(hfont.into());
    let _ = DeleteDC(hdc);
    Some(result)
}

#[cfg(target_os = "windows")]
pub fn linespace_for_size(family: &str, px: i32) -> i32 {
    unsafe {
        with_font(family, px, |hdc| {
            let mut tm = TEXTMETRICW::default();
            if GetTextMetricsW(hdc, &mut tm).as_bool() && tm.tmHeight > 0 {
                tm.tmHeight
            } else {
                px
            }
        })
        .unwrap_or(px)
    }
}

#[cfg(target_os = "windows")]
pub fn wrapped_text_height(family: &str, px: i32, text: &str, max_width_px: i32) -> i32 {
    if text.is_empty() {
        return linespace_for_size(family, px);
    }
    unsafe {
        with_font(family, px, |hdc| {
            let mut wide_text: Vec<u16> = text.encode_utf16().collect();
            let mut rect = RECT { left: 0, top: 0, right: max_width_px.max(1), bottom: 0 };
            DrawTextW(hdc, &mut wide_text, &mut rect, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
            let height = rect.bottom - rect.top;
            if height > 0 {
                height
            } else {
                linespace_for_size(family, px)
            }
        })
        .unwrap_or(px)
    }
}

#[cfg(target_os = "windows")]
unsafe fn measure_single_line(hdc: HDC, text: &str) -> i32 {
    let mut wide_text: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT { left: 0, top: 0, right: i16::MAX as i32, bottom: 0 };
    DrawTextW(hdc, &mut wide_text, &mut rect, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    (rect.right - rect.left).max(0)
}

#[cfg(target_os = "windows")]
pub fn max_text_width<'a>(family: &str, px: i32, texts: impl Iterator<Item = &'a str>) -> i32 {
    unsafe { with_font(family, px, |hdc| texts.map(|t| measure_single_line(hdc, t)).max().unwrap_or(0)).unwrap_or(0) }
}

#[cfg(target_os = "linux")]
fn cached_font(family: &str) -> Option<std::rc::Rc<fontdue::Font>> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    thread_local! {
        static CACHE: RefCell<HashMap<String, Option<Rc<fontdue::Font>>>> = RefCell::new(HashMap::new());
    }

    fn load(family: &str) -> Option<Rc<fontdue::Font>> {
        let output = std::process::Command::new("fc-match").args(["-f", "%{file}", family]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let bytes = std::fs::read(String::from_utf8(output.stdout).ok()?).ok()?;
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok().map(Rc::new)
    }

    CACHE.with(|cache| cache.borrow_mut().entry(family.to_string()).or_insert_with(|| load(family)).clone())
}

#[cfg(target_os = "linux")]
pub fn linespace_for_size(family: &str, px: i32) -> i32 {
    cached_font(family)
        .and_then(|font| font.horizontal_line_metrics(px as f32))
        .map(|m| (m.ascent - m.descent + m.line_gap).round() as i32)
        .filter(|&h| h > 0)
        .unwrap_or(px)
}

#[cfg(target_os = "linux")]
fn measure_single_line(font: &fontdue::Font, px: i32, text: &str) -> i32 {
    text.chars().map(|c| font.metrics(c, px as f32).advance_width).sum::<f32>().round() as i32
}

#[cfg(target_os = "linux")]
pub fn max_text_width<'a>(family: &str, px: i32, texts: impl Iterator<Item = &'a str>) -> i32 {
    let Some(font) = cached_font(family) else { return 0 };
    texts.map(|t| measure_single_line(&font, px, t)).max().unwrap_or(0)
}

#[cfg(target_os = "linux")]
pub fn wrapped_text_height(family: &str, px: i32, text: &str, max_width_px: i32) -> i32 {
    let Some(font) = cached_font(family) else { return px };
    let linespace = linespace_for_size(family, px).max(1);
    let max_width = max_width_px.max(1) as f32;
    let space_width = font.metrics(' ', px as f32).advance_width;

    let mut lines = 1i32;
    let mut current_width = 0.0f32;
    for word in text.split_whitespace() {
        let word_width: f32 = word.chars().map(|c| font.metrics(c, px as f32).advance_width).sum();
        if current_width > 0.0 && current_width + space_width + word_width > max_width {
            lines += 1;
            current_width = word_width;
        } else if current_width > 0.0 {
            current_width += space_width + word_width;
        } else {
            current_width = word_width;
        }
    }
    (lines * linespace).max(px)
}

pub fn solve_font_for_height(family: &str, target_linespace: i32) -> (i32, i32) {
    const MIN_SIZE_PX: i32 = 8;
    const MAX_SIZE_PX: i32 = 500;
    let target = target_linespace.max(1);

    let mut size = ((target as f32 * 0.75).round() as i32).clamp(MIN_SIZE_PX, MAX_SIZE_PX);
    let mut linespace = linespace_for_size(family, size);

    if linespace > target {
        while size > MIN_SIZE_PX {
            size -= 1;
            linespace = linespace_for_size(family, size);
            if linespace <= target {
                break;
            }
        }
        return (size, linespace);
    }
    while size < MAX_SIZE_PX {
        let next_linespace = linespace_for_size(family, size + 1);
        if next_linespace > target {
            break;
        }
        size += 1;
        linespace = next_linespace;
    }
    (size, linespace)
}

pub fn longest_word_width(family: &str, px: i32, text: &str) -> i32 {
    max_text_width(family, px, text.split_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_width(family: &str, px: i32, text: &str) -> i32 {
        max_text_width(family, px, std::iter::once(text))
    }

    #[test]
    fn converge_vers_un_interligne_proche_de_la_cible() {
        let (size, linespace) = solve_font_for_height("Segoe UI", 22);
        assert!(size >= 8);
        assert!((linespace - 22).abs() <= 3, "linespace={linespace}");
    }

    #[test]
    fn police_invalide_ne_plante_pas() {
        let (size, _) = solve_font_for_height("Cette Police N'Existe Sûrement Pas XYZ", 20);
        assert!(size >= 8);
    }

    #[test]
    fn respecte_le_plancher_de_8px() {
        let (size, _) = solve_font_for_height("Segoe UI", 1);
        assert!(size >= 8);
    }

    #[test]
    fn wrapped_text_height_grandit_avec_un_texte_plus_long() {
        let short = wrapped_text_height("Segoe UI", 14, "Short message.", 300);
        let long = wrapped_text_height(
            "Segoe UI",
            14,
            "This is a much longer message that should wrap across several lines once constrained to a narrow width, taking up noticeably more vertical space than the short one above.",
            300,
        );
        assert!(long > short, "short={short} long={long}");
    }

    #[test]
    fn wrapped_text_height_texte_vide_vaut_une_ligne() {
        assert_eq!(wrapped_text_height("Segoe UI", 14, "", 300), linespace_for_size("Segoe UI", 14));
    }

    #[test]
    fn wrapped_text_height_police_invalide_ne_plante_pas() {
        assert!(wrapped_text_height("Cette Police N'Existe Sûrement Pas XYZ", 14, "some text", 300) > 0);
    }

    #[test]
    fn text_width_grandit_avec_un_texte_plus_long() {
        let short = text_width("Segoe UI", 14, "short.exe");
        let long = text_width("Segoe UI", 14, "a_much_longer_executable_name_indeed.exe");
        assert!(long > short, "short={short} long={long}");
    }

    #[test]
    fn text_width_police_invalide_ne_plante_pas() {
        assert!(text_width("Cette Police N'Existe Sûrement Pas XYZ", 14, "some text") > 0);
    }

    #[test]
    fn longest_word_width_ignore_les_mots_courts_autour() {
        let long_word = "ligne-ligne-ligne-ligne-ligne-ligne-ligne-ligne.zip";
        let whole = format!("word1 word2 {long_word} word3");
        assert_eq!(longest_word_width("Segoe UI", 14, &whole), text_width("Segoe UI", 14, long_word));
    }

    #[test]
    fn longest_word_width_texte_vide_est_zero() {
        assert_eq!(longest_word_width("Segoe UI", 14, ""), 0);
        assert_eq!(longest_word_width("Segoe UI", 14, "   "), 0);
    }
}
