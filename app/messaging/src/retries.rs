//! Text retries and what happens when they run out (TXT-014, TXT-015, INT-023, D-1378).
//!
//! Every failed text is retried 3 times, 10, 20 and 30 minutes after its first failure
//! ([`next_step`]); the caller sends each retry through `optout::send_checked`, under quiet hours
//! (QH-008). [`record_failure`] counts a failed send (from the row's claims, so a repeat call
//! counts nothing twice) and answers the next step. When the third retry fails, [`give_up`] marks
//! the text `failed` in one transaction: an important text (quote link, who's calling, missed
//! call, a business's confirm request) is listed for the portal and, with an email on file, goes
//! by email through `messaging.email_outbox`; a homeowner with no email on file becomes a call-back
//! alert for Casey, due in 24 hours for an urgent job and in 4 days (a setting) otherwise.

use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::email::{
    EmailClass, NewEmail, Notice, PostalAddress, Recipient, audience_kind, homeowner_email,
    queue_email,
};
use crate::models::{self, EmailError, FailedText, OutboxError};
use crate::templates::Audience;

/// When each of the 3 retries is due, counted from the first failure (TXT-014, TXT-015).
pub const RETRY_AFTER: [SignedDuration; 3] = [
    SignedDuration::from_mins(10),
    SignedDuration::from_mins(20),
    SignedDuration::from_mins(30),
];

/// The texts that go by email and show in the portal when their retries fail (TXT-014): the
/// quote link and the missed-call text. The who's-calling text and a business's confirm request
/// join this list in the tasks that add their templates.
pub const IMPORTANT_TEMPLATES: [&str; 2] = ["first_quote", "caller_missed"];

/// The template ID of the email an important text becomes (TXT-014).
pub const TEXT_FAILED_EMAIL: &str = "text_failed";

/// The default call-back time for a job that is not urgent: 4 days (INT-023, a setting).
pub const NO_RUSH_CALL_BACK: SignedDuration = SignedDuration::from_hours(96);

/// The ops alert kind of a call-back for Casey (TXT-015, INT-023).
pub const CALL_BACK_ALERT: &str = "messaging.call_back_due";

/// What to do after a failed send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Retry at this time (before quiet hours move it, QH-008).
    RetryAt(Timestamp),
    /// The 3 retries failed: call [`give_up`].
    GiveUp,
}

/// The step after `failures` failed sends of a text whose first failure was at `first_failed`
/// (TXT-014, TXT-015): the first send and 3 retries, then give up.
#[must_use]
pub fn next_step(first_failed: Timestamp, failures: u32) -> Step {
    usize::try_from(failures)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|n| RETRY_AFTER.get(n))
        .map_or(Step::GiveUp, |after| Step::RetryAt(first_failed + *after))
}

/// How soon a homeowner's job needs her (INT-023): "Today, it's urgent", an emergency or a
/// 911-list request before "How soon?" was answered are `Urgent`; anything else is `NoRush`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// Call back within 24 hours.
    Urgent,
    /// Call back within the no-rush setting.
    NoRush,
}

/// When a call-back for a homeowner unreachable at `at` is due (INT-023, D-226): 24 hours for an
/// urgent job, `no_rush` (default [`NO_RUSH_CALL_BACK`]) otherwise.
#[must_use]
pub fn call_back_due(at: Timestamp, urgency: Urgency, no_rush: SignedDuration) -> Timestamp {
    match urgency {
        Urgency::Urgent => at + SignedDuration::from_hours(24),
        Urgency::NoRush => at + no_rush,
    }
}

/// Counts outbox text `id`'s failed send at `now` and answers the next step (TXT-015); records
/// when the retry is due.
///
/// # Errors
///
/// [`OutboxError::NotFound`] for a row that is sent, blocked, failed or ended its retries;
/// [`OutboxError::Database`].
pub async fn record_failure(
    db: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<Step, OutboxError> {
    let (failures, first) = models::record_failure(db, id, now)
        .await?
        .ok_or(OutboxError::NotFound)?;
    let step = next_step(first, u32::try_from(failures).unwrap_or(u32::MAX));
    if let Step::RetryAt(at) = step {
        models::set_next_retry(db, id, at).await?;
    }
    Ok(step)
}

/// Who a failed text was for, as [`give_up`] needs it.
#[derive(Clone, Copy)]
pub enum Reach<'a> {
    /// A homeowner: her email is looked up in `vault`; `urgency` sets a call-back's due time.
    Homeowner {
        /// The vault her email is read from.
        vault: &'a Vault,
        /// How soon her job needs her (INT-023).
        urgency: Urgency,
        /// The no-rush call-back time (the INT-023 setting).
        no_rush: SignedDuration,
    },
    /// A business: its sign-in email is always on file.
    Business,
}

/// How a text's retries ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaveUp {
    /// The important text was queued as this email and shown in the portal (TXT-014).
    Emailed(Uuid),
    /// Casey has a call-back alert, due then (TXT-015, INT-023).
    CallBack(Timestamp),
    /// Nothing more to send: a text that was not important and someone reachable another way.
    Failed,
}

/// Gives up outbox text `id` after its retries, at `now`, in one transaction (TXT-014, TXT-015):
/// marks it failed, lists an important one for the portal and emails it when an address is on
/// file, and raises a call-back alert for a homeowner with none. `to` is the email recipient
/// (its `id` is ignored: the row's recipient is used; its `audience` must be the kind `reach`
/// names, so the email's footer and address lookup match the text's recipient). `None` when the
/// text was already sent, blocked or given up.
///
/// # Errors
///
/// [`OutboxError::RecipientKind`] when `reach` or `to.audience` names a kind other than the
/// text's (nothing is changed), [`OutboxError::Template`]
/// or [`OutboxError::Database`].
pub async fn give_up(
    db: &mut PgConnection,
    (secret, postal): (&SecretString, &PostalAddress),
    (id, reach, to): (Uuid, Reach<'_>, &Recipient<'_>),
    now: Timestamp,
) -> Result<Option<GaveUp>, OutboxError> {
    if audience_kind(to.audience) != reach_kind(reach) {
        return Err(OutboxError::RecipientKind);
    }
    let Some((recipient, ..)) = models::outbox_target(db, id).await? else {
        return Ok(None);
    };
    let on_file = email_on_file(db, reach, recipient).await?;
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    let Some(text) = models::fail_text(&mut tx, id).await? else {
        return Ok(None);
    };
    if reach_kind(reach) != text.kind || text.recipient != recipient {
        return Err(OutboxError::RecipientKind);
    }
    let important = IMPORTANT_TEMPLATES.contains(&text.template_id.as_str());
    if important {
        models::insert_portal_notice(&mut tx, id, &text, now).await?;
    }
    let to = Recipient {
        id: text.recipient,
        ..*to
    };
    let outcome = match (important && on_file, reach) {
        (true, _) => {
            GaveUp::Emailed(email_text(&mut tx, (secret, postal), (id, &text, &to), now).await?)
        }
        (
            false,
            Reach::Homeowner {
                urgency, no_rush, ..
            },
        ) if !on_file => {
            let due = call_back_due(now, urgency, no_rush);
            GaveUp::CallBack(call_back(&mut tx, id, (due, urgency)).await?)
        }
        (false, _) => GaveUp::Failed,
    };
    if outcome == GaveUp::Failed {
        models::end_retry(&mut tx, id, ("failed", None)).await?;
    }
    tx.commit().await.map_err(|_| OutboxError::Database)?;
    Ok(Some(outcome))
}

/// The stored recipient kind a `reach` names.
fn reach_kind(reach: Reach<'_>) -> &'static str {
    audience_kind(match reach {
        Reach::Homeowner { .. } => Audience::Homeowner,
        Reach::Business => Audience::Business,
    })
}

/// Whether `recipient` has an email on file: a business always does; a homeowner's is looked up
/// in the vault, outside any transaction (the vault refuses a read inside one, PRIV-003).
async fn email_on_file(
    db: &mut PgConnection,
    reach: Reach<'_>,
    recipient: Uuid,
) -> Result<bool, OutboxError> {
    match reach {
        Reach::Business => Ok(true),
        Reach::Homeowner { vault, .. } => Ok(homeowner_email(db, vault, recipient)
            .await
            .map_err(|_| OutboxError::Recipient)?
            .is_some()),
    }
}

/// Queues the failed important text as an email (TXT-014) and records the outcome.
async fn email_text(
    db: &mut PgConnection,
    keys: (&SecretString, &PostalAddress),
    (id, text, to): (Uuid, &FailedText, &Recipient<'_>),
    now: Timestamp,
) -> Result<Uuid, OutboxError> {
    let email = NewEmail {
        template_id: TEXT_FAILED_EMAIL,
        subject: &[id],
        class: EmailClass::Notice,
        notice: Notice::TextFailed(&text.body),
    };
    let (email_id, _) = queue_email(db, keys, to, &email, now)
        .await
        .map_err(|e| match e {
            EmailError::Database => OutboxError::Database,
            _ => OutboxError::Template,
        })?;
    models::end_retry(db, id, ("emailed", None)).await?;
    Ok(email_id)
}

/// Raises Casey's call-back alert for failed text `id`, due at `due` (TXT-015, INT-023); an urgent
/// one is High so it reaches the OPS-032 push.
async fn call_back(
    db: &mut PgConnection,
    id: Uuid,
    (due, urgency): (Timestamp, Urgency),
) -> Result<Timestamp, OutboxError> {
    models::end_retry(db, id, ("call_back", Some(due))).await?;
    let severity = match urgency {
        Urgency::Urgent => Severity::High,
        Urgency::NoRush => Severity::Normal,
    };
    let alert = NewAlert {
        kind: CALL_BACK_ALERT,
        severity,
        subject: Some(("messaging.outbox", id)),
    };
    raise_alert(db, &alert)
        .await
        .map_err(|_| OutboxError::Database)?;
    Ok(due)
}
