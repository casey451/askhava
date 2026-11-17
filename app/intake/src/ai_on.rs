//! Whether the AI-008 switch lets the AI run now, for the pages that describe it (AI-018, T221,
//! D-3144, D-3878).
//!
//! `site` may not call `ai` (`kit/layers.json`), so the data pages, and later the help page (T201)
//! and the web chat (T103), read the switch here: [`ai_on`] is true exactly when
//! `ai::api::ai_mode` answers `AiMode::Ai` in that environment. Nothing is written.

use askhava_ai::api::{AiError, AiMode, ai_mode};
use askhava_core::api::Environment;
use sqlx::PgConnection;

/// AI-018, AI-008: true when the AI may run now in `env`, so a page shows AI-018's switched-on
/// wording; false while the switch is off.
///
/// # Errors
///
/// [`AiError::Database`] when the switch records cannot be read.
pub async fn ai_on(db: &mut PgConnection, env: Environment) -> Result<bool, AiError> {
    Ok(ai_mode(db, env).await? == AiMode::Ai)
}

#[cfg(test)]
mod tests {
    //! AI-018 (TD-TERMS-04): the wrapper follows `ai_mode`. Fake data only.

    use askhava_core::api::Environment;
    use sqlx::PgPool;

    use super::ai_on;

    /// TD-TERMS-04 (AI-018): with no AI records, live is off and the stand-in environments are on,
    /// as `ai_mode` decides.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_terms_04_ai_on_follows_the_switch(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        assert!(!ai_on(&mut db, Environment::Live).await.expect("live"));
        assert!(ai_on(&mut db, Environment::Dev).await.expect("dev"));
    }
}
