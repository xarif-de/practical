//! HTTP handlers per page or API area; each module's `router()` is merged in main.rs.

pub mod api;
pub mod assets;
pub mod calendar;
pub mod connect;
pub mod contacts;
pub mod dev;
pub mod editor;
pub mod history;
pub mod more;
pub mod pages;
pub mod settings;
pub mod tasks;

/// `Content-Disposition` of a download named `name` (RFC 6266): `filename` an ASCII stand-in (each character outside
/// printable ASCII, `"` and `\` → `_`), plus `filename*` = the name itself, UTF-8 percent-encoded (RFC 8187), when the
/// two differ. The name as it was (901d09e) sent its UTF-8 bytes raw (browsers read header bytes as Latin-1) and a
/// `"` unescaped, which ends the quoted string early (T: it::api_more::download_names_outside_ascii…).
pub fn attachment(name: &str) -> String {
    let ascii: String = name.chars().map(|c| if (' '..='~').contains(&c) && c != '"' && c != '\\' { c } else { '_' }).collect();
    if ascii == name {
        return format!("attachment; filename=\"{ascii}\"");
    }
    let mut enc = String::with_capacity(name.len() * 3);
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{enc}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn attachment_names_are_ascii_with_the_real_name_beside() {
        assert_eq!(super::attachment("Privat.ics"), "attachment; filename=\"Privat.ics\"");
        assert_eq!(super::attachment("a b-c_(1).ics"), "attachment; filename=\"a b-c_(1).ics\"");
        assert_eq!(super::attachment("Grüße.vcf"), "attachment; filename=\"Gr__e.vcf\"; filename*=UTF-8''Gr%C3%BC%C3%9Fe.vcf");
        assert_eq!(super::attachment("a\"b\\c\u{7f}\n.vcf"), "attachment; filename=\"a_b_c__.vcf\"; filename*=UTF-8''a%22b%5Cc%7F%0A.vcf");
        assert_eq!(super::attachment("😀"), "attachment; filename=\"_\"; filename*=UTF-8''%F0%9F%98%80");
        for n in ["Grüße.vcf", "a\"b\\c\u{7f}\n.vcf", "😀 x"] {
            assert!(axum::http::HeaderValue::from_str(&super::attachment(n)).is_ok(), "{n}: visible ASCII only");
        }
    }
}
