//! HTML generation for the calendar views; markup and class names are the ones style.css is written for.

use crate::domain::events::{EventDetail, Instance};
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;
use std::fmt::Write;

mod month;
mod week;
mod text;
mod day;
pub use month::*;
pub use week::*;
pub use text::*;
pub use day::*;

/// Words and dates of the server-rendered views in the page language (Fluent keys:
/// `mon-N`, `dow-N`, `dowl-N`, `date-*`, `v-*`).
pub struct L {
    pub lang: String,
    i18n: std::sync::Arc<crate::i18n::I18n>,
}
impl L {
    pub fn new(lang: &str, i18n: std::sync::Arc<crate::i18n::I18n>) -> Self {
        Self { lang: lang.to_string(), i18n }
    }
    pub fn t(&self, key: &str) -> String {
        self.i18n.t(&self.lang, key)
    }
    /// Message with named arguments (numbers stay numbers, so plural rules apply).
    pub fn ta(&self, key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]) -> String {
        let mut a = fluent_bundle::FluentArgs::new();
        for (k, v) in args {
            a.set(k.to_string(), v.clone());
        }
        self.i18n.t_args(&self.lang, key, Some(&a))
    }
    /// Message with the count `n` (its plural form: "1 Termin", "2 Termine").
    pub fn tn(&self, key: &str, n: impl TryInto<i64>) -> String {
        self.ta(key, &[("n", n.try_into().unwrap_or(i64::MAX).into())])
    }
    /// short weekday name, 0 = Monday
    pub fn dow_i(&self, i: usize) -> String {
        self.t(&format!("dow-{}", i + 1))
    }
    pub fn dowl_i(&self, i: usize) -> String {
        self.t(&format!("dowl-{}", i + 1))
    }
    pub fn dow(&self, d: NaiveDate) -> String {
        self.dow_i(d.weekday().num_days_from_monday() as usize)
    }
    pub fn dowl(&self, d: NaiveDate) -> String {
        self.dowl_i(d.weekday().num_days_from_monday() as usize)
    }
    pub fn mon(&self, m: u32) -> String {
        self.t(&format!("mon-{m}"))
    }
    pub fn mon3(&self, m: u32) -> String {
        self.mon(m).chars().take(3).collect()
    }
    pub fn long_date(&self, d: NaiveDate) -> String {
        self.ta("date-long", &[("dow", self.dowl(d).into()), ("day", i64::from(d.day()).into()), ("month", self.mon(d.month()).into())])
    }
    pub fn dm(&self, d: NaiveDate) -> String {
        self.ta("date-dm", &[("d", i64::from(d.day()).into()), ("m", i64::from(d.month()).into())])
    }
    /// a full date as the language writes it (chrono format from `date-fmt`)
    pub fn date(&self, d: NaiveDate) -> String {
        d.format(&self.t("date-fmt")).to_string()
    }
    pub fn week_title(&self, d: NaiveDate) -> String {
        self.ta("date-week", &[("week", i64::from(d.iso_week().week()).into()), ("year", i64::from(d.iso_week().year()).into())])
    }
    pub fn week_short(&self, w: u32) -> String {
        self.ta("date-week-short", &[("week", i64::from(w).into())])
    }
}

/// Named holidays by day (`domain::holidays::marks`); empty when the user switched them off.
pub type Hols = HashMap<NaiveDate, Vec<crate::domain::holidays::Mark>>;

/// Names of holiday marks, one per name: the regions of blocks with the same name are joined
/// ("Herbstferien (HE, SH)"); one that applies in every chosen region needs none.
fn joined<'a>(marks: impl Iterator<Item = &'a crate::domain::holidays::Mark>) -> Vec<String> {
    let mut out: Vec<(String, Option<Vec<String>>)> = vec![]; // None: applies everywhere
    for m in marks {
        match out.iter_mut().find(|(n, _)| *n == m.name) {
            Some((_, t)) => {
                if m.tags.is_empty() {
                    *t = None;
                } else if let Some(t) = t {
                    for x in &m.tags {
                        if !t.contains(x) {
                            t.push(x.clone());
                        }
                    }
                }
            }
            None => out.push((m.name.clone(), Some(m.tags.clone()).filter(|t| !t.is_empty()))),
        }
    }
    out.into_iter().map(|(n, t)| match t { Some(t) => format!("{n} ({})", t.join(", ")), None => n }).collect()
}

/// The holidays of a day as labels: public in red, school holidays (only with `school`) in amber.
fn hol_labels(hol: &Hols, d: NaiveDate, tag: &str, school: bool) -> String {
    let v = hol.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
    let public = joined(v.iter().filter(|m| !m.school)).into_iter().map(|n| format!("<{tag} class=\"hday\">{}</{tag}>", e(&n)));
    let fer = joined(v.iter().filter(|m| school && m.school)).into_iter().map(|n| format!("<{tag} class=\"hday school\">{}</{tag}>", e(&n)));
    public.chain(fer).collect()
}

/// Classes of a day cell: `hol` (public holiday, red number), `fer` (school holidays, tinted).
fn hol_class(hol: &Hols, d: NaiveDate) -> String {
    let v = hol.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
    format!("{}{}", if v.iter().any(|m| !m.school) { " hol" } else { "" }, if v.iter().any(|m| m.school) { " fer" } else { "" })
}

/// JSON for a `<script type="application/json">` block: `<` can only occur inside JSON strings,
/// and as `\u003c` it can no longer close the block (a task title `</script>…` from a shared
/// calendar would otherwise inject markup).
pub fn script_json(json: String) -> String {
    json.replace('<', "\\u003c")
}

/// Whether a URI from the data may become an `href`: a `javascript:` or `vbscript:` link runs
/// script in the page, `data:` shows foreign markup. Every other scheme (http(s), mailto, tel,
/// geo, a conference app's own) and a relative reference stays a link. The URL parser drops tab
/// and newline anywhere and C0 controls and spaces around the URL before it reads the scheme, so
/// those are not counted here either. app.js `lkSafe` applies the same rule.
pub fn safe_href(uri: &str) -> bool {
    let s: String = uri.chars().filter(|c| !c.is_ascii_control() && *c != ' ').take(11).collect::<String>().to_ascii_lowercase();
    !["javascript:", "vbscript:", "data:"].iter().any(|p| s.starts_with(p))
}

/// `e` as a `Display`: escapes while it is written, without a String of its own per value.
pub struct E<'a>(pub &'a str);

impl std::fmt::Display for E<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut rest = self.0;
        while let Some(at) = rest.find(['&', '<', '>', '"', '\'']) {
            f.write_str(&rest[..at])?;
            f.write_str(match rest.as_bytes()[at] {
                b'&' => "&amp;",
                b'<' => "&lt;",
                b'>' => "&gt;",
                b'"' => "&quot;",
                _ => "&#39;",
            })?;
            rest = &rest[at + 1..];
        }
        f.write_str(rest)
    }
}

/// `e(&i.key())` without building the key: "/" needs no escaping, so the parts escape on their own.
struct EKey<'a>(&'a Instance);

impl std::fmt::Display for EKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", E(&self.0.principal), E(&self.0.cal_id), E(&self.0.object_id))
    }
}

pub fn e(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests;
