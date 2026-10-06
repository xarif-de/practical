//! Fluent translations embedded at compile time (D9). One bundle per language in LANGS,
//! shared read-only across requests (`concurrent::FluentBundle`). A key a language lacks
//! falls back to English, then German, so a new language can grow file by file; the
//! test below keeps every shipped file complete.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::sync::Arc;
use unic_langid::LanguageIdentifier;

/// Languages of the UI, in the order of the language choice. Adding one: a `locales/<lang>.ftl`
/// here and in SOURCES, NATIVE, LOCALE, and the region names (`tools/gen_regions.py` LANGS).
pub const LANGS: &[&str] = &["de", "en", "fr", "es"];

/// Native names for the language choice (never translated: a reader must find their own).
pub const NATIVE: &[(&str, &str)] = &[("de", "Deutsch"), ("en", "English"), ("fr", "Français"), ("es", "Español")];

/// BCP 47 locale per language for date formatting in the browser (`Intl`).
pub const LOCALE: &[(&str, &str)] = &[("de", "de-DE"), ("en", "en-GB"), ("fr", "fr-FR"), ("es", "es-ES")];

const SOURCES: &[(&str, &str)] = &[
    ("de", include_str!("../locales/de.ftl")),
    ("en", include_str!("../locales/en.ftl")),
    ("fr", include_str!("../locales/fr.ftl")),
    ("es", include_str!("../locales/es.ftl")),
];

pub struct I18n {
    bundles: Vec<(&'static str, FluentBundle<FluentResource>)>,
}

fn bundle(lang: &str, ftl: &str) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = lang.parse().expect("language id");
    let res = FluentResource::try_new(ftl.to_string()).unwrap_or_else(|(_, errs)| {
        panic!("invalid .ftl for {lang}: {errs:?}");
    });
    let mut b = FluentBundle::new_concurrent(vec![id]);
    // Isolation marks (U+2068/2069) around placeables are for bidi text; our languages
    // are LTR and the marks show up as junk in <title>/attributes.
    b.set_use_isolating(false);
    b.add_resource(res).expect("duplicate message ids in .ftl");
    b
}

/// Message ids of one .ftl source (continuation lines and comments skipped).
fn keys_of(ftl: &str) -> Vec<String> {
    ftl.lines()
        .filter(|l| !l.starts_with(' ') && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = ").or_else(|| l.strip_suffix(" =").map(|k| (k, ""))).map(|(k, _)| k.trim().to_string()))
        .filter(|k| !k.is_empty() && !k.contains(' '))
        .collect()
}

pub fn supported(lang: &str) -> bool {
    LANGS.contains(&lang)
}

pub fn locale(lang: &str) -> &'static str {
    LOCALE.iter().find(|(l, _)| *l == lang).map(|(_, x)| *x).unwrap_or("de-DE")
}

impl I18n {
    pub fn load() -> Arc<Self> {
        Arc::new(Self { bundles: SOURCES.iter().map(|(l, src)| (*l, bundle(l, src))).collect() })
    }

    fn get(&self, lang: &str) -> Option<&FluentBundle<FluentResource>> {
        self.bundles.iter().find(|(l, _)| *l == lang).map(|(_, b)| b)
    }

    pub fn t(&self, lang: &str, key: &str) -> String {
        self.t_args(lang, key, None)
    }

    pub fn t_args(&self, lang: &str, key: &str, args: Option<&FluentArgs>) -> String {
        // the asked language, then English, then German
        for l in [lang, "en", "de"] {
            let Some(b) = self.get(l) else { continue };
            let Some(pattern) = b.get_message(key).and_then(|m| m.value()) else { continue };
            if l != lang {
                tracing::debug!("translation {lang}/{key} missing, using {l}");
            }
            let mut errors = vec![];
            let s = b.format_pattern(pattern, args, &mut errors).into_owned();
            if !errors.is_empty() {
                tracing::warn!("translation {l}/{key}: {errors:?}");
            }
            return s;
        }
        tracing::warn!("missing translation {lang}/{key}");
        format!("⟦{key}⟧")
    }

    /// Message with one named argument.
    pub fn t1(&self, lang: &str, key: &str, name: &str, value: impl Into<FluentValue<'static>>) -> String {
        let mut args = FluentArgs::new();
        args.set(name.to_string(), value.into());
        self.t_args(lang, key, Some(&args))
    }

    /// Every message whose id starts with `prefix`, formatted in `lang` — the strings app.js
    /// needs (`js-*`; build.rs `i18n_js` ships them as /assets/i18n-<lang>.js). Placeholders there are `%name`.
    pub fn with_prefix(&self, lang: &str, prefix: &str) -> Vec<(String, String)> {
        keys_of(SOURCES[0].1)
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .map(|k| {
                let v = self.t(lang, &k);
                (k, v)
            })
            .collect()
    }

    /// The js-* strings of `lang` as a JSON object — what build.rs writes into /assets/i18n-<lang>.js
    /// (test `i18n_assets_are_the_runtime_strings`).
    #[cfg(test)]
    pub fn js_json(&self, lang: &str) -> serde_json::Value {
        serde_json::Value::Object(self.with_prefix(lang, "js-").into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect())
    }

    /// Message ids missing in a language (or extra there), compared with German.
    #[cfg(test)]
    pub fn missing_keys(&self) -> Vec<String> {
        let de: std::collections::BTreeSet<String> = keys_of(SOURCES[0].1).into_iter().collect();
        let mut out = vec![];
        for (l, src) in &SOURCES[1..] {
            let other: std::collections::BTreeSet<String> = keys_of(src).into_iter().collect();
            out.extend(de.symmetric_difference(&other).map(|k| format!("{l}:{k}")));
        }
        out
    }
}

/// Pick a supported language from an Accept-Language header value.
pub fn negotiate(accept_language: Option<&str>, default: &str) -> String {
    if let Some(al) = accept_language {
        for part in al.split(',') {
            let tag = part.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            let primary = tag.split('-').next().unwrap_or("");
            if LANGS.contains(&primary) {
                return primary.to_string();
            }
        }
    }
    if LANGS.contains(&default) { default.to_string() } else { "de".to_string() }
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_load_and_are_symmetric() {
        let i = I18n::load();
        assert_eq!(i.missing_keys(), Vec::<String>::new());
        assert_eq!(i.t("de", "nav-calendar"), "Kalender");
        assert_eq!(i.t("en", "nav-calendar"), "Calendar");
    }

    #[test]
    fn every_language_is_wired() {
        for l in LANGS {
            assert!(SOURCES.iter().any(|(s, _)| s == l), "{l} has no .ftl");
            assert!(NATIVE.iter().any(|(s, _)| s == l), "{l} has no native name");
            assert!(LOCALE.iter().any(|(s, _)| s == l), "{l} has no locale");
        }
    }

    #[test]
    fn negotiation() {
        assert_eq!(negotiate(Some("en-US,en;q=0.9,de;q=0.8"), "de"), "en");
        assert_eq!(negotiate(Some("fr-FR,fr;q=0.9"), "de"), "fr");
        assert_eq!(negotiate(Some("pt-BR,pt;q=0.9"), "de"), "de");
        assert_eq!(negotiate(None, "en"), "en");
        assert_eq!(negotiate(None, "xx"), "de");
    }
}
