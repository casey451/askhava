//! The released job's page in the business portal (T085; PICK-024, BIZ-013, BIZ-035, PRIV-020;
//! TD-PICK-39, D-286, D-491, D-1739).
//!
//! [`job_release_page`] asks `picks::api::released_view` on every load, with the
//! `BusinessSessionProof` the signed-in principal makes, and keeps nothing: a refusal is `None`
//! (the route answers a bare 404), the purge's answer shows the removed line, and a shown release
//! prints the homeowner's first name once in the details line, which ends with the PICK-024 tip.
//! It shows how she chose to be contacted (BIZ-013): an email release says "Homeowner chose
//! email. Email first; calls may not be heard." and gives the email. Her phone is never on the
//! page (PICK-023). The call log (BIZ-035) is the composition's [`CallAttempt`] list: time,
//! inside the 8 a.m. to 8 p.m. window or not, answered or not, and the missed-call text's state,
//! with no number. The route [`JOB_RELEASE_ROUTE`] is served with the business portal (D-1728).

use askama::Template;
use askhava_picks::api::{PicksError, ViewAnswer, released_view};
use askhava_privacy::api::{SessionMint, Vault};
use jiff::Timestamp;
use secrecy::ExposeSecret;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::business_auth::BusinessPrincipal;

/// The released job's path.
pub const JOB_RELEASE_ROUTE: &str = "/b/picks/{pick}";
/// PICK-024's tip, which ends the details line.
pub const TIP_LINE: &str = "Tip: on your first call, ask which number to text.";
/// TD-PICK-39, D-295: the contact line of an email-contact release.
pub const EMAIL_CHOSEN_LINE: &str = "Homeowner chose email. Email first; calls may not be heard.";

/// What happened to the missed-call text after an unanswered call (FEE-022, D-286).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissedText {
    /// The call was answered, so none was due.
    NotDue,
    /// Ask Hava sent it (or its email copy).
    Sent,
    /// Ask Hava tried and delivery failed; this never counts against the business.
    Attempted,
}

/// One call attempt on the released job, as the composition read it (BIZ-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallAttempt {
    /// When the call was placed.
    pub at: Timestamp,
    /// Inside the homeowner's 8 a.m. to 8 p.m. window (PICK-018).
    pub in_window: bool,
    /// A person answered.
    pub answered: bool,
    /// The missed-call text's state.
    pub missed_text: MissedText,
}

/// The job and its call log, handed in by the composition.
#[derive(Debug, Clone, Copy)]
pub struct JobFacts<'a> {
    /// The job number shown to the business.
    pub job_number: i64,
    /// The released pick.
    pub pick: Uuid,
    /// The call log, oldest first.
    pub calls: &'a [CallAttempt],
}

struct Row {
    at: String,
    in_window: &'static str,
    answered: &'static str,
    text: &'static str,
}

/// Stands in for a release whose first name is missing, so the details line still reads.
pub const NO_NAME: &str = "The homeowner";

/// The released details as the page shows them; never a phone. A missing address is `None`, and
/// the details line then leaves out its address sentence.
pub(crate) struct Shown<'a> {
    pub(crate) first_name: &'a str,
    pub(crate) address: Option<&'a str>,
    pub(crate) email: Option<&'a str>,
}

/// The released view's answer as plain text, so the page's mapping is tested without a session
/// proof (D-1739): the view's fields are exposed here and dropped once the page is rendered.
pub(crate) enum Answer {
    /// Every refusal: a bare 404.
    NotFound,
    /// The purge's answer (PRIV-007).
    Removed,
    /// The details: first name, address, and the email only on an email-contact release.
    Shown([Option<String>; 3]),
}

impl From<ViewAnswer> for Answer {
    fn from(answer: ViewAnswer) -> Self {
        let text = |s: Option<&secrecy::SecretString>| s.map(|s| s.expose_secret().to_owned());
        match answer {
            ViewAnswer::NotFound => Self::NotFound,
            ViewAnswer::Removed => Self::Removed,
            ViewAnswer::Shown(v) => {
                Self::Shown([text(v.first_name()), text(v.address()), text(v.email())])
            }
        }
    }
}

/// The text, or `None` when it is missing or blank.
fn present(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|t| !t.trim().is_empty())
}

/// PICK-024, BIZ-013, D-1739: the page for `answer`; `None` for a refusal. A missing first name
/// reads [`NO_NAME`]; a missing or blank address or email is left out.
pub(crate) fn page_for(
    facts: JobFacts<'_>,
    answer: &Answer,
) -> Result<Option<String>, askama::Error> {
    let shown = match answer {
        Answer::NotFound => return Ok(None),
        Answer::Removed => None,
        Answer::Shown([name, address, email]) => Some(Shown {
            first_name: present(name).unwrap_or(NO_NAME),
            address: present(address),
            email: present(email),
        }),
    };
    render(facts, shown).map(Some)
}

#[derive(Template)]
#[template(path = "job_release.html")]
struct ReleaseView<'a> {
    job_number: i64,
    shown: Option<Shown<'a>>,
    rows: Vec<Row>,
    tip: &'static str,
    email_line: &'static str,
}

fn row(c: &CallAttempt) -> Row {
    let yes_no = |b: bool| if b { "Yes" } else { "No" };
    let at = c.at.in_tz("America/Phoenix").map_or_else(
        |_| c.at.to_string(),
        |z| z.strftime("%b %-d, %-I:%M %p").to_string(),
    );
    Row {
        at,
        in_window: yes_no(c.in_window),
        answered: yes_no(c.answered),
        text: match c.missed_text {
            MissedText::NotDue => "Not needed",
            MissedText::Sent => "Sent",
            MissedText::Attempted => "Tried, but it didn't go through",
        },
    }
}

/// Renders the page; `shown` is `None` after the purge (PRIV-007).
pub(crate) fn render(
    facts: JobFacts<'_>,
    shown: Option<Shown<'_>>,
) -> Result<String, askama::Error> {
    let view = ReleaseView {
        job_number: facts.job_number,
        shown,
        rows: facts.calls.iter().map(row).collect(),
        tip: TIP_LINE,
        email_line: EMAIL_CHOSEN_LINE,
    };
    view.render()
}

/// Why the page could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JobReleaseError {
    /// The released view's read failed (`picks` has already logged it).
    #[error("released view failed")]
    Database,
    /// The template failed.
    #[error("job release page failed")]
    Render,
}

/// PICK-022 to PICK-024, BIZ-013, BIZ-035, D-1739: the page for `facts.pick`, read now through
/// `released_view` with `principal`'s session proof; `None` when the view refuses (a bare 404).
///
/// # Errors
/// [`JobReleaseError`].
pub async fn job_release_page(
    (pool, vault, mint): (&PgPool, &Vault, &SessionMint),
    principal: &BusinessPrincipal,
    facts: JobFacts<'_>,
) -> Result<Option<String>, JobReleaseError> {
    let proof = principal.session_proof(mint);
    let answer = released_view(pool, vault, &proof, facts.pick).await;
    let answer = Answer::from(answer.map_err(|_: PicksError| JobReleaseError::Database)?);
    page_for(facts, &answer).map_err(|_| JobReleaseError::Render)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(calls: &[CallAttempt]) -> JobFacts<'_> {
        JobFacts {
            job_number: 1042,
            pick: Uuid::from_u128(7),
            calls,
        }
    }

    /// TD-PICK-39 (PICK-024, PRIV-020): the first name once, the details line ends with the tip,
    /// no phone; an email release holds the email line (BIZ-013, TD-BIZ-04 page case).
    #[test]
    fn td_pick_39_first_name_once_with_the_tip_and_the_email_line() {
        let call = Shown {
            first_name: "Testa",
            address: Some("100 Fake St, Lake Havasu City"),
            email: None,
        };
        let mail = Shown {
            email: Some("testa@example.test"),
            ..call
        };
        let page = render(facts(&[]), Some(call)).expect("page");
        assert_eq!(page.matches("Testa").count(), 1);
        assert!(page.contains(&format!("Lake Havasu City. {TIP_LINE}</p>")));
        assert!(page.contains("Testa picked you. The job is at 100 Fake St"));
        assert!(!page.contains(EMAIL_CHOSEN_LINE) && page.contains("Homeowner chose calls."));
        assert!(!page.contains("928") && !page.contains("tel:"));
        let page = render(facts(&[]), Some(mail)).expect("page");
        assert!(page.contains(EMAIL_CHOSEN_LINE) && page.contains("testa@example.test"));
        let gone = render(facts(&[]), None).expect("page");
        assert!(gone.contains("We removed") && !gone.contains(TIP_LINE));
    }

    /// TD-BIZ-12 call-log case (BIZ-035): each attempt's time, window, answer and text state, no
    /// homeowner number.
    #[test]
    fn td_biz_12_the_call_log_shows_each_attempt() {
        let at = Timestamp::from_second(1_790_000_000).expect("time");
        let calls = [
            CallAttempt {
                at,
                in_window: true,
                answered: false,
                missed_text: MissedText::Sent,
            },
            CallAttempt {
                at,
                in_window: false,
                answered: true,
                missed_text: MissedText::NotDue,
            },
            CallAttempt {
                at,
                in_window: true,
                answered: false,
                missed_text: MissedText::Attempted,
            },
        ];
        let page = render(facts(&calls), None).expect("page");
        assert_eq!(page.matches("<tr><td>").count(), 3);
        for want in [
            "<td>Sent</td>",
            "<td>Not needed</td>",
            "<td>Tried, but it didn&#39;t go through</td>",
        ] {
            assert!(page.contains(want), "{want}");
        }
        assert!(page.contains("<td>Sep 21, 7:13 AM</td>"), "{page}");
    }

    fn shown(name: Option<&str>, address: Option<&str>) -> Answer {
        Answer::Shown([name.map(str::to_owned), address.map(str::to_owned), None])
    }

    /// D-1739: a refusal is no page (the route's bare 404); the purge's answer is the removed
    /// line with no tip; a shown release prints its details.
    #[test]
    fn d_1739_the_view_answer_maps_to_the_page() {
        assert_eq!(page_for(facts(&[]), &Answer::NotFound).expect("ok"), None);
        let gone = page_for(facts(&[]), &Answer::Removed)
            .expect("ok")
            .expect("page");
        assert!(gone.contains("We removed") && !gone.contains(TIP_LINE));
        let page = page_for(facts(&[]), &shown(Some("Testa"), Some("100 Fake St")));
        let page = page.expect("ok").expect("page");
        assert!(page.contains(&format!(
            "Testa picked you. The job is at 100 Fake St. {TIP_LINE}"
        )));
        assert!(!page.contains(NO_NAME) && !page.contains("We removed"));
    }

    /// PICK-024: a missing first name reads "The homeowner"; a missing or blank address leaves
    /// the address sentence out (never "The job is at . "), and the tip still ends the line.
    #[test]
    fn pick_024_a_missing_name_or_address_still_reads() {
        for address in [None, Some(""), Some("  ")] {
            let page = page_for(facts(&[]), &shown(None, address));
            let page = page.expect("ok").expect("page");
            assert!(
                page.contains(&format!("{NO_NAME} picked you. {TIP_LINE}</p>")),
                "{page}"
            );
            assert!(!page.contains("The job is at"));
        }
        let blank_mail = Answer::Shown([None, None, Some(" ".to_owned())]);
        let page = page_for(facts(&[]), &blank_mail)
            .expect("ok")
            .expect("page");
        assert!(page.contains("Homeowner chose calls.") && !page.contains("mailto:"));
    }
}
