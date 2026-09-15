//! Fluent translations embedded at compile time (D9). Two languages, one bundle
//! each, shared read-only across requests (`concurrent::FluentBundle`).

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::sync::Arc;
use unic_langid::{LanguageIdentifier, langid};

pub const LANGS: &[&str] = &["de", "en"];

pub struct I18n {
    de: FluentBundle<FluentResource>,
    en: FluentBundle<FluentResource>,
}

fn bundle(lang: LanguageIdentifier, ftl: &str) -> FluentBundle<FluentResource> {
    let res = FluentResource::try_new(ftl.to_string()).unwrap_or_else(|(_, errs)| {
        panic!("invalid .ftl for {lang}: {errs:?}");
    });
    let mut b = FluentBundle::new_concurrent(vec![lang]);
    // Isolation marks (U+2068/2069) around placeables are for bidi text; our two
    // languages are LTR and the marks show up as junk in <title>/attributes.
    b.set_use_isolating(false);
    b.add_resource(res).expect("duplicate message ids in .ftl");
    b
}

impl I18n {
    pub fn load() -> Arc<Self> {
        Arc::new(Self {
            de: bundle(langid!("de"), include_str!("../locales/de.ftl")),
            en: bundle(langid!("en"), include_str!("../locales/en.ftl")),
        })
    }

    fn bundle_for(&self, lang: &str) -> &FluentBundle<FluentResource> {
        match lang {
            "en" => &self.en,
            _ => &self.de,
        }
    }

    pub fn t(&self, lang: &str, key: &str) -> String {
        self.t_args(lang, key, None)
    }

    pub fn t_args(&self, lang: &str, key: &str, args: Option<&FluentArgs>) -> String {
        let b = self.bundle_for(lang);
        let Some(msg) = b.get_message(key) else {
            tracing::warn!("missing translation {lang}/{key}");
            return format!("⟦{key}⟧");
        };
        let Some(pattern) = msg.value() else {
            return format!("⟦{key}⟧");
        };
        let mut errors = vec![];
        let s = b.format_pattern(pattern, args, &mut errors).into_owned();
        if !errors.is_empty() {
            tracing::warn!("translation {lang}/{key}: {errors:?}");
        }
        s
    }

    /// Message with one named argument.
    pub fn t1(&self, lang: &str, key: &str, name: &str, value: impl Into<FluentValue<'static>>) -> String {
        let mut args = FluentArgs::new();
        args.set(name.to_string(), value.into());
        self.t_args(lang, key, Some(&args))
    }

    /// Every message id in the German bundle must exist in English (and vice versa).
    #[cfg(test)]
    pub fn missing_keys(&self) -> Vec<String> {
        let de: std::collections::BTreeSet<String> = include_str!("../locales/de.ftl").lines().filter_map(|l| l.split_once(" = ").map(|(k, _)| k.trim().to_string())).filter(|k| !k.starts_with('#') && !k.starts_with('.')).collect();
        let en: std::collections::BTreeSet<String> = include_str!("../locales/en.ftl").lines().filter_map(|l| l.split_once(" = ").map(|(k, _)| k.trim().to_string())).filter(|k| !k.starts_with('#') && !k.starts_with('.')).collect();
        de.symmetric_difference(&en).cloned().collect()
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
    fn negotiation() {
        assert_eq!(negotiate(Some("en-US,en;q=0.9,de;q=0.8"), "de"), "en");
        assert_eq!(negotiate(Some("fr-FR,fr;q=0.9"), "de"), "de");
        assert_eq!(negotiate(None, "en"), "en");
        assert_eq!(negotiate(None, "xx"), "de");
    }
}
