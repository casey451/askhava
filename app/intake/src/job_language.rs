//! The language a homeowner page or text about a job is shown in (TXT-039, CON-026, PICK-016;
//! D-4052 item B42).
//!
//! [`job_language`] finds the request that holds the job (the lookup `job_texts` uses: the
//! `job.<trade>.<property>` answer naming the job, or the request's first job), reads that
//! request's stored language (`service::request_language`) and the stored "Spanish conversations"
//! switch (`messaging::api::spanish_switch`), and answers Spanish only when both say so. A job no
//! request holds is English. `portals`' pick review step reads it; T590's texts will too.

use askhava_messaging::api::{Language, spanish_switch};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, IntakeError};
use crate::rules::consent_language;
use crate::service::request_language;

/// TXT-039, CON-026, PICK-016, D-4052 (B42): the language for job `job`'s homeowner: Spanish only
/// when the request holding it is Spanish and Spanish conversations are on; English otherwise,
/// and English when no request holds the job.
///
/// # Errors
///
/// [`IntakeError::Database`] when a read or the switch fails; [`IntakeError::UnknownRequest`]
/// when the request row is gone.
pub async fn job_language(db: &mut PgConnection, job: Uuid) -> Result<Language, IntakeError> {
    let Some((conversation, _)) = models::request_of_job(db, job).await? else {
        return Ok(Language::En);
    };
    let asked = request_language(db, conversation).await?;
    let switch = spanish_switch(db)
        .await
        .map_err(|_| IntakeError::Database)?;
    Ok(consent_language(asked, switch.is_on()))
}
