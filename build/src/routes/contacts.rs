//! /contacts — letter panels or sheet table, detail editor on wide screens.

use crate::ctx::PageCtx;
use crate::domain::collections::{self, AbInfo, Owner};
use crate::domain::contacts::{self, Contact};
use crate::domain::prefs;
use crate::error::AppResult;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::{Query, State};
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
    pub owners: Vec<Owner>,
    pub groups: Vec<LetterGroup>,
    pub contacts: Vec<Contact>,
    pub categories: Vec<String>,
    pub table: bool,
    pub abs_param: String,
    pub columns: Vec<String>,
    /// fields shown per entry in the list view
    pub list_fields: Vec<String>,
    pub today: chrono::NaiveDate,
}

impl ContactsPage {
    pub fn col(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c == name)
    }
    pub fn lf(&self, name: &str) -> bool {
        self.list_fields.iter().any(|c| c == name)
    }
}

pub const ALL_COLUMNS: &[&str] = &["phone", "email", "category", "addressbook", "birthday", "address", "org"];
/// list view: everything but the note by default
pub const ALL_LIST_FIELDS: &[&str] = &["phone", "email", "birthday", "address", "category", "note"];

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<CtQuery>) -> AppResult<ContactsPage> {
    let all = collections::addressbooks_for(&state, &user, false).await?;
    let filter: Option<Vec<String>> = q.abs.as_deref().filter(|s| !s.is_empty()).map(|s| s.split(',').map(str::to_string).collect());
    let abs: Vec<AbInfo> = match &filter {
        Some(f) => all.iter().filter(|a| f.contains(&format!("{}/{}", a.principal, a.id))).cloned().collect(),
        None => all.clone(),
    };
    let contacts = contacts::contacts_for(&state, &abs).await?;
    let mut by_letter: BTreeMap<String, Vec<Contact>> = BTreeMap::new();
    for c in &contacts {
        by_letter.entry(c.letter()).or_default().push(c.clone());
    }
    let mut categories: Vec<String> = contacts.iter().flat_map(|c| c.categories.clone()).collect();
    categories.sort();
    categories.dedup();
    let p = prefs::get(&state.db, &user.id).await?;
    let columns: Vec<String> = p.contact_columns_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| ALL_COLUMNS.iter().take(5).map(|s| s.to_string()).collect());
    let list_fields: Vec<String> = p.contact_list_fields_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| ALL_LIST_FIELDS.iter().take(5).map(|s| s.to_string()).collect());
    let today = chrono::Utc::now().date_naive();
    Ok(ContactsPage {
        ctx,
        owners: collections::owners(&state, &user).await?,
        addressbooks: all,
        groups: by_letter.into_iter().map(|(letter, contacts)| LetterGroup { letter, contacts }).collect(),
        contacts,
        categories,
        table: q.view.as_deref() == Some("table"),
        abs_param: q.abs.unwrap_or_default(),
        columns,
        list_fields,
        today,
    })
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/contacts", get(page))
}
