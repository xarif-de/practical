//! App tokens (plan D8): same semantics as RustiCal's frontend — random 64-char
//! secret, pbkdf2-hashed by the store, shown once as `<id-prefix>_<secret>`.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use rustical_store::auth::{AppToken, AuthenticationProvider, Principal};

pub async fn list(state: &AppState, user: &Principal) -> AppResult<Vec<AppToken>> {
    let mut t = state.principals.get_app_tokens(&user.id).await?;
    t.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(t)
}

pub fn generate_secret() -> String {
    use rand::RngExt;
    use rand::distr::Alphanumeric;
    rand::rng().sample_iter(Alphanumeric).map(char::from).take(64).collect()
}

/// Returns the one-time visible token string.
pub async fn create(state: &AppState, user: &Principal, name: &str) -> AppResult<String> {
    if name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let secret = generate_secret();
    let mut id = state.principals.add_app_token(&user.id, name.trim().to_string(), secret.clone()).await?;
    id.truncate(4);
    Ok(format!("{id}_{secret}"))
}

pub async fn delete(state: &AppState, user: &Principal, id: &str) -> AppResult<()> {
    // the store scopes by (principal, id); verify ownership explicitly anyway
    if !state.principals.get_app_tokens(&user.id).await?.iter().any(|t| t.id == id) {
        return Err(AppError::NotFound);
    }
    state.principals.remove_app_token(&user.id, id).await?;
    Ok(())
}
