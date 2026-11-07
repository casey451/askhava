//! The business portal home and its step-up gate (T085; BIZ-025, CARD-022, SEC-038; D-480,
//! D-491, D-492).
//!
//! [`business_home_page`] shows the "Your numbers" panel from the [`YourNumbers`] the composition
//! reads (BIZ-025, D-193, D-250, D-284, D-331) and the signed-in person's own open-cards email
//! times with their form (CARD-022); [`save_open_cards_times`] stores that person's choice through
//! `businesses::api`, for themselves only. [`require_step_up`] is the gate every SEC-038 change
//! calls first: a live 30-day session is not enough, the person must have passed a Verify code in
//! the last 5 minutes (`businesses::api::has_fresh_step_up`); a refusal is one `authz_denied`
//! security event (SEC-048). The routes ([`BUSINESS_HOME_ROUTE`], [`OPEN_CARDS_ROUTE`]) are served
//! with the business portal (D-1728).

use askama::Template;
use askhava_businesses::api::{
    Acting, OpenCardsTime, OpenCardsTimes, has_fresh_step_up, set_open_cards_times,
};
use jiff::Timestamp;
use sqlx::PgConnection;

use crate::business_auth::BusinessPrincipal;
use crate::confirm_pay::money;
use crate::security::{SecurityEvent, SecurityEventKind, record_security_event};

/// The portal home's path.
pub const BUSINESS_HOME_ROUTE: &str = "/b";
/// Where the open-cards times form posts.
pub const OPEN_CARDS_ROUTE: &str = "/b/open-cards";

/// A change SEC-038 guards with a fresh Verify code (D-480).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveChange {
    /// A notice address.
    NoticeAddress,
    /// The backup address.
    BackupAddress,
    /// Linking a phone.
    LinkPhone,
    /// Approving a phone a member linked.
    ApprovePhone,
    /// A team member's role.
    TeamRole,
    /// The saved card.
    SavedCard,
    /// The monthly limit.
    MonthlyLimit,
    /// Opting in to auto-confirm.
    AutoConfirm,
}

impl SensitiveChange {
    /// Every guarded change.
    pub const ALL: [Self; 8] = [
        Self::NoticeAddress,
        Self::BackupAddress,
        Self::LinkPhone,
        Self::ApprovePhone,
        Self::TeamRole,
        Self::SavedCard,
        Self::MonthlyLimit,
        Self::AutoConfirm,
    ];
}

/// Why a guarded change did not go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StepUpError {
    /// No Verify code in the last 5 minutes; the person is asked for a new one (SEC-038).
    #[error("this change needs a fresh code")]
    NeedsCode,
    /// A database call failed.
    #[error("step-up database call failed")]
    Database,
}

/// SEC-038, SEC-048, D-480: `Ok` only when `principal` passed a Verify code no more than 5
/// minutes before `now`; else one `authz_denied` event for `route` with the member as actor, and
/// one log line naming `change` (no value).
///
/// The refusal's event is written on `db`, so call this on a connection whose work is kept, never
/// inside a transaction the caller rolls back when this refuses: the rollback would drop the
/// `authz_denied` event SEC-048 requires. Check first, then open the change's transaction.
///
/// # Errors
/// [`StepUpError`].
pub async fn require_step_up(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (change, route): (SensitiveChange, &str),
    now: Timestamp,
) -> Result<(), StepUpError> {
    let by = Acting {
        business: principal.business,
        member: principal.member,
    };
    let fresh = has_fresh_step_up(db, by, now).await;
    if fresh.map_err(|_| StepUpError::Database)? {
        return Ok(());
    }
    tracing::warn!(change = ?change, "sensitive change needs a fresh code");
    let event = SecurityEvent {
        kind: SecurityEventKind::AuthzDenied,
        route,
        request_id: None,
        actor_ref: Some(principal.member),
    };
    record_security_event(db, &event, now)
        .await
        .map_err(|_| StepUpError::Database)?;
    Err(StepUpError::NeedsCode)
}

/// This month's "Your numbers", counted by the composition from the ledger and the jobs (BIZ-025).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YourNumbers {
    /// Job cards sent.
    pub cards: u32,
    /// Cards that ended without a quote (D-331).
    pub cards_without_quote: u32,
    /// Quotes sent.
    pub quotes: u32,
    /// Picks.
    pub picks: u32,
    /// Picks confirmed.
    pub confirms: u32,
    /// Introduction fees charged, in cents.
    pub fees_cents: u64,
    /// Picks that ran out of time (D-284).
    pub lapsed_picks: u32,
    /// Picked jobs by the homeowner's DONE, NO and OTHER replies (D-250).
    pub finished: (u32, u32, u32),
    /// The typical (median) quotes per job in each trade, in tenths: (trade, tenths).
    pub typical_quotes: Vec<(String, u32)>,
}

/// One labelled number of the panel.
struct Line {
    label: &'static str,
    value: String,
}

/// BIZ-025: the panel's lines in order; fee per win is fees over confirms, none before one.
fn number_lines(n: &YourNumbers) -> Vec<Line> {
    let per_win = (n.confirms > 0).then(|| money(n.fees_cents / u64::from(n.confirms)));
    let (done, no, other) = n.finished;
    let rows: [(&'static str, String); 11] = [
        ("Job requests sent to you", n.cards.to_string()),
        (
            "Requests that closed before you quoted",
            n.cards_without_quote.to_string(),
        ),
        ("Quotes you sent", n.quotes.to_string()),
        ("Times you were picked", n.picks.to_string()),
        ("Picks you confirmed", n.confirms.to_string()),
        ("Introduction fees charged", money(n.fees_cents)),
        (
            "Average introduction fee per confirmed pick",
            per_win.unwrap_or_else(|| "None yet".to_owned()),
        ),
        ("Picks not confirmed in time", n.lapsed_picks.to_string()),
        ("Homeowner said it's done", done.to_string()),
        ("Homeowner said it isn't done", no.to_string()),
        ("Homeowner gave another answer", other.to_string()),
    ];
    rows.into_iter()
        .map(|(label, value)| Line { label, value })
        .collect()
}

/// One open-cards time box.
struct TimeBox {
    code: &'static str,
    label: &'static str,
    checked: bool,
}

#[derive(Template)]
#[template(path = "business_home.html")]
struct HomeView<'a> {
    lines: Vec<Line>,
    typical: Vec<(&'a str, String)>,
    boxes: Vec<TimeBox>,
    too_many: bool,
    csrf: &'a str,
}

/// BIZ-025, CARD-022: the portal home with `numbers` and the person's `times`; `too_many` shows
/// the form's error after more than two times were sent (A11Y-030).
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn business_home_page(
    numbers: &YourNumbers,
    (times, too_many): (OpenCardsTimes, bool),
    csrf: &str,
) -> Result<String, askama::Error> {
    let chosen = times.times();
    let label = |t: OpenCardsTime| match t {
        OpenCardsTime::EightAm => "8 a.m.",
        OpenCardsTime::Noon => "Noon",
        OpenCardsTime::FivePm => "5 p.m.",
    };
    let view = HomeView {
        lines: number_lines(numbers),
        typical: (numbers.typical_quotes.iter())
            .map(|(trade, t)| (trade.as_str(), format!("{}.{}", t / 10, t % 10)))
            .collect(),
        boxes: (OpenCardsTime::ALL.into_iter())
            .map(|t| TimeBox {
                code: t.code(),
                label: label(t),
                checked: chosen.contains(&t),
            })
            .collect(),
        too_many,
        csrf,
    };
    view.render()
}

/// CARD-022, D-492: stores the form's `codes` as `principal`'s own open-cards times. `Ok(None)`
/// for more than two times or an unknown value (nothing stored); the page shows the error.
///
/// # Errors
/// `askhava_businesses::api::BusinessesError` when the person is no longer on the team or the
/// database fails.
pub async fn save_open_cards_times(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    codes: &[String],
    now: Timestamp,
) -> Result<Option<OpenCardsTimes>, askhava_businesses::api::BusinessesError> {
    let parsed: Option<Vec<_>> = codes.iter().map(|c| OpenCardsTime::parse(c)).collect();
    let Some(times) = parsed.and_then(|p| OpenCardsTimes::new(&p)) else {
        return Ok(None);
    };
    let by = Acting {
        business: principal.business,
        member: principal.member,
    };
    set_open_cards_times(db, by, times, now).await?;
    Ok(Some(times))
}

#[cfg(test)]
mod tests {
    use askhava_businesses::api::{BusinessesError, add_owner, create, open_cards_times};
    use sqlx::types::Uuid;

    use super::*;

    type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

    /// TD-PICK-17 and TD-BIZ-11 page cases (BIZ-025): every panel number shows as given, fee per
    /// win is fees over confirms, and the typical quotes per trade show.
    #[test]
    fn td_pick_17_your_numbers_panel_shows_each_number() {
        let n = YourNumbers {
            cards: 12,
            cards_without_quote: 7,
            quotes: 5,
            picks: 3,
            confirms: 2,
            fees_cents: 10_000,
            lapsed_picks: 1,
            finished: (4, 2, 9),
            typical_quotes: vec![("Plumbing".to_owned(), 25)],
        };
        let page = business_home_page(&n, (OpenCardsTimes::DEFAULT, false), "t").expect("page");
        for want in [
            "<dd>12</dd>",
            "<dd>7</dd>",
            "$100.00",
            "<dd>$50.00</dd>",
            "<dd>9</dd>",
        ] {
            assert!(page.contains(want), "{want}");
        }
        assert!(page.contains("Plumbing: 2.5"));
        assert!(page.contains(r#"value="5pm" checked"#), "5 p.m. by default");
        assert!(!page.contains("role=\"alert\""));
        let none = business_home_page(
            &YourNumbers::default(),
            (OpenCardsTimes::DEFAULT, true),
            "t",
        );
        let none = none.expect("page");
        assert!(none.contains("None yet") && none.contains("<title>Error: "));
        assert!(none.contains(r#"aria-describedby="times-hint times-error""#));
        assert!(!page.contains("times-error"));
    }

    /// CARD-022, D-492: the person's own times are stored; more than two times or an unknown
    /// value stores nothing; a person not on the business's team is refused.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn card_022_save_open_cards_times_for_the_person_only(mut db: Db) {
        let at = Timestamp::from_second(1_790_000_000).expect("time");
        let business = create(&mut db, false, at).await.expect("business");
        let member = add_owner(&mut db, business, Uuid::from_u128(0xd1), at).await;
        let me = BusinessPrincipal {
            member: member.expect("owner"),
            business,
            owner: true,
            roles: (true, true),
        };
        let codes = |c: &[&str]| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        for bad in [codes(&["8am", "noon", "5pm"]), codes(&["9am"])] {
            let none = save_open_cards_times(&mut db, &me, &bad, at).await;
            assert_eq!(none.expect("ok"), None, "{bad:?}");
        }
        let unchanged = open_cards_times(&mut db, me.member).await.expect("read");
        assert_eq!(unchanged, OpenCardsTimes::DEFAULT, "nothing stored");
        let saved = save_open_cards_times(&mut db, &me, &codes(&["noon"]), at).await;
        let noon = OpenCardsTimes::new(&[OpenCardsTime::Noon]).expect("one");
        assert_eq!(saved.expect("ok"), Some(noon));
        assert_eq!(open_cards_times(&mut db, me.member).await.expect("r"), noon);
        let stranger = BusinessPrincipal {
            member: Uuid::from_u128(0xd2),
            ..me
        };
        let refused = save_open_cards_times(&mut db, &stranger, &codes(&["8am"]), at).await;
        assert_eq!(refused, Err(BusinessesError::UnknownBusiness));
    }
}
