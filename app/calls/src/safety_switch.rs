//! The stored owner-portal "safety voice call" switch and Casey's recorded TCPA attorney approval
//! (CALL-019, CALL-022, CON-031, TXT-024; D-275, D-384, D-3134, T407).
//!
//! `calls.safety_switch` is append-only: each change is a new row and the newest is in force; no
//! row reads off, the launch state (D-384). [`safety_switch`] gives the stored [`SafetySwitch`]:
//! an "on" row is read back through [`SafetySwitch::turn_on`] with its approval and
//! [`SAFETY_CONDITIONS`], so a switch never reads on unless both still hold. The worker's
//! `calls.safety_call` handler and the inbound text route read it on every run and message.
//! [`turn_safety_switch_on`] stores an "on" row only when `turn_on` gives on for the approval and
//! the given [`Conditions`] (production passes [`SAFETY_CONDITIONS`]); [`turn_safety_switch_off`]
//! stores an "off" row. Each change writes one audit row in the same transaction, holding the
//! owner's ID, the row's ID and the from and to states only, never the reference (PRIV-028).
//! A request for the state already in force stores nothing. The owner portal (`portals`) asks
//! for the SEC-043 fresh factor before calling either.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::CallsError;
use crate::voice::{Approval, Conditions, SafetySwitch};

/// The audit action of a stored switch change (from and to are `off` and `on`).
pub const SAFETY_SWITCH_ACTION: &str = "calls.safety_switch_changed";

/// The CALL-022 items as built and tested on `main` (D-384, D-3134, D-4721). Each is true only
/// because a test covers it:
/// - `narrowed_trigger`: `tests/integration/tests/voice_calls.rs`
///   `td_call_09_only_danger_words_trigger_the_safety_call`.
/// - `recent_only`: `td_call_09_one_safety_call_from_a_non_main_number_and_press_9_stops_calls`
///   (a 31-minute-old message places no call; only her vault contact is dialled).
/// - `script`: `td_call_09_the_safety_script_is_short_and_generic`.
/// - `other_number`: `td_call_09_one_safety_call_from_a_non_main_number_and_press_9_stops_calls`.
/// - `list_checked`: `tests/integration/tests/number_pool.rs`
///   `td_con_28_no_automated_call_after_an_opt_out`.
/// - `consent_text`: `app/intake/src/tests/consent.rs` (TD-CON-18, the sentence only while on) and
///   `tests/integration/tests/safety_switch.rs` (TD-CALL-26, the route reads the stored switch).
/// - `logged`: `td_call_09_each_safety_call_is_claimed_once_and_every_key_and_result_is_logged`
///   and T378's answered and key routes, `tests/integration/tests/voice_call_plan.rs`
///   `td_call_22_a_safety_call_carries_its_twiml_and_a_person_acknowledges_once` and
///   `td_call_22_a_9_on_the_safety_key_route_lists_her_number`.
pub const SAFETY_CONDITIONS: Conditions = Conditions {
    narrowed_trigger: true,
    recent_only: true,
    script: true,
    other_number: true,
    list_checked: true,
    consent_text: true,
    logged: true,
};

/// CALL-022: the code names of the items of `met` that are false, in CALL-022's order; empty when
/// all are met. `portals` turns them into words on the settings page.
#[must_use]
pub fn unmet_conditions(met: &Conditions) -> Vec<&'static str> {
    [
        ("narrowed_trigger", met.narrowed_trigger),
        ("recent_only", met.recent_only),
        ("script", met.script),
        ("other_number", met.other_number),
        ("list_checked", met.list_checked),
        ("consent_text", met.consent_text),
        ("logged", met.logged),
    ]
    .into_iter()
    .filter_map(|(name, ok)| (!ok).then_some(name))
    .collect()
}

/// What a switch change did (CALL-019, CALL-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchChange {
    /// One new row and one audit row were stored.
    Changed,
    /// The switch was already in that state; nothing was stored.
    Unchanged,
    /// No approval with a reference was given; nothing was stored (CALL-019, D-275).
    NoApproval,
    /// A CALL-022 item is false; nothing was stored (D-384).
    Unmet,
}

/// The row in force: whether it is on, with the approval it holds.
async fn latest(conn: &mut PgConnection) -> Result<Option<Approval>, CallsError> {
    let row = sqlx::query!(
        r#"SELECT is_on, approved_on::text AS approved_on, reference
           FROM calls.safety_switch ORDER BY changed_at DESC, id LIMIT 1"#
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| CallsError::Database)?;
    let Some(row) = row.filter(|r| r.is_on) else {
        return Ok(None);
    };
    let date = row.approved_on.as_deref().map(str::parse::<Date>);
    match (date, row.reference) {
        (Some(Ok(date)), Some(reference)) => Ok(Some(Approval { date, reference })),
        _ => Err(CallsError::Database),
    }
}

/// CALL-019, CALL-022, D-384: the stored switch; off with no row, after an "off" row, or when
/// [`SafetySwitch::turn_on`] no longer gives on for the stored approval and [`SAFETY_CONDITIONS`].
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn safety_switch(conn: &mut PgConnection) -> Result<SafetySwitch, CallsError> {
    let approval = latest(conn).await?;
    Ok(SafetySwitch::turn_on(approval.as_ref(), &SAFETY_CONDITIONS)
        .unwrap_or_else(SafetySwitch::off))
}

/// The approval in force while the stored switch is on, for the settings page; `None` while off.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn safety_approval(conn: &mut PgConnection) -> Result<Option<Approval>, CallsError> {
    let on = safety_switch(conn).await?.is_on();
    Ok(if on { latest(conn).await? } else { None })
}

/// CALL-019, CALL-022, D-275, D-384: turns the switch on for `owner` at `now`, storing the
/// approval, only when its reference is 1 to 200 characters and [`SafetySwitch::turn_on`] gives
/// on for it and `met`; otherwise stores nothing and says why. Already on: nothing stored.
///
/// # Errors
///
/// [`CallsError::Database`] or [`CallsError::Ops`]; nothing is kept.
pub async fn turn_safety_switch_on(
    conn: &mut PgConnection,
    owner: Uuid,
    (approval, met): (&Approval, &Conditions),
    now: Timestamp,
) -> Result<SwitchChange, CallsError> {
    if !(1..=200).contains(&approval.reference.trim().chars().count()) {
        return Ok(SwitchChange::NoApproval);
    }
    if SafetySwitch::turn_on(Some(approval), met).is_none() {
        return Ok(SwitchChange::Unmet);
    }
    store(conn, owner, Some(approval), now).await
}

/// CALL-019, D-384: turns the switch off for `owner` at `now`. Already off: nothing stored.
///
/// # Errors
///
/// [`CallsError::Database`] or [`CallsError::Ops`]; nothing is kept.
pub async fn turn_safety_switch_off(
    conn: &mut PgConnection,
    owner: Uuid,
    now: Timestamp,
) -> Result<SwitchChange, CallsError> {
    store(conn, owner, None, now).await
}

/// One new row (on with `approval`, off without) and its audit row in one transaction, after the
/// table's lock, so two changes never share a time; nothing when the state is already in force.
async fn store(
    conn: &mut PgConnection,
    owner: Uuid,
    approval: Option<&Approval>,
    now: Timestamp,
) -> Result<SwitchChange, CallsError> {
    let mut tx = conn.begin().await.map_err(|_| CallsError::Database)?;
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(
             hashtextextended('calls.safety_switch', 407))) AS lock"#
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    let was_on = latest(&mut tx).await?.is_some();
    if was_on == approval.is_some() {
        return Ok(SwitchChange::Unchanged);
    }
    let date = approval.map(|a| a.date.to_string());
    let reference = approval.map(|a| a.reference.trim());
    let id = sqlx::query_scalar!(
        "INSERT INTO calls.safety_switch (is_on, approved_on, reference, changed_by, changed_at) \
         SELECT $1, $2::text::date, $3, $4, \
                GREATEST($5, (SELECT max(changed_at) + interval '1 microsecond' \
                              FROM calls.safety_switch)) \
         RETURNING id",
        approval.is_some(),
        date,
        reference,
        owner,
        now.to_sqlx() as _,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    let (from, to) = if was_on { ("on", "off") } else { ("off", "on") };
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: SAFETY_SWITCH_ACTION,
        subject_kind: "calls.safety_switch",
        subject_id: id,
        change: Change::Status { from, to },
    };
    audit(&mut tx, &entry).await.map_err(|_| CallsError::Ops)?;
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(SwitchChange::Changed)
}
