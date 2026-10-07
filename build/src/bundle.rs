//! The shipped app.js and style.css are joined from part files in assets/js/ and assets/css/, in the order
//! of `BUNDLES` (classic scripts joined as text: no module system; the order is the cascade and the run
//! order). One table for both readers: build.rs (`#[path]`-included, minifies the join) and development
//! mode (routes/assets.rs serves the join uncached).

use std::path::Path;

pub enum Part {
    File(&'static str),
    /// `open`, the parts, `close`, each on a line of its own: parts that share ONE function scope (an IIFE cut
    /// into fragments; a fragment alone is no script)
    Wrap(&'static str, &'static [Part], &'static str),
}
use Part::{File, Wrap};

const IIFE: &str = "(function () {";
const END: &str = "})();";
/// The editor's function: run once at load (it returns at once on a page without #edform) and again by a page that
/// loads the dialog later (`practical.editorInit`: the settings' categories, js/shell/settings-pages.js)
const ED_OPEN: &str = "practical.editorInit = function () {";
const ED_END: &str = "};\npractical.editorInit();";

pub const BUNDLES: &[(&str, &[Part])] = &[
    ("app.js", &[
        File("js/core.js"),
        File("js/prefetch.js"),
        File("js/req.js"),
        Wrap(IIFE, &[File("js/shell/base.js"), File("js/shell/settings-pages.js"), File("js/shell/popovers-stream.js")], END),
        Wrap(IIFE, &[File("js/colors/picker.js"), File("js/colors/fields.js"), File("js/colors/optimise.js")], END),
        File("js/calendar.js"),
        Wrap(ED_OPEN, &[
            File("js/editor/setup.js"),
            File("js/editor/form.js"),
            File("js/editor/categories.js"),
            File("js/editor/rule-editor.js"),
            File("js/editor/reminders.js"),
            File("js/editor/status.js"),
            File("js/editor/note.js"),
            File("js/editor/people.js"),
            File("js/editor/links.js"),
            File("js/editor/location.js"),
            File("js/editor/map.js"),
            File("js/editor/recurrence.js"),
            File("js/editor/strip.js"),
            File("js/editor/cards.js"),
            File("js/editor/fill.js"),
            File("js/editor/templates.js"),
            File("js/editor/solo.js"),
            File("js/editor/wiring.js"),
        ], ED_END),
        File("js/search.js"),
        File("js/contacts.js"),
        File("js/tasks.js"),
        File("js/settings.js"),
    ]),
    ("style.css", &[
        File("css/base.css"),
        File("css/frame.css"),
        File("css/grid.css"),
        File("css/chrome.css"),
        File("css/dialogs.css"),
        File("css/generic.css"),
        File("css/views.css"),
        File("css/icon-buttons.css"),
        File("css/card-form.css"),
        File("css/reminders.css"),
        File("css/recurrence.css"),
        File("css/editor-cards.css"),
        File("css/links-location.css"),
        File("css/tasks.css"),
        File("css/settings.css"),
        File("css/color-picker.css"),
        File("css/color-optimiser.css"),
    ]),
];

/// The source of asset `name` in `dir`: a bundle joined from its parts (each ends with a newline: a part
/// ending in a `//` comment must not swallow the next one's first line), any other name the file itself.
/// An error names the part that failed and keeps its kind.
pub fn read(dir: &Path, name: &str) -> std::io::Result<Vec<u8>> {
    let Some((_, parts)) = BUNDLES.iter().find(|(n, _)| *n == name) else { return std::fs::read(dir.join(name)) };
    let mut out = Vec::new();
    join(dir, parts, &mut out)?;
    Ok(out)
}

fn join(dir: &Path, parts: &[Part], out: &mut Vec<u8>) -> std::io::Result<()> {
    for p in parts {
        match p {
            File(f) => {
                let b = std::fs::read(dir.join(f)).map_err(|e| std::io::Error::new(e.kind(), format!("{f}: {e}")))?;
                out.extend_from_slice(&b);
                if !b.ends_with(b"\n") {
                    out.push(b'\n');
                }
            }
            Wrap(open, inner, close) => {
                out.extend_from_slice(open.as_bytes());
                out.push(b'\n');
                join(dir, inner, out)?;
                out.extend_from_slice(close.as_bytes());
                out.push(b'\n');
            }
        }
    }
    Ok(())
}

/// Every part file of every bundle (a test compares them with the files in assets/js and assets/css).
#[cfg(test)]
pub fn files() -> Vec<&'static str> {
    fn walk(parts: &'static [Part], out: &mut Vec<&'static str>) {
        for p in parts {
            match p {
                File(f) => out.push(f),
                Wrap(_, inner, _) => walk(inner, out),
            }
        }
    }
    let mut out = Vec::new();
    for (_, parts) in BUNDLES {
        walk(parts, &mut out);
    }
    out
}
