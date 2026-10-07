//! /contacts — letter panels or sheet table, detail editor on wide screens. The page renders the view it opens in
//! (`?view=`, else the `ctview` cookie app.js sets on a switch, else the list); the other one comes from
//! /contacts/part on its first switch (perf3 #8: the table was 417 KB of the 764 KB page with both).

use crate::ctx::PageCtx;
use crate::domain::collections::{self, AbInfo};
use crate::domain::contacts::{self, Contact};
use crate::domain::prefs;
use crate::error::AppResult;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::State;
use crate::extract::Query;
use axum::routing::get;
use rustical_store::auth::Principal;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct CtQuery {
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub abs: Option<String>,
}

pub struct LetterGroup {
    pub letter: String,
    pub contacts: Vec<Contact>,
}

#[derive(Template, WebTemplate)]
#[template(path = "contacts.html")]
pub struct ContactsPage {
    pub ctx: PageCtx,
    pub addressbooks: Vec<AbInfo>,
    pub groups: Vec<LetterGroup>,
    pub contacts: Vec<Contact>,
    pub categories: Vec<String>,
    pub table: bool,
    /// the table's rows carry the sort keys (app.js sorts by them): no list entry does
    pub keys: bool,
    pub columns: Vec<String>,
    /// fields shown per entry in the list view
    pub list_fields: Vec<String>,
}

impl ContactsPage {
    /// a group address book is in the filter: only then does "only mine" differ from "all"
    pub fn has_shared(&self) -> bool {
        self.addressbooks.iter().any(|a| a.is_group)
    }
    pub fn col(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c == name)
    }
    pub fn lf(&self, name: &str) -> bool {
        self.list_fields.iter().any(|c| c == name)
    }
}

pub const ALL_COLUMNS: &[&str] = &["phone", "email", "category", "addressbook", "birthday", "address", "org", "note"];
/// list view: everything but the note by default
pub const ALL_LIST_FIELDS: &[&str] = &["phone", "email", "birthday", "address", "addressbook", "category", "note"];

/// The view the page opens in: the URL's, else the one last picked in this browser (cookie), else the list.
fn table_view(q: &CtQuery, headers: &axum::http::HeaderMap) -> bool {
    q.view.clone().or_else(|| crate::extract::cookie(headers, "ctview")).as_deref() == Some("table")
}

/// The contacts of the filter (`abs`), sorted, and grouped by initial; plus all address books.
async fn read(state: &SharedState, user: &Principal, q: &CtQuery) -> AppResult<(Vec<AbInfo>, Vec<Contact>, Vec<LetterGroup>)> {
    let all = collections::addressbooks_for(state, user, false).await?;
    let filter: Option<Vec<String>> = q.abs.as_deref().filter(|s| !s.is_empty()).map(|s| s.split(',').map(str::to_string).collect());
    let abs: Vec<AbInfo> = match &filter {
        Some(f) => all.iter().filter(|a| f.contains(&format!("{}/{}", a.principal, a.id))).cloned().collect(),
        None => all.clone(),
    };
    let mut contacts = contacts::contacts_for(state, &abs).await?;
    contacts.sort_by_cached_key(|c| c.given_key());
    let mut by_letter: BTreeMap<String, Vec<Contact>> = BTreeMap::new();
    for c in &contacts {
        by_letter.entry(c.given_letter()).or_default().push(c.clone());
    }
    Ok((all, contacts, by_letter.into_iter().map(|(letter, contacts)| LetterGroup { letter, contacts }).collect()))
}

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, headers: axum::http::HeaderMap, Query(q): Query<CtQuery>) -> AppResult<ContactsPage> {
    let (all, contacts, groups) = read(&state, &user, &q).await?;
    let mut categories: Vec<String> = contacts.iter().flat_map(|c| c.categories.clone()).collect();
    categories.sort();
    categories.dedup();
    let table = table_view(&q, &headers);
    let p = prefs::get(&state.db, &user.id).await?;
    let columns: Vec<String> = p.contact_columns_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| ALL_COLUMNS.iter().take(5).map(|s| s.to_string()).collect());
    let list_fields: Vec<String> = p.contact_list_fields_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| ALL_LIST_FIELDS.iter().take(6).map(|s| s.to_string()).collect());
    Ok(ContactsPage {
        ctx,
        addressbooks: all,
        groups,
        contacts,
        categories,
        table,
        keys: table,
        columns,
        list_fields,
    })
}

/// The view a page did not bring (`?view=list|table`, the page's `abs`): the list's letter panels or the table's rows,
/// as the page renders them; rows without the sort keys (the page has the list entries).
#[derive(Template, WebTemplate)]
#[template(path = "contacts_part.html")]
pub struct ContactsPart {
    pub ctx: PageCtx,
    pub groups: Vec<LetterGroup>,
    pub contacts: Vec<Contact>,
    pub table: bool,
    pub keys: bool,
}

async fn part(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<CtQuery>) -> AppResult<ContactsPart> {
    let (_, contacts, groups) = read(&state, &user, &q).await?;
    Ok(ContactsPart { ctx, groups, contacts, table: q.view.as_deref() == Some("table"), keys: false })
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/contacts", get(page)).route("/contacts/part", get(part))
}
