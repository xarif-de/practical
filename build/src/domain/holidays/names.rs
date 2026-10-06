//! Region names (CLDR) in a compact text form: one table per kind, one line per region, sorted by
//! code — `CODE \t META \t de \t en \t fr \t es`. Each name is the one `name_in` (holidays.rs) resolves
//! for that language (its own, else English, else any); a non-English field is empty when it equals
//! the English one, an empty English field means "no name". META: `oh + 2·ng` (countries; which
//! holiday source covers it) or the parent code, empty when it is the code's country (subdivisions).
//! build.rs writes the built-in tables from src/data/regions.json (`#[path]`: std only here);
//! holidays.rs packs a newer CLDR release the same way at run time.

/// The column order of the names (= crate::i18n::LANGS, test `names_columns_are_the_ui_languages`).
pub const LANGS: [&str; 4] = ["de", "en", "fr", "es"];
const EN: usize = 1;

/// One region as the source has it: its names per LANGS column, `any` = some name of it (the
/// fallback when it has no English one).
pub struct Src {
    pub code: String,
    pub meta: String,
    pub names: [Option<String>; 4],
    pub any: Option<String>,
}

/// Tab and newline are the separators: a name holding one (never in CLDR) gets a space instead.
fn clean(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains(['\t', '\n']) { s.replace(['\t', '\n'], " ").into() } else { s.into() }
}

/// The table text of `rows` (any order; a code appears once).
pub fn encode(mut rows: Vec<Src>) -> String {
    rows.sort_by(|a, b| a.code.cmp(&b.code));
    let mut out = String::new();
    for r in &rows {
        let en = r.names[EN].as_deref().or(r.any.as_deref()).unwrap_or("");
        out.push_str(&clean(&r.code));
        out.push('\t');
        out.push_str(&clean(&r.meta));
        for (i, n) in r.names.iter().enumerate() {
            out.push('\t');
            if i == EN {
                out.push_str(&clean(en));
            } else if let Some(n) = n.as_deref().filter(|n| *n != en) {
                out.push_str(&clean(n));
            }
        }
        out.push('\n');
    }
    out
}

/// A decoded table: the text and where each line starts.
pub struct Table {
    text: String,
    starts: Vec<u32>,
}

/// One line of a table.
#[derive(Clone, Copy)]
pub struct Row<'a> {
    pub code: &'a str,
    pub meta: &'a str,
    names: &'a str,
}

impl<'a> Row<'a> {
    fn of(line: &'a str) -> Self {
        let mut f = line.splitn(3, '\t');
        Row { code: f.next().unwrap_or(""), meta: f.next().unwrap_or(""), names: f.next().unwrap_or("") }
    }
    /// The name in `lang` (its own, else English), None when the region has none.
    pub fn name(&self, lang: &str) -> Option<&'a str> {
        let mut f = self.names.split('\t');
        let i = LANGS.iter().position(|l| *l == lang);
        let mut own = None;
        let mut en = "";
        for j in 0..LANGS.len() {
            let v = f.next().unwrap_or("");
            if j == EN {
                en = v;
            } else if Some(j) == i && !v.is_empty() {
                own = Some(v);
            }
        }
        if en.is_empty() { None } else { Some(own.unwrap_or(en)) }
    }
    /// A subdivision's parent code.
    pub fn parent(&self) -> &'a str {
        if self.meta.is_empty() { &self.code[..2.min(self.code.len())] } else { self.meta }
    }
}

impl Table {
    pub fn new(text: String) -> Self {
        let mut starts = vec![];
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            starts.push(at as u32);
            at += line.len();
        }
        let t = Table { text, starts };
        debug_assert!(t.rows().zip(t.rows().skip(1)).all(|(a, b)| a.code < b.code), "table not sorted by code");
        t
    }
    fn row(&self, i: usize) -> Row<'_> {
        let end = self.starts.get(i + 1).map_or(self.text.len(), |e| *e as usize);
        Row::of(self.text[self.starts[i] as usize..end].trim_end_matches('\n'))
    }
    pub fn find(&self, code: &str) -> Option<Row<'_>> {
        self.starts.binary_search_by(|&s| {
            let line = &self.text[s as usize..];
            line[..line.find('\t').unwrap_or(line.len())].cmp(code)
        }).ok().map(|i| self.row(i))
    }
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        (0..self.starts.len()).map(|i| self.row(i))
    }
    /// The rows whose code starts with `prefix` (a contiguous run: the table is sorted by code).
    pub fn with_prefix<'t>(&'t self, prefix: &str) -> impl Iterator<Item = Row<'t>> {
        let code = |s: &u32| { let l = &self.text[*s as usize..]; &l[..l.find('\t').unwrap_or(l.len())] };
        let from = self.starts.partition_point(|s| code(s) < prefix);
        let to = from + self.starts[from..].partition_point(|s| code(s).starts_with(prefix));
        (from..to).map(|i| self.row(i))
    }
    /// heap bytes held (text + line index)
    #[allow(dead_code)]
    pub fn heap_bytes(&self) -> usize {
        self.text.capacity() + self.starts.capacity() * 4
    }
}
