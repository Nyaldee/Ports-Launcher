
use std::time::Duration;

const USER_AGENT: &str = concat!("Ports-Launcher/", env!("APP_BUILD_DATE"));

pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).user_agent(USER_AGENT).https_only(true).build().into()
}

pub fn api_agent() -> ureq::Agent {
    agent(Duration::from_secs(30))
}

pub fn encode_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        if c.is_ascii_graphic() && !matches!(c, '"' | '<' | '>' | '\\' | '^' | '`' | '{' | '|' | '}') {
            out.push(c);
        } else {
            let mut buf = [0; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

pub fn decode_url(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_les_caracteres_hors_ascii_comme_un_navigateur() {
        assert_eq!(encode_url("https://x/extra/NANACA†CRASH!!.Windows.zip"), "https://x/extra/NANACA%E2%80%A0CRASH!!.Windows.zip");
        assert_eq!(encode_url("https://x/a b.zip?q=1&r=2#f"), "https://x/a%20b.zip?q=1&r=2#f");
    }

    #[test]
    fn ne_reencode_pas_une_url_deja_encodee() {
        assert_eq!(encode_url("https://x/a%20b.zip"), "https://x/a%20b.zip");
    }

    #[test]
    fn decode_le_nom_de_fichier() {
        assert_eq!(decode_url("NANACA%E2%80%A0CRASH!!.Windows.zip"), "NANACA†CRASH!!.Windows.zip");
        assert_eq!(decode_url("a%20b%zz.zip"), "a b%zz.zip");
    }
}
