//! Root redirect (the pages live in calendar.rs, contacts.rs, tasks.rs, settings.rs).

use axum::response::Redirect;

pub async fn root() -> Redirect {
    Redirect::to("/calendar")
}
