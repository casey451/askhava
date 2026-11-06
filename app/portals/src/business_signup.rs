//! The business sign-up screens (BIZ-024, BIZ-026, BIZ-027, BIZ-030, BIZ-031, LIC-047, WEB-024;
//! D-179, D-254, D-288, D-312, D-366, D-417, D-462, D-493).
//!
//! [`signup_page`] renders `templates/business_signup.html` for one [`Screen`]: the form (the
//! category list with the pest attestation, and the plain-words fee box above the Terms link), the
//! masked record contact and its kind before any code, the "That number is old" saved screen, the
//! letter step with its town and ZIP, and the done screen. Every screen shows the WEB-024 notice
//! line for the business-texting switch and BIZ-024's "Someone else filling this in?" line: anyone
//! may fill in the form, the code still goes only to the record contact. [`done_facts`] reads the
//! done screen's trade count through `businesses::api` and the open jobs through `quotes::api`
//! (`businesses` cannot call `quotes`, D-491). The fee box's numbers come from the fee settings
//! ([`fee_numbers`]). The routes that read the sign-up and post its steps are wired with the
//! business portal (D-1709).

use askama::Template;
use askhava_businesses::api::{
    FeeNumbers, MaskedContact, NUMBER_OLD_BUTTON, SOMEONE_ELSE_LINE, STOP_FIX_BUTTON, fee_summary,
    fix_record_lines, trade_count,
};
use askhava_payments::api::Tier;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::pick_flow::PageLanguage;

/// The sign-up's path on askhava.com; each step posts to a path under it.
pub const SIGNUP_ROUTE: &str = "/b/signup";

/// WEB-024, D-307, D-344: the notice line while business texting is off, English and Spanish.
pub const NOTICE_OFF: [&str; 2] = [
    "Job requests and picks come by email. We don't text or call businesses.",
    "Las solicitudes de trabajo y las selecciones llegan por correo electrónico. No enviamos \
mensajes de texto ni llamamos a los negocios.",
];
/// WEB-024, TXT-035: the notice line while business texting is on, English and Spanish.
pub const NOTICE_ON: [&str; 2] = [
    "Choose how you get job requests and picks: email, text or both. Ask Hava never calls you \
with a recording.",
    "Elija cómo recibir las solicitudes de trabajo y las selecciones: correo electrónico, texto o \
ambos. Ask Hava nunca le llama con una grabación.",
];

/// LIC-047, D-417: the pest-control attestation and its record categories.
pub const PEST_ATTESTATION: &str =
    "I hold an active Arizona Department of Agriculture pest-management business license";

/// WEB-024, TXT-035, D-493: the notice line for the business-texting switch, in `language`.
#[must_use]
pub const fn notice_line(texting_on: bool, language: PageLanguage) -> &'static str {
    let lines = if texting_on { NOTICE_ON } else { NOTICE_OFF };
    match language {
        PageLanguage::En => lines[0],
        PageLanguage::Es => lines[1],
    }
}

/// BIZ-030, D-173: the fee box's numbers from the fee settings: the three tiers
/// (`payments::api::Tier::cents`), the Confirm window and the emergency window.
#[must_use]
pub fn fee_numbers(confirm: ConfirmSettings, rules: WindowRules) -> FeeNumbers {
    FeeNumbers {
        tier_cents: [Tier::Small, Tier::Medium, Tier::Large].map(Tier::cents),
        confirm_hours: confirm.window_minutes / 60,
        emergency_minutes: rules.emergency_minutes,
    }
}

/// What the done screen counts (BIZ-031): the trade's name, its <n>, whether the business only
/// joined the waitlist, and the open jobs it can quote now (only once active).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneFacts {
    /// The trade's name as shown, for example "Plumbing".
    pub trade: String,
    /// The businesses in the trade, counted as `businesses::api::trade_count` does.
    pub count: u32,
    /// Joined only the waitlist.
    pub waitlist: bool,
    /// The carded jobs still waiting for quotes, oldest first.
    pub open_jobs: Vec<Uuid>,
}

/// Why the done screen's facts could not be read; no value is carried (R5).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("the sign-up done screen could not be read")]
pub struct DoneError;

/// BIZ-031, CARD-018, D-462: the done screen's facts for `business` in trade `(code, name)`; the
/// open jobs are read only when `active` (`quotes::api::open_card_jobs`).
///
/// # Errors
/// [`DoneError`] when a read fails.
pub async fn done_facts(
    db: &mut PgConnection,
    business: Uuid,
    (category, trade): (&str, &str),
    (waitlist, active): (bool, bool),
) -> Result<DoneFacts, DoneError> {
    let count = trade_count(db, business, category, waitlist).await;
    let open_jobs = if active {
        let open = askhava_quotes::api::open_card_jobs(db, business).await;
        open.map_err(|_| DoneError)?
    } else {
        Vec::new()
    };
    Ok(DoneFacts {
        trade: trade.to_owned(),
        count: count.map_err(|_| DoneError)?,
        waitlist,
        open_jobs,
    })
}

/// BIZ-031, D-187, D-261, D-312, D-462: the done screen's lines and job links. One business so
/// far reads "the first", since "one of 1" is not plain English (D-1710).
#[must_use]
pub fn done_lines(f: &DoneFacts) -> (Vec<String>, Vec<(String, String)>) {
    let (n, t) = (f.count, &f.trade);
    if f.waitlist {
        let who = if n == 1 {
            "the first business waiting".to_owned()
        } else {
            format!("one of {n} businesses waiting")
        };
        let line = format!(
            "You're on the waitlist for {t}, {who}. We'll email you once when it opens; your join date is kept."
        );
        return (vec![line], Vec::new());
    }
    let one_of = if n == 1 {
        format!("You're the first {t} business so far.")
    } else {
        format!("You're one of {n} businesses in {t} so far.")
    };
    let k = f.open_jobs.len();
    if k == 0 {
        let line =
            format!("{one_of} We'll tell you once, by email, when requests in your trade start.");
        return (vec![line], Vec::new());
    }
    let jobs = if k == 1 { "job" } else { "jobs" };
    let links = f.open_jobs.iter().enumerate();
    let links = links.map(|(i, j)| (format!("/b/jobs/{j}"), format!("Open {t} job {}", i + 1)));
    (
        vec![format!("{k} open {t} {jobs} you can quote now"), one_of],
        links.collect(),
    )
}

/// One sign-up screen (BIZ-024, BIZ-026, BIZ-030, BIZ-031).
#[derive(Debug, Clone, Copy)]
pub enum Screen<'a> {
    /// The form: the category list with "Open now" or "Coming later" (BIZ-021) and the fee box.
    Form(&'a [(String, &'static str)]),
    /// Before any code: the record contact the code goes to, and a landline on the record, if any.
    Contact(&'a MaskedContact, Option<&'a MaskedContact>),
    /// "That number is old": saved, with the typical list delay in days.
    NumberOld(u32),
    /// Before a letter: the address's town and ZIP, and the typical list delay in days.
    Letter(&'a MaskedContact, u32),
    /// The done screen.
    Done(&'a DoneFacts),
}

/// What every screen needs besides the screen itself.
#[derive(Debug, Clone, Copy)]
pub struct Parts<'a> {
    /// The business-texting switch (TXT-035).
    pub texting_on: bool,
    /// The page language of the WEB-024 line.
    pub language: PageLanguage,
    /// The session's CSRF token for the posted steps (SEC-042).
    pub csrf: &'a str,
    /// The fee box's numbers (BIZ-030).
    pub fees: FeeNumbers,
}

#[derive(Template, Default)]
#[template(path = "business_signup.html")]
struct SignupView<'a> {
    title: &'static str,
    notice: &'static str,
    someone_else: &'static str,
    lines: Vec<String>,
    categories: Vec<(String, String, &'static str)>,
    pest: bool,
    pest_attestation: &'static str,
    fee_lines: Vec<String>,
    links: Vec<(String, String)>,
    buttons: Vec<(&'static str, &'static str)>,
    csrf: &'a str,
}

/// BIZ-024, BIZ-026, BIZ-030, BIZ-031, LIC-047, WEB-024: the page for `screen`; see the module
/// docs. The fee box (form only) sits above the Terms link.
///
/// # Errors
/// The template's render error.
pub fn signup_page(screen: Screen<'_>, parts: &Parts<'_>) -> Result<String, askama::Error> {
    let mut v = SignupView {
        notice: notice_line(parts.texting_on, parts.language),
        someone_else: SOMEONE_ELSE_LINE,
        pest_attestation: PEST_ATTESTATION,
        csrf: parts.csrf,
        ..SignupView::default()
    };
    match screen {
        Screen::Form(categories) => {
            v.title = "Sign up your business";
            v.categories = categories
                .iter()
                .map(|(c, l)| (c.clone(), c.replace('_', " "), *l))
                .collect();
            v.pest = categories.iter().any(|(c, _)| c == "pest_control");
            v.fee_lines = fee_summary(&parts.fees);
        }
        Screen::Contact(to, landline) => {
            v.title = "Where your code goes";
            v.lines = landline
                .into_iter()
                .chain([to])
                .map(MaskedContact::line)
                .collect();
            v.buttons = vec![("send", "Send the code"), ("old", NUMBER_OLD_BUTTON)];
        }
        Screen::NumberOld(delay) => {
            v.title = "Your sign-up is saved";
            v.lines = fix_record_lines(delay).to_vec();
        }
        Screen::Letter(to, delay) => {
            v.title = "Your code comes by letter";
            v.lines = [to.line()]
                .into_iter()
                .chain(fix_record_lines(delay))
                .collect();
            v.links = vec![(
                "https://roc.az.gov".to_owned(),
                "How to update your ROC record".to_owned(),
            )];
            v.buttons = vec![("letter", "Mail the letter"), ("old", STOP_FIX_BUTTON)];
        }
        Screen::Done(facts) => {
            v.title = "You're signed up";
            (v.lines, v.links) = done_lines(facts);
        }
    }
    v.render()
}
