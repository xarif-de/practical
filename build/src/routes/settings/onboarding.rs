//! First-start onboarding: the language gate's form, the confirmations, and the two end states.
//!
//! The gate is a real form (`POST /settings/onboarding/lang`) so the language can be set without
//! JavaScript; everything else is the guided hints talking, and answers JSON.
//!
//! A confirmation records a mark and nothing else ([`confirm`]): every confirmable point means
//! "what is in force is fine", and writing that value out would turn a default the user accepted
//! into a choice they never made.

use super::*;
use crate::domain::onboarding::{self, Step};

#[derive(Deserialize)]
pub(super) struct LangForm {
    csrf: String,
    #[serde(default)]
    lang: String,
    /// where to go back to; only a local path is used (`crate::auth::safe_redirect`)
    #[serde(default)]
    back: String,
    /// Set when the user only switched the language in the gate, without pressing the button: then
    /// the page is reloaded in that language and the gate stays up, because they have not decided
    /// yet (asked for 2026-10-09). Only the button records the mark.
    #[serde(default)]
    try_only: String,
}

/// The language gate: stores the chosen language, and records the decision unless this was only a
/// switch to see the gate in that language ([`LangForm::try_only`]).
pub(super) async fn lang_save(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<LangForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let lang = Some(f.lang.clone()).filter(|l| crate::i18n::supported(l)).ok_or_else(|| AppError::bad("err-lang"))?;
        let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
        let mut p = prefs::get(&state.db, &user.id).await?;
        p.lang = Some(lang);
        prefs::set(&state.db, &user.id, &p).await?;
        if f.try_only.is_empty() {
            onboarding::confirm(&state.db, &user.id, Step::Lang).await?;
            return Ok::<_, AppError>(Some("flash-saved"));
        }
        // merely trying a language on: the gate returns in it, and a "Saved." toast would be noise
        Ok::<_, AppError>(None)
    }
    .await;
    /* The gate sits on whatever page the user opened, so it returns there, not to /settings. The
       login's own guard decides what counts as a local path — it also drops the tab/CR/LF a browser
       reads over, which a hand-rolled check here missed (audit, 2026-10-09). */
    super::finish_here(&state, &session, &ctx, r, &crate::auth::safe_redirect(Some(f.back.trim()))).await
}

#[derive(Deserialize)]
pub(super) struct StepChoice {
    step: String,
}

/// Records a confirmation: the mark, and nothing else.
///
/// No step writes a preference here any more. Each confirmable point means "take what is already in
/// force", and in every case that value is one the user is better off NOT having written out:
///
/// - time zone: confirming means taking the picker's "Standard (Europe/Berlin)" row, which stands
///   for following the server's default. Writing today's default into the column would freeze it as
///   the user's own choice, and the picker would then show a named zone where they chose "Standard"
///   (2026-10-09). A zone they really picked closes the point by itself ([`onboarding::Facts`]).
/// - holidays: there is no default at all, so confirming can only mean "show me none" — there is no
///   value to store, which is exactly why the mark exists.
/// - language: written by the gate's own form, the only way to confirm it.
pub(super) async fn confirm(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(d): crate::extract::Json<StepChoice>) -> AppResult<axum::Json<onboarding::View>> {
    let step = Step::parse(&d.step).filter(|s| s.is_confirmable()).ok_or_else(|| AppError::bad("err-onboarding-step"))?;
    {
        let _lock = state.guards.prefs(&user.id).await; // the record rides in the prefs row
        onboarding::confirm(&state.db, &user.id, step).await?;
    }
    state_view(&state, &user.id).await
}

/// Everything was satisfied: stop counting collections on every page view. The browser reports this
/// after it has shown the closing word, so no page view has to write — but the server checks for
/// itself, because a tab working from a stale list must not shut the setup down with points still
/// open. Refused then, and the hints stay.
pub(super) async fn complete(State(state): State<SharedState>, user: Principal, _: csrf::Token) -> AppResult<axum::http::StatusCode> {
    let _lock = state.guards.prefs(&user.id).await;
    let p = prefs::get(&state.db, &user.id).await?;
    let facts = onboarding::facts(&state, &user.id, &p).await;
    let took = onboarding::complete(&state.db, &user.id, facts).await?;
    Ok(if took { axum::http::StatusCode::NO_CONTENT } else { axum::http::StatusCode::CONFLICT })
}

/// Settings › Erste Schritte: show the hints again (the confirmations stay). A page form, so its
/// token is the form field — not the header a fetch would send.
pub(super) async fn reopen(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<super::Csrf>) -> Response {
    let r = match csrf::check(&session, &f.csrf).await {
        Ok(()) => {
            let _lock = state.guards.prefs(&user.id).await;
            onboarding::reopen(&state.db, &user.id).await.map(|()| None).map_err(AppError::from)
        }
        Err(e) => Err(e),
    };
    super::finish_here(&state, &session, &ctx, r, "/calendar").await
}

/// The current state, for hints that have to re-aim after an in-page save (no reload).
pub(super) async fn api_state(State(state): State<SharedState>, user: Principal) -> AppResult<axum::Json<onboarding::View>> {
    state_view(&state, &user.id).await
}

async fn state_view(state: &SharedState, principal: &str) -> AppResult<axum::Json<onboarding::View>> {
    let rec = onboarding::get(&state.db, principal).await?;
    // a dismissed or finished record has no view; the browser reads an empty list as "nothing to do"
    Ok(axum::Json(onboarding::view(state, principal, &rec).await.unwrap_or(onboarding::View { open: vec![], tz: String::new() })))
}
