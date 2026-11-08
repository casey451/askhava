//! The "Emergency emails at any hour" switch, its test emergency email, and the conditions on the
//! after-hours tick (BIZ-007, BIZ-037, BIZ-042, CARD-021, INT-033; T150, D-346, D-362, D-491).
//!
//! Each person's switch lives in `businesses.emergency_switch` with the last test email and the
//! last Yes to "Did your phone alert you with Do Not Disturb or Sleep on?". A person *covers* the
//! business when they can confirm (the owner or "Confirm and billing"), are current, have the
//! switch on and a Yes younger than the life given (90 days, [`EMERGENCY_YES_LIFE`]).
//! [`set_after_hours`] ticks after-hours only while someone covers; [`recheck_after_hours`]
//! removes the tick and queues the owner's email ([`AFTER_HOURS_OFF_KIND`]) when the last person
//! who had the switch on turns it off, answers No or is removed; [`queue_emergency_retests`]
//! clears a Yes at 90 days and queues the re-test, and the business stops counting until a new
//! Yes; [`suspend_after_hours`] takes a business out of the count after two unvisited night cards
//! ([`missed_in_a_row`]) until a new Yes. [`night_count`] is INT-033's narrowed count and
//! [`night_people`] who gets an emergency email at night. No address or phone is stored here.

use askhava_core::api::{NewTask, brand, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::team::Acting;

/// The task whose sender emails the test emergency email ([`emergency_test_words`]) (BIZ-042).
pub const EMERGENCY_TEST_KIND: &str = "businesses.emergency_test";
/// The owner's email when the after-hours tick is removed or suspended (BIZ-007).
pub const AFTER_HOURS_OFF_KIND: &str = "businesses.after_hours_off";
/// How long a Yes after the test emergency email counts (BIZ-042, D-362; a setting's default).
pub const EMERGENCY_YES_LIFE: SignedDuration = SignedDuration::from_hours(90 * 24);
/// How long after a night EMERGENCY card a visit to its own link pages still counts (BIZ-007).
pub const VISIT_WITHIN: SignedDuration = SignedDuration::from_mins(60);
/// Unvisited night EMERGENCY cards in a row that suspend the tick (BIZ-007; a setting's default).
pub const MISSED_IN_A_ROW: usize = 2;

/// Why a switch or tick change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SwitchError {
    /// A database call failed.
    #[error("emergency switch database call failed")]
    Database,
    /// No such current member of the business.
    #[error("unknown team member")]
    UnknownMember,
    /// BIZ-007: nobody who can confirm has the switch on with a current Yes.
    #[error("no one covers after-hours emergencies")]
    NoCover,
    /// BIZ-042: a Yes without a test emergency email sent first.
    #[error("no test emergency email was sent")]
    NoTest,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, SwitchError> {
    r.map_err(|_| SwitchError::Database)
}

/// BIZ-042, D-362: the test emergency email's subject and body, in English or the Spanish draft.
#[must_use]
pub fn emergency_test_words(spanish: bool) -> [String; 2] {
    if spanish {
        let b = brand().es().short_name();
        return [
            format!("{b}: EMERGENCY tarjeta de trabajo de muestra (no es un trabajo real)"),
            "Es solo una prueba, no un trabajo real. Vuelva a la página de correos de emergencia de \
             su cuenta y díganos: ¿le avisó su teléfono con No molestar o el modo Dormir activado?"
                .to_owned(),
        ];
    }
    let b = brand().short_name();
    [
        format!("{b}: EMERGENCY sample job card (not a real job)"),
        "This is only a test, not a real job. Go back to the Emergency emails page in your \
         account and tell us: did your phone alert you with Do Not Disturb or Sleep on?"
            .to_owned(),
    ]
}

/// BIZ-042: a Yes given at `yes_at` still counts at `now`: day 89 23:59:59 counts, day 90 not.
#[must_use]
pub fn yes_current(yes_at: Timestamp, now: Timestamp, life: SignedDuration) -> bool {
    now.duration_since(yes_at) < life
}

/// BIZ-007: true when the last `in_a_row` night EMERGENCY cards each got no own-link visit within
/// [`VISIT_WITHIN`] (`None` is no visit; a visit at 59:59 keeps the tick, one at 60:00 does not).
#[must_use]
pub fn missed_in_a_row(visits_after_send: &[Option<SignedDuration>], in_a_row: usize) -> bool {
    in_a_row > 0
        && visits_after_send.len() >= in_a_row
        && visits_after_send[visits_after_send.len() - in_a_row..]
            .iter()
            .all(|v| v.is_none_or(|d| d >= VISIT_WITHIN))
}

async fn log(
    tx: &mut PgConnection,
    business: Uuid,
    action: &'static str,
) -> Result<(), SwitchError> {
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action,
        subject_kind: "businesses.business",
        subject_id: business,
        change: Change::Field("after_hours"),
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| SwitchError::Database)
}

async fn queue(
    tx: &mut PgConnection,
    (kind, business, member): (&'static str, Uuid, Uuid),
    now: Timestamp,
) -> Result<(), SwitchError> {
    let payload = json!({"business": business, "member": member});
    let mut task = NewTask::new(kind, payload, now);
    task.dedupe_key = Some(format!("{kind}.{member}.{}", now.as_second()));
    enqueue(tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| SwitchError::Database)
}

/// BIZ-007: whether someone who can confirm covers `business` at `now` (switch on, Yes within `life`).
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn covered(
    conn: &mut PgConnection,
    business: Uuid,
    (now, life): (Timestamp, SignedDuration),
) -> Result<bool, SwitchError> {
    let since = now.checked_sub(life).map_err(|_| SwitchError::Database)?;
    let q = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.emergency_switch s \
         JOIN businesses.team_member m ON m.id = s.member_id AND m.business_id = s.business_id \
         WHERE s.business_id = $1 AND s.switch_on AND m.removed_at IS NULL \
         AND m.confirm_and_billing AND s.yes_at > $2) AS \"c!\"",
        business,
        since.to_sqlx() as _
    );
    db(q.fetch_one(conn).await)
}

/// BIZ-007: ticks or unticks after-hours emergencies; ticking is refused unless [`covered`]. A
/// "Cards and quotes" member alone never covers.
///
/// # Errors
/// [`SwitchError::NoCover`] or `Database`.
pub async fn set_after_hours(
    conn: &mut PgConnection,
    business: Uuid,
    on: bool,
    (now, life): (Timestamp, SignedDuration),
) -> Result<(), SwitchError> {
    let mut tx = db(conn.begin().await)?;
    if on && !covered(&mut tx, business, (now, life)).await? {
        return Err(SwitchError::NoCover);
    }
    let q = sqlx::query!(
        "UPDATE businesses.business SET after_hours = $2, after_hours_suspended_at = NULL \
         WHERE id = $1 AND after_hours <> $2",
        business,
        on
    );
    if db(q.execute(&mut *tx).await)?.rows_affected() > 0 {
        log(&mut tx, business, "businesses.after_hours_set").await?;
    }
    db(tx.commit().await)
}

/// BIZ-007: when the business is ticked and nobody who can confirm has the switch on any more
/// (turned off, a No, or removed), removes the tick and queues the owner's email. True if removed.
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn recheck_after_hours(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<bool, SwitchError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query_scalar!(
        "UPDATE businesses.business b SET after_hours = false WHERE b.id = $1 AND b.after_hours \
         AND NOT EXISTS (SELECT 1 FROM businesses.emergency_switch s \
         JOIN businesses.team_member m ON m.id = s.member_id AND m.business_id = s.business_id \
         WHERE s.business_id = b.id AND s.switch_on AND s.yes_at IS NOT NULL \
         AND m.removed_at IS NULL AND m.confirm_and_billing) \
         RETURNING (SELECT id FROM businesses.team_member WHERE business_id = b.id AND is_owner)",
        business
    );
    let Some(owner) = db(q.fetch_optional(&mut *tx).await)? else {
        return Ok(false);
    };
    log(&mut tx, business, "businesses.after_hours_removed").await?;
    let owner = owner.ok_or(SwitchError::UnknownMember)?;
    queue(&mut tx, (AFTER_HOURS_OFF_KIND, business, owner), now).await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// BIZ-007: true when turning `by`'s switch off would remove the tick (the warning first).
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn switch_off_warns(conn: &mut PgConnection, by: Acting) -> Result<bool, SwitchError> {
    let q = sqlx::query_scalar!(
        "SELECT b.after_hours AND NOT EXISTS (SELECT 1 FROM businesses.emergency_switch s \
         JOIN businesses.team_member m ON m.id = s.member_id AND m.business_id = s.business_id \
         WHERE s.business_id = b.id AND s.member_id <> $2 AND s.switch_on \
         AND s.yes_at IS NOT NULL AND m.removed_at IS NULL AND m.confirm_and_billing) AS \"w!\" \
         FROM businesses.business b WHERE b.id = $1",
        by.business,
        by.member
    );
    Ok(db(q.fetch_optional(conn).await)?.unwrap_or(false))
}

/// BIZ-037, BIZ-007: turns `by`'s "Emergency emails at any hour" on or off; off clears the Yes
/// (a new test is needed) and may remove the tick ([`recheck_after_hours`]).
///
/// # Errors
/// [`SwitchError::UnknownMember`] or `Database`.
pub async fn set_emergency_emails(
    conn: &mut PgConnection,
    by: Acting,
    on: bool,
    now: Timestamp,
) -> Result<(), SwitchError> {
    let q = sqlx::query!(
        "INSERT INTO businesses.emergency_switch (member_id, business_id, switch_on) \
         SELECT id, business_id, $3 FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL \
         ON CONFLICT (member_id) DO UPDATE SET switch_on = $3, \
         yes_at = CASE WHEN $3 THEN businesses.emergency_switch.yes_at END",
        by.member,
        by.business,
        on
    );
    if db(q.execute(&mut *conn).await)?.rows_affected() == 0 {
        return Err(SwitchError::UnknownMember);
    }
    if !on {
        recheck_after_hours(conn, by.business, now).await?;
    }
    Ok(())
}

/// BIZ-042: queues the test emergency email to `by` (switch on only) and records the send.
///
/// # Errors
/// [`SwitchError::UnknownMember`] (also with the switch off) or `Database`.
pub async fn send_emergency_test(
    conn: &mut PgConnection,
    by: Acting,
    now: Timestamp,
) -> Result<(), SwitchError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query!(
        "UPDATE businesses.emergency_switch s SET test_sent_at = $3 FROM businesses.team_member m \
         WHERE s.member_id = $1 AND s.business_id = $2 AND s.switch_on \
         AND m.id = s.member_id AND m.removed_at IS NULL",
        by.member,
        by.business,
        now.to_sqlx() as _
    );
    if db(q.execute(&mut *tx).await)?.rows_affected() == 0 {
        return Err(SwitchError::UnknownMember);
    }
    queue(&mut tx, (EMERGENCY_TEST_KIND, by.business, by.member), now).await?;
    db(tx.commit().await)
}

/// BIZ-042, D-362: stores the answer to "Did your phone alert you with Do Not Disturb or Sleep
/// on?". A Yes needs a test sent and clears a suspension; a No clears the Yes and may remove the tick.
///
/// # Errors
/// [`SwitchError::NoTest`] or `Database`.
pub async fn record_alert_answer(
    conn: &mut PgConnection,
    by: Acting,
    yes: bool,
    now: Timestamp,
) -> Result<(), SwitchError> {
    let q = sqlx::query!(
        "UPDATE businesses.emergency_switch SET yes_at = $3 WHERE member_id = $1 \
         AND business_id = $2 AND switch_on AND test_sent_at IS NOT NULL",
        by.member,
        by.business,
        yes.then_some(now.to_sqlx()) as _
    );
    if db(q.execute(&mut *conn).await)?.rows_affected() == 0 {
        return Err(SwitchError::NoTest);
    }
    if !yes {
        recheck_after_hours(conn, by.business, now).await?;
        return Ok(());
    }
    let q = sqlx::query!(
        "UPDATE businesses.business SET after_hours_suspended_at = NULL WHERE id = $1",
        by.business
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-042, D-362: clears every Yes older than `life` and queues the re-test email; the business
/// stops counting until a new Yes, its tick waiting. Returns how many; safe to run twice.
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn queue_emergency_retests(
    conn: &mut PgConnection,
    life: SignedDuration,
    now: Timestamp,
) -> Result<usize, SwitchError> {
    let before = now.checked_sub(life).map_err(|_| SwitchError::Database)?;
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query!(
        "UPDATE businesses.emergency_switch SET yes_at = NULL WHERE switch_on AND yes_at <= $1 \
         RETURNING member_id, business_id",
        before.to_sqlx() as _
    );
    let due = db(q.fetch_all(&mut *tx).await)?;
    for r in &due {
        queue(
            &mut tx,
            (EMERGENCY_TEST_KIND, r.business_id, r.member_id),
            now,
        )
        .await?;
    }
    db(tx.commit().await)?;
    Ok(due.len())
}

/// BIZ-007, D-362: suspends the tick after [`missed_in_a_row`] and queues the owner's email with
/// the re-test link; the business leaves the INT-033 count until a new Yes. True if newly suspended.
///
/// # Errors
/// [`SwitchError::Database`] or `UnknownMember` (no owner).
pub async fn suspend_after_hours(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<bool, SwitchError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query_scalar!(
        "UPDATE businesses.business b SET after_hours_suspended_at = $2 WHERE b.id = $1 \
         AND b.after_hours AND b.after_hours_suspended_at IS NULL \
         RETURNING (SELECT id FROM businesses.team_member WHERE business_id = b.id AND is_owner)",
        business,
        now.to_sqlx() as _
    );
    let Some(owner) = db(q.fetch_optional(&mut *tx).await)? else {
        return Ok(false);
    };
    log(&mut tx, business, "businesses.after_hours_suspended").await?;
    let owner = owner.ok_or(SwitchError::UnknownMember)?;
    queue(&mut tx, (AFTER_HOURS_OFF_KIND, business, owner), now).await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// INT-033, BIZ-007, TD-BIZ-24: active businesses ticked after-hours, not suspended, serving
/// `town` with a job type in `category`, that someone who can confirm [`covered`] at `now`.
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn night_count(
    conn: &mut PgConnection,
    (category, town): (&str, &str),
    (now, life): (Timestamp, SignedDuration),
) -> Result<u32, SwitchError> {
    let since = now.checked_sub(life).map_err(|_| SwitchError::Database)?;
    let q = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.business b WHERE b.status = 'active' \
         AND b.after_hours AND b.after_hours_suspended_at IS NULL \
         AND EXISTS (SELECT 1 FROM businesses.town t WHERE t.business_id = b.id AND t.town = $2) \
         AND EXISTS (SELECT 1 FROM businesses.job_type_choice c \
                     WHERE c.business_id = b.id AND c.category = $1) \
         AND EXISTS (SELECT 1 FROM businesses.emergency_switch s JOIN businesses.team_member m \
                     ON m.id = s.member_id AND m.business_id = s.business_id \
                     WHERE s.business_id = b.id AND s.switch_on AND m.removed_at IS NULL \
                     AND m.confirm_and_billing AND s.yes_at > $3)",
        category,
        town,
        since.to_sqlx() as _
    );
    let n = db(q.fetch_one(conn).await)?;
    u32::try_from(n).map_err(|_| SwitchError::Database)
}

/// One person's switch as their page shows it (BIZ-037, BIZ-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwitchState {
    /// "Emergency emails at any hour" is on.
    pub on: bool,
    /// A test emergency email was queued.
    pub test_sent: bool,
    /// The last Yes still counts at the time asked.
    pub yes_current: bool,
}

/// BIZ-037, BIZ-042: `by`'s switch at `now`; the default when they never set it.
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn switch_state(
    conn: &mut PgConnection,
    by: Acting,
    (now, life): (Timestamp, SignedDuration),
) -> Result<SwitchState, SwitchError> {
    let q = sqlx::query!(
        "SELECT switch_on, test_sent_at IS NOT NULL AS \"sent!\", yes_at FROM \
         businesses.emergency_switch WHERE member_id = $1 AND business_id = $2",
        by.member,
        by.business
    );
    Ok(
        db(q.fetch_optional(conn).await)?.map_or_else(SwitchState::default, |r| SwitchState {
            on: r.switch_on,
            test_sent: r.sent,
            yes_current: r
                .yes_at
                .is_some_and(|y| yes_current(y.to_jiff(), now, life)),
        }),
    )
}

/// One person who gets an emergency email at night (CARD-021, BIZ-037): opaque IDs and roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightPerson {
    /// The team member.
    pub member: Uuid,
    /// The opaque ID of their sign-in email (PRIV-001).
    pub mailbox: Uuid,
    /// Holds "Cards and quotes" (gets cards).
    pub cards: bool,
    /// Holds "Confirm and billing" (gets picks).
    pub picks: bool,
}

/// CARD-021, BIZ-037, PICK-037: the current people of `business` with the switch on, empty unless
/// the business is ticked after-hours and not suspended; they get emergency emails at night.
///
/// # Errors
/// [`SwitchError::Database`].
pub async fn night_people(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<NightPerson>, SwitchError> {
    let q = sqlx::query!(
        "SELECT m.id, m.mailbox_id, m.cards_and_quotes, m.confirm_and_billing \
         FROM businesses.emergency_switch s JOIN businesses.team_member m \
         ON m.id = s.member_id AND m.business_id = s.business_id \
         JOIN businesses.business b ON b.id = s.business_id \
         WHERE s.business_id = $1 AND s.switch_on AND m.removed_at IS NULL \
         AND b.after_hours AND b.after_hours_suspended_at IS NULL ORDER BY m.is_owner DESC, m.id",
        business
    );
    let rows = db(q.fetch_all(conn).await)?;
    Ok(rows
        .into_iter()
        .map(|r| NightPerson {
            member: r.id,
            mailbox: r.mailbox_id,
            cards: r.cards_and_quotes,
            picks: r.confirm_and_billing,
        })
        .collect())
}
