//! The inbox check after sign-up and the sample pick step (BIZ-042; T149, D-328, D-352, D-380,
//! D-462).
//!
//! Once a person's address is verified, [`send_sample_card`] queues one [`SAMPLE_CARD_KIND`]
//! task (IDs only, no job) whose sender renders [`sample_card_words`] (fake words, from
//! `core::api::brand()`) and sends it through `messaging` from the same sender name and address as
//! every business notice (SEC-025, D-1987); a resend is the same call. [`record_inbox_answer`] keeps the answer to "Did it arrive in your inbox?";
//! [`record_ok_reply`] marks an OK reply to the sample, shown as "Reply received", never a
//! command and never a help request. The sample pick step is offered the first time an owner or
//! "Confirm and billing" person opens a real job card ([`offer_sample_pick`], never during
//! sign-up); [`send_sample_pick`] queues [`SAMPLE_PICK_KIND`] ([`sample_pick_words`]) and [`record_pick_alert`] keeps a
//! Yes for [`PICK_ALERT_YES_LIFE`], after which [`queue_pick_retests`] sends the re-test email.
//! Nothing here reads or changes the account status, so none of it blocks activation, and the
//! samples are not job cards, so no card count sees them. The words of each mail-app step live in
//! `docs/copy/mail-app-steps.md`, read by `portals::email_check`.

use askhava_core::api::{NewTask, brand, enqueue};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::{Connection, PgConnection};

use crate::email_verification::EmailCheckError;
use crate::team::Acting;

/// The task whose sender emails the sample card ([`sample_card_words`]) to `to` (BIZ-042).
pub const SAMPLE_CARD_KIND: &str = "businesses.sample_card";
/// The task whose sender emails the sample pick ([`sample_pick_words`]) to `to` (BIZ-042).
pub const SAMPLE_PICK_KIND: &str = "businesses.sample_pick";

fn short(spanish: bool) -> &'static str {
    if spanish {
        brand().es().short_name()
    } else {
        brand().short_name()
    }
}

/// BIZ-042, D-1987: the sample card email's subject and body: "<brand>: Sample job card (not a
/// real job)", made-up words only, in English or the Spanish draft.
#[must_use]
pub fn sample_card_words(spanish: bool) -> [String; 2] {
    let b = short(spanish);
    if spanish {
        return [
            format!("{b}: Tarjeta de trabajo de muestra (no es un trabajo real)"),
            "Es inventada, para que vea dónde llegan nuestros correos. Vuelva a la página de \
             registro y díganos: ¿llegó a su bandeja de entrada?"
                .to_owned(),
        ];
    }
    [
        format!("{b}: Sample job card (not a real job)"),
        "This one is made up, so you can see where our emails land. Go back to the sign-up page \
         and tell us: did it arrive in your inbox?"
            .to_owned(),
    ]
}

/// BIZ-042, D-1987: the sample pick email's subject and body: "<brand>: Sample pick (not a real
/// job). Confirm by <time>." with `time` as the sender writes it (Arizona time).
#[must_use]
pub fn sample_pick_words(time: &str, spanish: bool) -> [String; 2] {
    let b = short(spanish);
    if spanish {
        return [
            format!("{b}: Elección de muestra (no es un trabajo real). Confirme antes de {time}."),
            "Es solo una prueba, no hay nada que confirmar. Vuelva a la página del trabajo y \
             díganos: ¿le avisó su teléfono?"
                .to_owned(),
        ];
    }
    [
        format!("{b}: Sample pick (not a real job). Confirm by {time}."),
        "This is only a test, so there is nothing to confirm. Go back to the job page and tell \
         us: did your phone alert you?"
            .to_owned(),
    ]
}
/// How long a Yes to "Did your phone alert you?" lasts before the re-test (BIZ-042, D-380).
pub const PICK_ALERT_YES_LIFE: SignedDuration = SignedDuration::from_hours(90 * 24);
/// The sample pick's "Confirm by" time: 4 business hours, as a real pick (PICK-019).
const SAMPLE_CONFIRM_IN: SignedDuration = SignedDuration::from_hours(4);

/// The answer to "Did it arrive in your inbox?" (BIZ-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxAnswer {
    /// Inbox: no steps.
    Inbox,
    /// Spam or Junk.
    SpamOrJunk,
    /// Promotions or another tab.
    OtherTab,
    /// Not yet.
    NotYet,
}

impl InboxAnswer {
    /// Every answer, in the order the screen offers them.
    pub const ALL: [Self; 4] = [Self::Inbox, Self::SpamOrJunk, Self::OtherTab, Self::NotYet];

    /// The stored and posted code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::SpamOrJunk => "spam",
            Self::OtherTab => "tab",
            Self::NotYet => "not_yet",
        }
    }

    /// The answer for a posted or stored code; `None` for anything else.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.code() == code)
    }

    /// BIZ-042: every answer but Inbox shows the mail app's steps and a resend.
    #[must_use]
    pub const fn shows_steps(self) -> bool {
        !matches!(self, Self::Inbox)
    }
}

/// One person's inbox check as the screen shows it (BIZ-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmailCheck {
    /// The sample card was queued.
    pub sample_sent: bool,
    /// The last answer to "Did it arrive in your inbox?".
    pub answer: Option<InboxAnswer>,
    /// An OK reply to the sample came in ("Reply received").
    pub reply_received: bool,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, EmailCheckError> {
    r.map_err(|_| EmailCheckError::Database)
}

/// Queues one `kind` task to the person's main address (IDs and a time only, PRIV-001).
async fn queue(
    conn: &mut PgConnection,
    by: Acting,
    (kind, confirm_by): (&'static str, Option<i64>),
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query_scalar!(
        "SELECT mailbox_id FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL",
        by.member,
        by.business
    );
    let main = db(q.fetch_optional(&mut *conn).await)?.ok_or(EmailCheckError::UnknownMember)?;
    let payload = json!({"business": by.business, "member": by.member, "to": [main],
        "confirm_by": confirm_by});
    let mut task = NewTask::new(kind, payload, now);
    task.dedupe_key = Some(format!("{kind}.{}.{}", by.member, now.as_second()));
    enqueue(conn, &task)
        .await
        .map(|_| ())
        .map_err(|_| EmailCheckError::Database)
}

/// BIZ-042, SEC-025: queues the sample card (or its resend) to `by`'s main address and records
/// the send. It never touches the account status (activation never waits on it).
///
/// # Errors
/// [`EmailCheckError::UnknownMember`] or `Database`.
pub async fn send_sample_card(
    conn: &mut PgConnection,
    by: Acting,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    queue(conn, by, (SAMPLE_CARD_KIND, None), now).await?;
    let q = sqlx::query!(
        "INSERT INTO businesses.email_check (member_id, business_id, sample_sent_at) \
         VALUES ($1, $2, $3) ON CONFLICT (member_id) DO UPDATE SET sample_sent_at = $3",
        by.member,
        by.business,
        now.to_sqlx() as _
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-042: stores `by`'s answer to "Did it arrive in your inbox?".
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn record_inbox_answer(
    conn: &mut PgConnection,
    by: Acting,
    answer: InboxAnswer,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "INSERT INTO businesses.email_check (member_id, business_id, answer, answered_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (member_id) DO UPDATE SET answer = $3, \
         answered_at = $4",
        by.member,
        by.business,
        answer.code(),
        now.to_sqlx() as _
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-042, BIZ-038, D-352: an email reply of OK to `by`'s sample card. It only records the time
/// ("Reply received"); it runs no command and opens no help request.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn record_ok_reply(
    conn: &mut PgConnection,
    by: Acting,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "INSERT INTO businesses.email_check (member_id, business_id, ok_reply_at) \
         VALUES ($1, $2, $3) ON CONFLICT (member_id) DO UPDATE SET ok_reply_at = $3",
        by.member,
        by.business,
        now.to_sqlx() as _
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-042: `by`'s inbox check for the screen; the default when nothing was sent yet.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn email_check(
    conn: &mut PgConnection,
    by: Acting,
) -> Result<EmailCheck, EmailCheckError> {
    let q = sqlx::query!(
        "SELECT sample_sent_at IS NOT NULL AS \"sent!\", answer, ok_reply_at IS NOT NULL AS \
         \"ok!\" FROM businesses.email_check WHERE member_id = $1 AND business_id = $2",
        by.member,
        by.business
    );
    Ok(
        db(q.fetch_optional(conn).await)?.map_or_else(EmailCheck::default, |r| EmailCheck {
            sample_sent: r.sent,
            answer: r.answer.as_deref().and_then(InboxAnswer::from_code),
            reply_received: r.ok,
        }),
    )
}

/// BIZ-042, D-462: true exactly once per person, the first time an owner or "Confirm and billing"
/// member opens a real job card; never for a "Cards and quotes"-only member.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn offer_sample_pick(
    conn: &mut PgConnection,
    by: Acting,
    now: Timestamp,
) -> Result<bool, EmailCheckError> {
    let q = sqlx::query_scalar!(
        "INSERT INTO businesses.email_check (member_id, business_id, pick_step_offered_at) \
         SELECT id, business_id, $3 FROM businesses.team_member WHERE id = $1 \
         AND business_id = $2 AND removed_at IS NULL AND confirm_and_billing \
         ON CONFLICT (member_id) DO UPDATE SET pick_step_offered_at = $3 \
         WHERE businesses.email_check.pick_step_offered_at IS NULL RETURNING member_id",
        by.member,
        by.business,
        now.to_sqlx() as _
    );
    Ok(db(q.fetch_optional(conn).await)?.is_some())
}

/// BIZ-042: queues the sample pick (or its resend) to `by`'s main address, "Confirm by" 4 hours on.
///
/// # Errors
/// [`EmailCheckError::UnknownMember`] or `Database`.
pub async fn send_sample_pick(
    conn: &mut PgConnection,
    by: Acting,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let by_time = (now + SAMPLE_CONFIRM_IN).as_second();
    queue(conn, by, (SAMPLE_PICK_KIND, Some(by_time)), now).await
}

/// BIZ-042: stores the answer to "Did your phone alert you?"; a No clears an earlier Yes so the
/// steps show again.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn record_pick_alert(
    conn: &mut PgConnection,
    by: Acting,
    yes: bool,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let at = yes.then_some(now.to_sqlx());
    let q = sqlx::query!(
        "INSERT INTO businesses.email_check (member_id, business_id, pick_alert_yes_at) \
         VALUES ($1, $2, $3) ON CONFLICT (member_id) DO UPDATE SET pick_alert_yes_at = $3",
        by.member,
        by.business,
        at as _
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-042, D-380: for every current person whose Yes is older than `life` (a setting, default
/// [`PICK_ALERT_YES_LIFE`]), clears the Yes and queues the sample pick again as the re-test email.
/// Returns how many were queued; safe to run twice.
///
/// # Errors
/// [`EmailCheckError::Database`] or `UnknownMember`.
pub async fn queue_pick_retests(
    conn: &mut PgConnection,
    life: SignedDuration,
    now: Timestamp,
) -> Result<usize, EmailCheckError> {
    let before = now
        .checked_sub(life)
        .map_err(|_| EmailCheckError::Database)?;
    let q = sqlx::query!(
        "UPDATE businesses.email_check c SET pick_alert_yes_at = NULL \
         FROM businesses.team_member m WHERE m.id = c.member_id AND m.removed_at IS NULL \
         AND c.pick_alert_yes_at <= $1 RETURNING c.member_id, c.business_id",
        before.to_sqlx() as _
    );
    let mut tx = db(conn.begin().await)?;
    let due = db(q.fetch_all(&mut *tx).await)?;
    for r in &due {
        let by = Acting {
            business: r.business_id,
            member: r.member_id,
        };
        send_sample_pick(&mut tx, by, now).await?;
    }
    db(tx.commit().await)?;
    Ok(due.len())
}
