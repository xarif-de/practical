//! /editor?kind=event|task — the editor dialog (templates/editor.html) with its JSON islands and no page around
//! it: the settings' categories load it on first use and open a rule over the settings page
//! (js/shell/settings-pages.js, `practical.editorInit`). `Islands` and `calendars` are what every page with the
//! editor feeds it (calendar.rs, tasks.rs): one builder, so the dialog reads the same data wherever it opens.

use crate::ctx::PageCtx;
use crate::domain::collections::CalInfo;
use crate::domain::{catrules, events, prefs};
use crate::error::AppResult;
use crate::extract::Query;
use crate::routes::calendar::PageView;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use rustical_store::auth::Principal;
use serde::Deserialize;
use tower_sessions::Session;

/// The JSON islands editor.html's script reads (`#alarm-presets-data`, `#category-rules-data`, `#alarm-snaps-data`;
/// templates/editor_islands.html), escaped for `<script>`.
pub struct Islands {
    /// reminder presets; `null` = the editor's defaults
    pub alarm_presets_json: String,
    /// snap points of the reminder strip (`events::AlarmSnaps`)
    pub alarm_snaps_json: String,
    /// the user's category rules of this kind (`domain::catrules`), applied by the editor
    pub category_rules_json: String,
}

/// `kind`: "event" | "task" (the rules' scope)
pub async fn islands(state: &SharedState, user: &Principal, p: &prefs::Prefs, kind: &str) -> AppResult<Islands> {
    Ok(Islands {
        alarm_snaps_json: crate::render::script_json(serde_json::to_string(&events::alarm_snaps(p.alarm_snaps_json.as_deref())).unwrap()),
        alarm_presets_json: crate::render::script_json(p.alarm_presets_json.clone().filter(|j| serde_json::from_str::<Vec<events::AlarmPreset>>(j).is_ok()).unwrap_or_else(|| "null".into())),
        category_rules_json: crate::render::script_json(serde_json::to_string(&catrules::all(&state.db, &user.id, kind).await?).unwrap()),
    })
}

/// The calendars of the editor's calendar choice: every visible one for an event, those holding VTODOs for a task.
pub fn calendars(all: &[CalInfo], kind: &str) -> Vec<CalInfo> {
    all.iter().filter(|c| kind != "task" || c.components.iter().any(|x| x == "VTODO")).cloned().collect()
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Event,
    Task,
}

#[derive(Deserialize)]
struct EditorQuery {
    kind: Kind,
}

#[derive(Template, WebTemplate)]
#[template(path = "editor_fragment.html")]
struct EditorFragment {
    ctx: PageCtx,
    calendars: Vec<CalInfo>,
    editor_kind: &'static str,
    alarm_presets_json: String,
    alarm_snaps_json: String,
    category_rules_json: String,
}

async fn fragment(State(state): State<SharedState>, user: Principal, session: Session, PageView(ctx, vc): PageView, Query(q): Query<EditorQuery>) -> AppResult<EditorFragment> {
    let kind = match q.kind {
        Kind::Event => "event",
        Kind::Task => "task",
    };
    // the page context takes a pending flash off the session; a fragment shows none, so it goes back for the next
    // page (a prefetch never gets here with one: sessions::activity answers it 204)
    if let Some(f) = &ctx.flash {
        crate::ctx::set_flash(&session, &f.kind, f.text.clone()).await;
    }
    let all = events::visible_calendars(&state, &user, None).await?;
    let Islands { alarm_presets_json, alarm_snaps_json, category_rules_json } = islands(&state, &user, &vc.prefs, kind).await?;
    Ok(EditorFragment { ctx, calendars: calendars(&all, kind), editor_kind: kind, alarm_presets_json, alarm_snaps_json, category_rules_json })
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/editor", get(fragment))
}
