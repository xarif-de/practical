//! Test support (compiled for `cargo test` only): golden files, and the real router in-process on a
//! temporary SQLite file that RustiCal's own migrator created (`create_db_pool(.., true)`) plus our
//! migrations — the same two steps a production start takes, without the network.
//!
//! Golden files live in `tests/snapshots/`; `UPDATE_SNAPSHOTS=1 cargo test` rewrites them. A missing
//! file fails the test (it never passes silently).

use crate::config::Config;
use crate::state::{AppState, SharedState};
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use rustical_store::auth::{AuthenticationProvider, Principal, PrincipalType};
use rustical_store::{AddressbookWriteStore, CalendarWriteStore};
use rustical_store_sqlite::{SqliteAddressbookStore, SqliteCalendarStore, SqlitePrincipalStore};
use sqlx::Row;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

// ---- golden files ----

pub fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// One element per line (a break between `><`), so a changed golden file diffs readably.
pub fn lines_of_html(html: &str) -> String {
    html.replace("><", ">\n<")
}

/// Compares `actual` with `tests/snapshots/<name>`; `UPDATE_SNAPSHOTS=1` writes it instead. The icon
/// sprite's version is masked (`?v=<v>`): it changes with every icon, not with the markup under test.
pub fn assert_snapshot(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(name);
    let actual = actual.replace(crate::routes::assets::icons(), "/assets/icons.svg?v=<v>");
    let actual = if actual.ends_with('\n') { actual } else { format!("{actual}\n") };
    if std::env::var("UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden file {} — run UPDATE_SNAPSHOTS=1 cargo test", path.display()));
    if expected != actual {
        let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
        let at = e.iter().zip(&a).position(|(x, y)| x != y).unwrap_or(e.len().min(a.len()));
        let ctx = |v: &[&str]| v[at.saturating_sub(2)..(at + 3).min(v.len())].join("\n");
        panic!("{name} differs from the golden file at line {}:\n--- expected\n{}\n--- actual\n{}\n(UPDATE_SNAPSHOTS=1 cargo test accepts the new output)", at + 1, ctx(&e), ctx(&a));
    }
}

// ---- memory: bytes the current thread holds (mimalloc underneath, as in production) ----

pub struct Counting;

/// One thread's bytes: held now, highest since the last `peak_of` began. Allocated once per thread straight from
/// mimalloc and never freed: an allocation may be freed on another thread, or after its own thread ended.
struct Ctr {
    now: std::sync::atomic::AtomicIsize,
    peak: std::sync::atomic::AtomicIsize,
}

/// a thread's first allocations before its counter exists, and those of a thread being torn down
static ORPHAN: Ctr = Ctr { now: std::sync::atomic::AtomicIsize::new(0), peak: std::sync::atomic::AtomicIsize::new(0) };

thread_local! {
    static MINE: std::cell::Cell<*const Ctr> = const { std::cell::Cell::new(std::ptr::null()) };
}

fn mine() -> *const Ctr {
    MINE.try_with(|m| {
        if m.get().is_null() {
            let p = unsafe { std::alloc::GlobalAlloc::alloc(&mimalloc::MiMalloc, std::alloc::Layout::new::<Ctr>()) } as *mut Ctr;
            if p.is_null() {
                return &ORPHAN as *const Ctr;
            }
            unsafe { p.write(Ctr { now: std::sync::atomic::AtomicIsize::new(0), peak: std::sync::atomic::AtomicIsize::new(0) }) };
            m.set(p);
        }
        m.get()
    })
    .unwrap_or(&ORPHAN)
}

fn count(c: *const Ctr, delta: isize) {
    use std::sync::atomic::Ordering::Relaxed;
    let c = unsafe { &*c };
    let now = c.now.fetch_add(delta, Relaxed) + delta;
    if delta > 0 {
        c.peak.fetch_max(now, Relaxed);
    }
}

/* Every block carries the counter of the thread that allocated it, just before the pointer handed out, so a free
   counts against its owner wherever it happens: a value bound to a query goes to SQLite's worker thread and is freed
   there, which a per-thread count kept as held forever (the history of a 4 000-object import: 18 MiB "held", most
   of it the import's texts and diffs bound to INSERTs). */
const HDR: usize = 16;

fn outer(l: std::alloc::Layout) -> Option<(std::alloc::Layout, usize)> {
    let off = l.align().max(HDR);
    std::alloc::Layout::from_size_align(l.size().checked_add(off)?, off).ok().map(|o| (o, off))
}

unsafe fn owner(p: *mut u8) -> *const Ctr {
    unsafe { (p.sub(8) as *const *const Ctr).read() }
}

unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let Some((o, off)) = outer(l) else { return std::ptr::null_mut() };
        let base = unsafe { std::alloc::GlobalAlloc::alloc(&mimalloc::MiMalloc, o) };
        if base.is_null() {
            return base;
        }
        let (p, c) = (unsafe { base.add(off) }, mine());
        unsafe { (p.sub(8) as *mut *const Ctr).write(c) };
        count(c, l.size() as isize);
        p
    }
    unsafe fn alloc_zeroed(&self, l: std::alloc::Layout) -> *mut u8 {
        let Some((o, off)) = outer(l) else { return std::ptr::null_mut() };
        let base = unsafe { std::alloc::GlobalAlloc::alloc_zeroed(&mimalloc::MiMalloc, o) };
        if base.is_null() {
            return base;
        }
        let (p, c) = (unsafe { base.add(off) }, mine());
        unsafe { (p.sub(8) as *mut *const Ctr).write(c) };
        count(c, l.size() as isize);
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        let (o, off) = outer(l).expect("a layout this allocator handed out");
        count(unsafe { owner(p) }, -(l.size() as isize));
        unsafe { std::alloc::GlobalAlloc::dealloc(&mimalloc::MiMalloc, p.sub(off), o) };
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, new: usize) -> *mut u8 {
        let (o, off) = outer(l).expect("a layout this allocator handed out");
        let Some(size) = new.checked_add(off) else { return std::ptr::null_mut() };
        let c = unsafe { owner(p) };
        // the header moves along with the content: the block keeps its owner
        let base = unsafe { std::alloc::GlobalAlloc::realloc(&mimalloc::MiMalloc, p.sub(off), o, size) };
        if base.is_null() {
            return base;
        }
        count(c, new as isize - l.size() as isize);
        unsafe { base.add(off) }
    }
}

/// `f`'s result and the most bytes this thread held during it beyond what it held before: what it allocated and
/// is not freed yet, wherever the free happens. A `#[tokio::test]` runs the router on its own thread (current-thread
/// runtime), so this is the request's own peak; what the SQLite worker thread allocates for the rows is not in it.
pub async fn peak_of<F: std::future::Future>(f: F) -> (F::Output, usize) {
    use std::sync::atomic::Ordering::Relaxed;
    let c = unsafe { &*mine() };
    let base = c.now.load(Relaxed);
    c.peak.store(base, Relaxed);
    let out = f.await;
    (out, (c.peak.load(Relaxed) - base).max(0) as usize)
}

// ---- a running app ----

static SEQ: AtomicUsize = AtomicUsize::new(0);

/// A directory of its own per test: tests run in parallel.
fn temp_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("practical-test-{}-{}-{nanos}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub const PUBLIC_URL: &str = "http://localhost:4001";

/// An OIDC client as production has one, with the production defaults: no sign-up, principal id from
/// `preferred_username`. A test that wants it otherwise changes the field.
pub fn oidc_settings() -> crate::config::OidcSettings {
    crate::config::OidcSettings {
        name: "SSO".into(),
        issuer: "https://auth.example.org".into(),
        client_id: "practical".into(),
        client_secret: None,
        scopes: vec!["openid".into()],
        allow_sign_up: false,
        claim_userid: "preferred_username".into(),
    }
}

pub fn config(db_path: &str) -> Config {
    Config {
        bind: "127.0.0.1:0".into(),
        db_path: db_path.into(),
        rustical_url: String::new(),
        public_url: PUBLIC_URL.into(),
        public_dav_url: "https://dav.example.org".into(),
        oidc: None,
        allow_password_login: true,
        logout_url: String::new(),
        trash_retention_days: 90,
        history_retention_days: 90,
        vapid_private_key_pem: None,
        vapid_subject: "mailto:admin@example.org".into(),
        holidays_country: "DE".into(),
        holidays_enabled: true,
        default_timezone: "Europe/Berlin".into(),
        default_lang: "de".into(),
        session_hours: 12,
        dev_assets_dir: None,
        dev_login: None,
        dev_now: None,
        dev_poll_ms: None,
    }
}

pub struct TestApp {
    pub state: SharedState,
    pub router: axum::Router,
    pub sessions: crate::sessions::DbStore,
    dir: PathBuf,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The holiday cache rows a test region needs (`fetched_at` far ahead = never stale), so no view
/// asks OpenHolidays: Sachsen 2026, public and school, German and English.
const HOLIDAYS_SN_2026: &[(&str, &str, &str, &str)] = &[
    ("public", "2026-10-03", "2026-10-03", "Tag der Deutschen Einheit|Day of German Unity"),
    ("public", "2026-10-31", "2026-10-31", "Reformationstag|Reformation Day"),
    ("public", "2026-11-18", "2026-11-18", "Buß- und Bettag|Repentance and Prayer Day"),
    ("public", "2026-12-25", "2026-12-25", "1. Weihnachtsfeiertag|Christmas Day"),
    ("school", "2026-10-05", "2026-10-17", "Herbstferien|Autumn holidays"),
    ("school", "2026-12-21", "2027-01-02", "Weihnachtsferien|Christmas holidays"),
];

impl TestApp {
    pub async fn new() -> Self {
        Self::with(|_| {}).await
    }

    /// `tweak` adjusts the configuration before the state is built.
    pub async fn with(tweak: impl FnOnce(&mut Config)) -> Self {
        let dir = temp_dir();
        let db_path = dir.join("db.sqlite3").to_string_lossy().into_owned();
        // RustiCal's migrator first, exactly as `rustical serve` does on a fresh volume
        let db = rustical_store_sqlite::create_db_pool(&format!("sqlite://{db_path}"), true).await.expect("rustical migrations");
        assert!(crate::db::rustical_schema_present(&db).await.unwrap());
        crate::db::migrate(&db).await.expect("practical migrations");
        let mut config = config(&db_path);
        tweak(&mut config);
        let (cal_tx, _) = tokio::sync::mpsc::channel(64);
        let (ab_tx, _) = tokio::sync::mpsc::channel(64);
        let (changes, _) = tokio::sync::broadcast::channel(256);
        let (dev_reload, _) = tokio::sync::broadcast::channel(16);
        let state: SharedState = Arc::new(AppState {
            principals: Arc::new(SqlitePrincipalStore::new(db.clone())),
            calendars: Arc::new(SqliteCalendarStore::new(db.clone(), cal_tx, true)),
            addressbooks: Arc::new(SqliteAddressbookStore::new(db.clone(), ab_tx, true)),
            i18n: crate::i18n::I18n::load(),
            changes,
            dev_reload,
            // every outbound request fails at once: the discard port refuses, nothing leaves the host
            http: reqwest::Client::builder().proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap()).timeout(std::time::Duration::from_secs(2)).build().unwrap(),
            geo: Default::default(),
            search_index: Default::default(),
            task_cache: Default::default(),
            caches: Default::default(),
            history: Default::default(),
            vapid: None,
            started_at: chrono::DateTime::from_timestamp(1_791_964_800, 0).unwrap(),
            guards: Default::default(),
            db,
            config,
        });
        for lang in ["de", "en"] {
            for kind in ["public", "school"] {
                let list: Vec<serde_json::Value> = HOLIDAYS_SN_2026
                    .iter()
                    .filter(|h| h.0 == kind)
                    .map(|(_, s, e, n)| {
                        let (de, en) = n.split_once('|').unwrap();
                        serde_json::json!({ "start": s, "end": e, "name": if lang == "de" { de } else { en }, "kind": kind, "nationwide": kind == "public", "source": "openholidays", "regions": [] })
                    })
                    .collect();
                sqlx::query("INSERT INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES ('DE', 'DE-SN', 2026, ?, ?, '2999-01-01 00:00:00')")
                    .bind(format!("{kind}2-{lang}"))
                    .bind(serde_json::Value::Array(list).to_string())
                    .execute(&state.db)
                    .await
                    .unwrap();
            }
            // the subdivision list behind the settings page's region names
            let subs = serde_json::json!([{ "code": "DE-SN", "name": if lang == "de" { "Sachsen" } else { "Saxony" }, "parent": "DE", "depth": 0, "category": "", "school": false }]);
            sqlx::query("INSERT INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES ('DE', '', 0, ?, ?, '2999-01-01 00:00:00')")
                .bind(format!("subdivisions2-{lang}"))
                .bind(subs.to_string())
                .execute(&state.db)
                .await
                .unwrap();
        }
        let sessions = crate::sessions::DbStore::new(state.db.clone());
        let router = crate::build_router_with(&state, sessions.clone()).expect("router");
        Self { state, router, sessions, dir }
    }

    /// An individual principal whose password is `pw` (the hash is made by the store's own app-token
    /// hasher, which the password check verifies too).
    pub async fn user(&self, id: &str, name: &str) -> Principal {
        let p = Principal { id: id.into(), displayname: Some(name.into()), principal_type: PrincipalType::Individual, password: None, memberships: vec![] };
        self.state.principals.insert_principal(p, false).await.unwrap();
        let tid = self.state.principals.add_app_token(id, "pw".into(), PASSWORD.into()).await.unwrap();
        let hash: String = sqlx::query("SELECT token FROM app_tokens WHERE id = ?").bind(&tid).fetch_one(&self.state.db).await.unwrap().get("token");
        self.state.principals.remove_app_token(id, &tid).await.unwrap();
        sqlx::query("UPDATE principals SET password_hash = ? WHERE id = ?").bind(hash).bind(id).execute(&self.state.db).await.unwrap();
        self.principal(id).await
    }

    pub async fn group(&self, id: &str, name: &str, members: &[&str]) {
        let p = Principal { id: id.into(), displayname: Some(name.into()), principal_type: PrincipalType::Group, password: None, memberships: vec![] };
        self.state.principals.insert_principal(p, false).await.unwrap();
        for m in members {
            self.state.principals.add_membership(m, id).await.unwrap();
        }
    }

    pub async fn principal(&self, id: &str) -> Principal {
        self.state.principals.get_principal(id).await.unwrap().unwrap()
    }

    pub async fn calendar(&self, owner: &str, name: &str, color: &str, comps: &[&str]) -> String {
        let user = self.principal(owner).await;
        crate::domain::collections::create_calendar(
            &self.state,
            &user,
            crate::domain::collections::NewCalendar {
                owner: owner.into(),
                name: name.into(),
                color: color.into(),
                description: String::new(),
                components: crate::domain::collections::parse_components(&comps.iter().map(|c| c.to_string()).collect::<Vec<_>>()),
                timezone: "Europe/Berlin".into(),
            },
        )
        .await
        .unwrap()
    }

    pub async fn addressbook(&self, owner: &str, name: &str, color: &str) -> String {
        let user = self.principal(owner).await;
        crate::domain::collections::create_addressbook(&self.state, &user, owner, name, color, "").await.unwrap()
    }

    /// Stores a calendar object as a DAV client would (object id = `id`).
    pub async fn put_ics(&self, owner: &str, cal: &str, id: &str, ics: &str) {
        let obj = rustical_ical::CalendarObject::import(ics, None).unwrap_or_else(|e| panic!("{id}: {e}"));
        self.state.calendars.put_object(owner, cal, id, obj, true).await.unwrap();
    }

    pub async fn put_vcf(&self, owner: &str, ab: &str, id: &str, vcf: &str) {
        let obj = rustical_ical::AddressObject::from_vcf(vcf.to_string()).unwrap_or_else(|e| panic!("{id}: {e}"));
        AddressbookWriteStore::put_object(&*self.state.addressbooks, owner, ab, id, obj, true).await.unwrap();
    }

    /// A browser session: logs in through the login form (CSRF token from the page, then the POST).
    pub async fn login(&self, id: &str) -> Client {
        let mut c = Client::for_router(self.router.clone());
        c.login_as(id).await;
        c
    }

    /// No session at all.
    pub fn anonymous(&self) -> Client {
        Client::for_router(self.router.clone())
    }

    /// HTTP Basic with a fresh app token (how DAV clients and scripts authenticate).
    pub async fn basic(&self, id: &str) -> Client {
        let user = self.principal(id).await;
        let token = crate::domain::tokens::create(&self.state, &user, "test").await.unwrap();
        use base64ct::Encoding;
        let b = base64ct::Base64::encode_string(format!("{id}:{token}").as_bytes());
        Client { router: self.router.clone(), cookies: vec![], csrf: String::new(), basic: Some(format!("Basic {b}")) }
    }
}

pub const PASSWORD: &str = "test-password-1";

pub struct Resp {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
    pub body: String,
}

impl Resp {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
    }
    pub fn location(&self) -> Option<String> {
        self.header("location")
    }
    pub fn meta_csrf(&self) -> String {
        let at = self.body.find("<meta name=\"csrf\" content=\"").unwrap_or_else(|| panic!("no csrf meta: {}", &self.body[..self.body.len().min(300)]));
        let rest = &self.body[at + "<meta name=\"csrf\" content=\"".len()..];
        rest[..rest.find('"').unwrap()].to_string()
    }
    /// The JSON of `<script type="application/json" id="{id}">`.
    pub fn island(&self, id: &str) -> serde_json::Value {
        let open = format!("<script type=\"application/json\" id=\"{id}\">");
        let at = self.body.find(&open).unwrap_or_else(|| panic!("no #{id} island"));
        let rest = &self.body[at + open.len()..];
        serde_json::from_str(&rest[..rest.find("</script>").unwrap()]).unwrap()
    }
    #[track_caller]
    pub fn ok(&self) -> &Self {
        assert_eq!(self.status, StatusCode::OK, "{}", self.body);
        self
    }
    #[track_caller]
    pub fn has(&self, needle: &str) -> &Self {
        assert!(self.body.contains(needle), "{needle:?} not in:\n{}", &self.body[..self.body.len().min(4000)]);
        self
    }
}

#[derive(Clone)]
pub struct Client {
    router: axum::Router,
    cookies: Vec<(String, String)>,
    pub csrf: String,
    basic: Option<String>,
}

impl Client {
    pub fn for_router(router: axum::Router) -> Client {
        Client { router, cookies: vec![], csrf: String::new(), basic: None }
    }

    pub async fn login_as(&mut self, id: &str) {
        let page = self.get("/login").await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
        let csrf = page.meta_csrf();
        let r = self.post_form_raw("/login", &[("username", id), ("password", PASSWORD), ("csrf", &csrf), ("redirect_uri", "/calendar")]).await;
        assert_eq!((r.status, r.location()), (StatusCode::SEE_OTHER, Some("/calendar".to_string())), "login as {id}: {}", r.body);
        // the session id cycles on login; the CSRF token stays the session's
        self.csrf = self.get("/settings").await.meta_csrf();
    }

    pub async fn send(&mut self, method: Method, path: &str, headers: &[(&str, &str)], body: Body) -> Resp {
        let mut req = Request::builder().method(method.clone()).uri(path);
        if !self.cookies.is_empty() {
            req = req.header(header::COOKIE, self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "));
        }
        if let Some(b) = &self.basic {
            req = req.header(header::AUTHORIZATION, b);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = self.router.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        for v in resp.headers().get_all(header::SET_COOKIE) {
            let v = v.to_str().unwrap();
            let (kv, attrs) = v.split_once(';').unwrap_or((v, ""));
            let (k, val) = kv.split_once('=').unwrap();
            self.cookies.retain(|(n, _)| n != k);
            if !attrs.to_ascii_lowercase().contains("max-age=0") {
                self.cookies.push((k.trim().to_string(), val.trim().to_string()));
            }
        }
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024).await.unwrap().to_vec();
        Resp { status, headers, body: String::from_utf8_lossy(&bytes).into_owned(), bytes }
    }

    /// Status and headers only: for streams (SSE) whose body never ends.
    pub async fn open(&mut self, path: &str) -> (StatusCode, HeaderMap) {
        let mut req = Request::builder().uri(path);
        if !self.cookies.is_empty() {
            req = req.header(header::COOKIE, self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "));
        }
        if let Some(b) = &self.basic {
            req = req.header(header::AUTHORIZATION, b);
        }
        let resp = self.router.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        (resp.status(), resp.headers().clone())
    }

    /// The Cookie header this client sends.
    pub fn cookie_header(&self) -> String {
        self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    /// A cookie the page's script set (`document.cookie`).
    pub fn cookie(&mut self, name: &str, value: &str) {
        self.cookies.retain(|(n, _)| n != name);
        self.cookies.push((name.into(), value.into()));
    }

    /// A page as the browser loads it.
    pub async fn get(&mut self, path: &str) -> Resp {
        self.send(Method::GET, path, &[("accept", "text/html")], Body::empty()).await
    }

    pub async fn get_json(&mut self, path: &str) -> Resp {
        self.send(Method::GET, path, &[("accept", "application/json")], Body::empty()).await
    }

    /// fetch() from the page: same-origin metadata and the CSRF header.
    pub async fn json(&mut self, method: Method, path: &str, body: serde_json::Value) -> Resp {
        let csrf = self.csrf.clone();
        self.send(method, path, &[("content-type", "application/json"), ("accept", "application/json"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &csrf)], Body::from(body.to_string())).await
    }

    pub async fn delete(&mut self, path: &str) -> Resp {
        let csrf = self.csrf.clone();
        self.send(Method::DELETE, path, &[("accept", "application/json"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &csrf)], Body::empty()).await
    }

    /// A form POST without adding anything.
    pub async fn post_form_raw(&mut self, path: &str, fields: &[(&str, &str)]) -> Resp {
        let mut s = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in fields {
            s.append_pair(k, v);
        }
        self.send(Method::POST, path, &[("content-type", "application/x-www-form-urlencoded"), ("sec-fetch-site", "same-origin")], Body::from(s.finish())).await
    }

    /// A settings form as the page submits it (CSRF field added).
    pub async fn post_form(&mut self, path: &str, fields: &[(&str, &str)]) -> Resp {
        let csrf = self.csrf.clone();
        let mut all: Vec<(&str, &str)> = fields.to_vec();
        all.push(("csrf", &csrf));
        self.post_form_raw(path, &all).await
    }

    /// The same form saved by the page's autosave (`Accept: application/json`).
    pub async fn post_form_json(&mut self, path: &str, fields: &[(&str, &str)]) -> Resp {
        let csrf = self.csrf.clone();
        let mut s = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in fields.iter().chain([("csrf", csrf.as_str())].iter()) {
            s.append_pair(k, v);
        }
        self.send(Method::POST, path, &[("content-type", "application/x-www-form-urlencoded"), ("accept", "application/json"), ("sec-fetch-site", "same-origin")], Body::from(s.finish())).await
    }

    /// A raw body (the import endpoints read the file as the body).
    pub async fn post_raw(&mut self, path: &str, body: &str) -> Resp {
        let csrf = self.csrf.clone();
        self.send(Method::POST, path, &[("content-type", "text/plain"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &csrf)], Body::from(body.to_string())).await
    }
}

/// A minimal VCALENDAR around VEVENT/VTODO lines, with a VTIMEZONE for every TZID used (RustiCal
/// rejects a dangling TZID).
pub fn ics(body: &str) -> String {
    let mut zones: Vec<&str> = body.split("TZID=").skip(1).filter_map(|r| r.split([':', ';']).next()).collect();
    zones.sort();
    zones.dedup();
    let vtz: String = zones.iter().filter_map(|z| crate::domain::ical::vtimezone(z)).map(|v| format!("{}\r\n", v.trim_end())).collect();
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//test//EN\r\n{vtz}{}\r\nEND:VCALENDAR\r\n", body.trim().replace('\n', "\r\n"))
}

pub fn vevent(uid: &str, summary: &str, extra: &str) -> String {
    ics(&format!("BEGIN:VEVENT\nUID:{uid}\nDTSTAMP:20260901T000000Z\nSUMMARY:{summary}\n{}\nEND:VEVENT", extra.trim()))
}

pub fn vtodo(uid: &str, summary: &str, extra: &str) -> String {
    ics(&format!("BEGIN:VTODO\nUID:{uid}\nDTSTAMP:20260901T000000Z\nSUMMARY:{summary}\n{}\nEND:VTODO", extra.trim()))
}

// ---- view inputs ----

pub fn date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn datetime(s: &str) -> chrono::NaiveDateTime {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").or_else(|_| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap())).unwrap()
}

/// An instance as `events::instances` makes it; "YYYY-MM-DD" bounds make it all-day (end exclusive).
pub fn instance(cal: &str, id: &str, summary: &str, start: &str, end: &str) -> crate::domain::events::Instance {
    crate::domain::events::Instance {
        principal: "alice".into(),
        cal_id: cal.into(),
        cal_name: format!("Kalender {cal}"),
        color: if cal == "b" { "#30b46c".into() } else { "#0a84ff".into() },
        object_id: id.into(),
        uid: id.into(),
        summary: summary.into(),
        location: "".into(),
        description: "".into(),
        start: datetime(start),
        end: datetime(end),
        all_day: start.len() == 10,
        recurring: false,
        categories: vec![],
        alarm_minutes: None,
        alarms: vec![],
        is_group: false,
        cancelled: false,
    }
}

/// Holiday marks by day, as `holidays::marks` returns them: (first day, last day, name, school, tags).
pub fn marks(list: &[(&str, &str, &str, bool, &[&str])]) -> crate::render::Hols {
    let mut out = crate::render::Hols::new();
    for (from, to, name, school, tags) in list {
        let (from, to) = (date(from), date(to));
        let mut d = from;
        while d <= to {
            out.entry(d).or_default().push(crate::domain::holidays::Mark { name: name.to_string(), tags: tags.iter().map(|t| t.to_string()).collect(), school: *school, start: from });
            d = d.succ_opt().unwrap();
        }
    }
    out
}

pub fn lang(l: &str) -> crate::render::L {
    crate::render::L::new(l, crate::i18n::I18n::load())
}
