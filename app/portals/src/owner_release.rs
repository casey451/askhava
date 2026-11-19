//! The owner's release button: re-sends a release that already went out, never makes one
//! (PICK-022, PICK-023, SEC-006, SEC-042, SEC-043; TD-PICK-41; T540, D-074, D-3890, D-3892).
//!
//! [`owner_release_page`] renders `templates/owner_release.html`: one form where the owner types
//! a job number and presses "Re-send the release notice", carrying the CSRF token and a one-time
//! press ID minted at each render (as `confirm_pay` does), so a resubmitted form queues nothing
//! more.
//! [`owner_release_resend`] passes `owner_power` with [`OwnerPower::Release`] (SEC-043 asks no
//! fresh factor for it), resolves the number through `jobs::api::job_by_number` and calls
//! `picks::api::resend_release`, which acts only on a standing `picks.release` row. It never calls
//! `picks::api::release`, so it can never get past safe mode, a CANCEL pause or the Confirm
//! gates: an unknown number, a job with no release and a malformed press ID are refused on the
//! page and queue nothing. The page never shows a homeowner detail (PICK-023). `server`'s
//! `owner_portal` serves [`OWNER_RELEASE_ROUTE`] behind the owner session.

use askama::Template;
use askhava_jobs::api::job_by_number;
use askhava_picks::api::{ResendPress, Resent, resend_release};
use axum_extra::extract::cookie::CookieJar;
use jiff::Timestamp;
use serde::Deserialize;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::owner_auth::{OwnerAuthError, OwnerPower, owner_power};
use crate::owner_portal::OwnerToolError;

/// The release page (`GET`) and its "Re-send the release notice" press (`POST`, a
/// [`ReleaseForm`]); a fixed path, the job number in the body (D-3890).
pub const OWNER_RELEASE_ROUTE: &str = "/owner/release";

/// The body of the press: the job number as typed and the press ID the page rendered. The CSRF
/// field is read by `server`'s `CsrfForm`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReleaseForm {
    /// The job number as typed.
    pub job: String,
    /// The press ID the page rendered into the form.
    pub press: String,
}

/// What the release page shows (PICK-022, D-074).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseScreen {
    /// The empty form.
    Ask,
    /// The release notice for this job number was queued again.
    Sent(i64),
    /// This press was already sent; nothing more went out.
    Repeat,
    /// No job has this number, or its job has no release; nothing was sent.
    NotReleased(i64),
    /// What was typed is not a job number; nothing was sent.
    NotANumber,
    /// The job's business has no pick-and-release address; nothing was sent.
    NoRecipient(i64),
    /// The form's press ID was malformed; nothing was sent.
    StaleForm,
}

impl ReleaseScreen {
    /// Whether this screen is a refusal (answered 400 with its error summary).
    #[must_use]
    pub fn refused(self) -> bool {
        matches!(
            self,
            Self::NotReleased(_) | Self::NotANumber | Self::NoRecipient(_) | Self::StaleForm
        )
    }

    /// The line shown above the form, if any.
    fn line(self) -> Option<String> {
        Some(match self {
            Self::Ask => return None,
            Self::Sent(job) => {
                format!("Sent again. We sent the release notice for job {job} to the business.")
            }
            Self::Repeat => "You already pressed this, so we didn't send it twice.".to_owned(),
            Self::NotReleased(job) => format!(
                "Job {job} hasn't been released, so there's nothing to send again. Check the job number."
            ),
            Self::NotANumber => "Type the job number in digits, like 1042.".to_owned(),
            Self::NoRecipient(job) => format!(
                "The business on job {job} has no email set up for picks. Ask them to add one in their notice settings."
            ),
            Self::StaleForm => "This page needed a refresh. Press the button again.".to_owned(),
        })
    }
}

#[derive(Template)]
#[template(path = "owner_release.html")]
struct ReleaseView<'a> {
    csrf: &'a str,
    route: &'a str,
    press: String,
    typed: &'a str,
    line: Option<String>,
    failed: bool,
}

/// The release page for `screen`, its form carrying `csrf`, a fresh press ID and `typed` (the
/// number as typed, kept after a refusal) (SEC-042, PICK-023: no homeowner detail).
///
/// # Errors
///
/// The template's render error.
pub fn owner_release_page(
    csrf: &str,
    screen: ReleaseScreen,
    typed: &str,
) -> Result<String, askama::Error> {
    ReleaseView {
        csrf,
        route: OWNER_RELEASE_ROUTE,
        press: Uuid::new_v4().to_string(),
        typed: if screen.refused() { typed.trim() } else { "" },
        line: screen.line(),
        failed: screen.refused(),
    }
    .render()
}

/// PICK-022, PICK-023, SEC-006, D-074, D-3892: one press of "Re-send the release notice" for the
/// owner in `jar` at `now`: re-sends the typed job's standing release through
/// `picks::api::resend_release` and never makes one; see the module docs.
///
/// # Errors
///
/// [`OwnerToolError::Refused`] (no live owner), [`OwnerToolError::NeedsFreshFactor`] or
/// [`OwnerToolError::Database`].
pub async fn owner_release_resend(
    conn: &mut PgConnection,
    jar: &CookieJar,
    form: &ReleaseForm,
    now: Timestamp,
) -> Result<ReleaseScreen, OwnerToolError> {
    let power = (OwnerPower::Release, OWNER_RELEASE_ROUTE);
    let principal = match owner_power(conn, jar, power, now).await {
        Ok(principal) => principal,
        Err(OwnerAuthError::NotOwner) => return Err(OwnerToolError::Refused),
        Err(OwnerAuthError::NeedsFreshFactor) => return Err(OwnerToolError::NeedsFreshFactor),
        Err(_) => return Err(OwnerToolError::Database),
    };
    let Ok(press) = Uuid::parse_str(form.press.trim()) else {
        return Ok(ReleaseScreen::StaleForm);
    };
    let Some(number) = job_number_typed(&form.job) else {
        return Ok(ReleaseScreen::NotANumber);
    };
    let job = job_by_number(conn, number)
        .await
        .map_err(|_| OwnerToolError::Database)?;
    let Some(job) = job else {
        return Ok(ReleaseScreen::NotReleased(number));
    };
    let press = ResendPress {
        press,
        owner: principal.owner(),
    };
    let resent = resend_release(conn, job, press, now)
        .await
        .map_err(|_| OwnerToolError::Database)?;
    Ok(match resent {
        Resent::Queued => ReleaseScreen::Sent(number),
        Resent::Repeat => ReleaseScreen::Repeat,
        Resent::NotReleased => ReleaseScreen::NotReleased(number),
        Resent::NoRecipient => ReleaseScreen::NoRecipient(number),
    })
}

/// The job number typed: digits only, above 0, with any spaces or a leading `#` dropped.
fn job_number_typed(typed: &str) -> Option<i64> {
    let digits = typed.trim().trim_start_matches('#').trim();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    //! The page's screens and the number reading (PICK-023, D-3892). Fake numbers only.

    use super::{ReleaseScreen, job_number_typed, owner_release_page};

    #[test]
    fn a_typed_number_reads_digits_only() {
        assert_eq!(job_number_typed(" 1042 "), Some(1042));
        assert_eq!(job_number_typed("#1042"), Some(1042));
        for bad in ["", "abc", "10 42", "-5", "0", "1e3", "99999999999999999999"] {
            assert_eq!(job_number_typed(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_refusal_keeps_the_number_and_links_its_field() {
        let html =
            owner_release_page("tok", ReleaseScreen::NotReleased(1042), "1042").expect("page");
        assert!(html.contains("<title>Error: "));
        assert!(html.contains("href=\"#job\""));
        assert!(html.contains("value=\"1042\""));
        assert!(html.contains("aria-invalid=\"true\""));
        let sent = owner_release_page("tok", ReleaseScreen::Sent(1042), "1042").expect("page");
        assert!(!sent.contains("Error: "));
        assert!(sent.contains("Sent again."));
        assert!(!sent.contains("value=\"1042\""));
    }

    #[test]
    fn each_render_mints_a_new_press() {
        let press = |html: &str| {
            let at = html.find("name=\"press\" value=\"").expect("press") + 20;
            html[at..at + 36].to_owned()
        };
        let one = owner_release_page("tok", ReleaseScreen::Ask, "").expect("page");
        let two = owner_release_page("tok", ReleaseScreen::Ask, "").expect("page");
        assert_ne!(press(&one), press(&two));
        assert!(!one.contains("role=\"alert\""));
    }
}
