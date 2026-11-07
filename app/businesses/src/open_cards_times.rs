//! Each person's open-cards email times (CARD-022, BIZ-037; T085, D-491, D-492).
//!
//! A person who gets job cards chooses up to two of 8 a.m., noon and 5 p.m. Arizona time for the
//! open-cards email, or none ("off"); with no choice stored they get it at 5 p.m. The choice is
//! per person, one `businesses.open_cards_time` row per team member (migration 0133), set only by
//! that person for themselves on the portal home ([`set_open_cards_times`]) and read by T198's
//! composer through [`open_cards_times`]. No contact value is stored (PRIV-001).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::BusinessesError;
use crate::team::Acting;

/// One of the three open-cards email times, Arizona time (CARD-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCardsTime {
    /// 8 a.m.
    EightAm,
    /// Noon.
    Noon,
    /// 5 p.m., the default.
    FivePm,
}

impl OpenCardsTime {
    /// The three times, earliest first.
    pub const ALL: [Self; 3] = [Self::EightAm, Self::Noon, Self::FivePm];

    /// Minutes after midnight, Arizona time.
    #[must_use]
    pub const fn minute(self) -> u16 {
        match self {
            Self::EightAm => 480,
            Self::Noon => 720,
            Self::FivePm => 1020,
        }
    }

    /// The form value, also read back by [`OpenCardsTime::parse`].
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EightAm => "8am",
            Self::Noon => "noon",
            Self::FivePm => "5pm",
        }
    }

    /// The time a form value names, or `None`.
    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.code() == code)
    }
}

/// A person's chosen times: at most two, none meaning off (CARD-022). Built only by
/// [`OpenCardsTimes::new`], so it never holds three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCardsTimes([bool; 3]);

impl OpenCardsTimes {
    /// No choice stored: 5 p.m. only (CARD-022).
    pub const DEFAULT: Self = Self([false, false, true]);

    /// CARD-022: the set of `times` (a repeat counts once), or `None` for all three.
    #[must_use]
    pub fn new(times: &[OpenCardsTime]) -> Option<Self> {
        let set = OpenCardsTime::ALL.map(|t| times.contains(&t));
        (set.iter().filter(|on| **on).count() <= 2).then_some(Self(set))
    }

    /// The chosen times, earliest first; empty when switched off.
    #[must_use]
    pub fn times(self) -> Vec<OpenCardsTime> {
        OpenCardsTime::ALL
            .into_iter()
            .zip(self.0)
            .filter_map(|(t, on)| on.then_some(t))
            .collect()
    }
}

/// CARD-022: `member`'s open-cards times, [`OpenCardsTimes::DEFAULT`] when none is stored.
///
/// This reads the times only; it does not decide who gets the email. It answers for any ID,
/// including a removed member or one with no seat at all ([`OpenCardsTimes::DEFAULT`]), and a
/// removed member's row stays. The composer (T198) must first pick the recipients itself, active
/// team members only (`removed_at IS NULL`) who get job cards (BIZ-037), and only then read their
/// times here.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn open_cards_times(
    conn: &mut PgConnection,
    member: Uuid,
) -> Result<OpenCardsTimes, BusinessesError> {
    let q = sqlx::query!(
        "SELECT at_8am, at_noon, at_5pm FROM businesses.open_cards_time WHERE member_id = $1",
        member
    );
    let row = q
        .fetch_optional(conn)
        .await
        .map_err(|_| BusinessesError::Database)?;
    Ok(row.map_or(OpenCardsTimes::DEFAULT, |r| {
        OpenCardsTimes([r.at_8am, r.at_noon, r.at_5pm])
    }))
}

/// CARD-022, D-492: stores `times` for the acting person themselves, only while they are a
/// current member of the business they act for. A stored row says only when; whether the person
/// still gets the email is the composer's active-team and gets-job-cards check (BIZ-037), see
/// [`open_cards_times`].
///
/// # Errors
/// [`BusinessesError::UnknownBusiness`] when `by` is not a current member of `by.business`;
/// [`BusinessesError::Database`].
pub async fn set_open_cards_times(
    conn: &mut PgConnection,
    by: Acting,
    times: OpenCardsTimes,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let [a, b, c] = times.0;
    let q = sqlx::query!(
        "INSERT INTO businesses.open_cards_time (member_id, at_8am, at_noon, at_5pm, changed_at) \
         SELECT id, $3, $4, $5, $6 FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL \
         ON CONFLICT (member_id) DO UPDATE SET at_8am = $3, at_noon = $4, at_5pm = $5, \
         changed_at = $6",
        by.member,
        by.business,
        a,
        b,
        c,
        now.to_sqlx() as _
    );
    let done = q
        .execute(conn)
        .await
        .map_err(|_| BusinessesError::Database)?;
    if done.rows_affected() == 1 {
        Ok(())
    } else {
        Err(BusinessesError::UnknownBusiness)
    }
}
