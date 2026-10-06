//! CSS minifier of build.rs (`#[path]`-included there; compiled into the crate for its tests only).
//! Removes comments, collapses whitespace and drops it only where CSS gives it no meaning: next to
//! `{ } ; ,` and after `:`. Never before `:` — `.a ::x` (descendant) is not `.a::x` (the
//! `minifier` crate gets this wrong: it hid the task table's date-picker icons, caught by the
//! visual compare) — and never around `( ) + - * / > ~`: `and (` is not the function `and(`,
//! `calc(1px + 2px)` needs its spaces. Strings and backslash escapes are copied as they are.

pub fn minify_css(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut space = false;
    // the last byte came from an escape (`\:`): it is part of an identifier, not punctuation
    let mut escaped = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            // a comment separates tokens like whitespace does
            i = src[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            space = true;
            continue;
        }
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
            space = true;
            i += 1;
            continue;
        }
        if space {
            space = false;
            let glue = |x: u8| matches!(x, b'{' | b'}' | b';' | b',');
            if out.last().is_some_and(|&p| escaped || (!glue(p) && p != b':')) && !glue(c) {
                out.push(b' ');
            }
        }
        let after_escape = std::mem::take(&mut escaped);
        match c {
            b'"' | b'\'' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != c {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                out.extend_from_slice(&b[start..i]);
            }
            b'\\' => {
                let end = (i + 2).min(b.len());
                out.extend_from_slice(&b[i..end]);
                i = end;
                escaped = true;
                continue;
            }
            _ => {
                if c == b'}' && out.last() == Some(&b';') && !after_escape {
                    out.pop();
                }
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("only ASCII bytes are dropped")
}

#[cfg(test)]
mod tests {
    use super::minify_css;

    #[test]
    fn keeps_every_meaningful_space() {
        let cases = [
            (".tkmain .sheet ::-webkit-calendar-picker-indicator { opacity: 0; }", ".tkmain .sheet ::-webkit-calendar-picker-indicator{opacity:0}"),
            ("a :hover , b:focus { color : red ; }", "a :hover,b:focus{color :red}"),
            ("@media (min-width: 900px) and (hover: none) { .a { x: calc(100% - 2 * var(--g)); } }", "@media (min-width:900px) and (hover:none){.a{x:calc(100% - 2 * var(--g))}}"),
            ("/* c */ .a /* d */ > .b + .c ~ .d {}", ".a > .b + .c ~ .d{}"),
            (".a::before { content: \"a ,  b /* no */ ;}\"; font: 12px 'x  y' }", ".a::before{content:\"a ,  b /* no */ ;}\";font:12px 'x  y'}"),
            (".md\\: flex {}\n\n", ".md\\: flex{}"), (".a\\{ b {}", ".a\\{ b{}"), ("a{x:\\;}", "a{x:\\;}"),
            ("a { margin: 0 auto !important; padding: max(6px, env(x)) }", "a{margin:0 auto !important;padding:max(6px,env(x))}"),
        ];
        for (src, want) in cases {
            assert_eq!(minify_css(src), want, "{src}");
        }
    }

    #[test]
    fn unterminated_input_ends_without_panicking() {
        assert_eq!(minify_css("a{content:\"x"), "a{content:\"x");
        assert_eq!(minify_css("a{} /* open"), "a{}");
        assert_eq!(minify_css("a\\"), "a\\");
    }
}
