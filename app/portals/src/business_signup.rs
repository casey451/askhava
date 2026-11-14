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
//!
//! T165 added the business name and trade name, the license numbers (ROC, Board of Technical
//! Registration, a no-license business's city license number) and the Google and Yelp links to
//! the form; [`Screen::FormAgain`] shows the form again with what was typed ([`Entered`]) and the
//! refused field's reason (BIZ-022, BIZ-052, LIC-011, PICK-041; D-2943, D-2954). The checks are
//! `businesses::api::save_business_name` and `check_links`; the `POST` is T302's.
//!
//! T155 added the capability step (BIZ-049, BIZ-004, CARD-001; D-393, D-492): [`capability_page`]
//! over `templates/capability_setup.html` shows the checklist `businesses::api::checklist_offer`
//! builds from the license map (pre-ticked, untick any, no-license extras only), then towns,
//! largest job, recurring work and after-hours, as checkboxes on one page ([`CapabilityStep::All`])
//! or one question per guided screen; [`CapabilityValues`] reads either post the same way and
//! [`capability_summary`] reads the answers back on the check screen. Saving is
//! `businesses::api::save_capability`; the route waits with the rest of sign-up (D-1709).
//!
//! T248 added the service area beside towns served (BIZ-058, BIZ-049, A11Y-010 to A11Y-013;
//! D-1518, D-3116): [`Screen::ServiceArea`] asks the base ZIP and [`RADIUS_QUESTION`] on their own
//! screen after the towns question, with every typed value kept and a focused error summary;
//! `businesses::api::ServiceArea::parse` checks them. [`CapabilityValues`] carries both as hidden
//! fields, so the check screen reads them back ([`service_area_lines`], BIZ-030), and
//! [`signup_can_finish`] is false until a business has towns and a service area.
//!
//! T268 added the E-SIGN consent box (CON-019, PICK-049; D-1715, D-3187): the form shows
//! `businesses::api::consent_box` for [`signup_channels`] (email, and texts when the business chose
//! them), never ticked; [`submit_signup`] refuses a form without the tick before anything is stored
//! ([`SignupRefusal::NoConsent`], shown as [`CONSENT_MISSING`] by the box through
//! [`Entered::problem`] on the `consent` field, [`refusal_problem`]) and a text box with no phone
//! ([`SignupRefusal::NoTextPhone`], [`TEXT_PHONE_MISSING`]), then signs up and, in one
//! transaction, records the owner, links a chosen text phone and stores one consent row per
//! channel.
//!
//! T450 added the code screens and the notice-channel choice (BIZ-002, BIZ-036, BIZ-040, BIZ-042,
//! CON-019, TXT-035, A11Y-030; D-3602): one route constant per sign-up `POST` step, every form's
//! `action` taken from them; [`Screen::IdentityCode`] and [`Screen::EmailCode`] with one
//! one-time-code input and the refusal beside it ([`CodeProblem`]); while texting is on, the email,
//! text or both choice and the text phone field, always rendered, never shown or hidden by script.
//! The consent wording is rendered on the server but the channel is chosen in the browser, so the
//! form carries the channels its wording named and their seal (`link_token` over the SHA256 of
//! the length-prefixed label, channels, [`CONSENT_VERSION`] and CSRF token, [`sealed_signup_page`]);
//! [`ChannelPost::parse`] keeps the shown channels only when the seal verifies (constant time), and
//! [`ChannelPost::consent_channels`] is `None` unless they cover the choice, so `FormAgain` comes
//! back with the wording naming texts and nothing is stored. The `POST` routes are T302's.

use std::collections::BTreeSet;

use askama::Template;
use askhava_adapters::api::PaymentsAdapter;
use askhava_businesses::api::{
    Acting, ConsentChannel, ConsentError, SenderLink, SignUpForm, SignedUp, SignupError, TeamError,
    add_owner, consent_box, link_phone, record_consent, sign_up,
};
use askhava_businesses::api::{
    BusinessesError, ServiceArea, ServiceAreaProblem, service_area, towns as stored_towns,
};
use askhava_businesses::api::{CONSENT_VERSION, consent_wording};
use askhava_businesses::api::{
    Capability, CapabilityAnswers, JobTypeChoice, LargestJob, TradeOffer, kept_ticks, pre_ticked,
};
use askhava_businesses::api::{
    ContactKind, FeeNumbers, MaskedContact, NUMBER_OLD_BUTTON, SOMEONE_ELSE_LINE, STOP_FIX_BUTTON,
    fee_summary, fix_record_lines, trade_count,
};
use askhava_core::api::{IdSecret, LinkKind, LinkSalt, LinkToken, link_token};
use askhava_licensing::api::ClassMap;
use askhava_payments::api::Tier;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use subtle::ConstantTimeEq;

use crate::pick_flow::PageLanguage;

/// The sign-up's path on askhava.com; each step posts to a path under it.
pub const SIGNUP_ROUTE: &str = "/b/signup";
/// BIZ-024, CON-019, D-3602: the form posts here.
pub const SIGNUP_FORM_ROUTE: &str = "/b/signup/start";
/// BIZ-026, D-3602: "Send the code" on the contact screen and "Send a new code" by the identity code.
pub const CODE_SEND_ROUTE: &str = "/b/signup/send";
/// BIZ-026, D-288: "That number is old" and "Stop, I'll fix my record first".
pub const NUMBER_OLD_ROUTE: &str = "/b/signup/old";
/// BIZ-026: "Mail the letter".
pub const LETTER_ROUTE: &str = "/b/signup/letter";
/// BIZ-002, BIZ-026, D-3602: the identity code sent to the record contact.
pub const IDENTITY_CHECK_ROUTE: &str = "/b/signup/identity-code";
/// BIZ-040, BIZ-042, D-3602: the code emailed to the sign-up address.
pub const EMAIL_CHECK_ROUTE: &str = "/b/signup/email-code";
/// BIZ-042, D-3602: the email code's resend button.
pub const EMAIL_RESEND_ROUTE: &str = "/b/signup/email-code/resend";

/// WEB-024, D-307, D-344: the notice line while business texting is off, English and Spanish.
/// Both Spanish lines here are drafts until a native speaker reviews them, as the Spanish text
/// templates are (`messaging/templates/es.json`, `"draft": true`; D-1709).
pub const NOTICE_OFF: [&str; 2] = [
    "Job requests and picks come by email. We don't text or call businesses.",
    "Las solicitudes de trabajo y las selecciones llegan por correo electrónico. No enviamos \
mensajes de texto ni llamamos a los negocios.",
];
/// WEB-024, TXT-035, WORD-001: the notice line while business texting is on, English and Spanish,
/// with the brand file's display name and its Spanish form (D-1932, T294).
#[must_use]
pub fn notice_on() -> [String; 2] {
    let brand = askhava_core::api::brand();
    [
        format!(
            "Choose how you get job requests and picks: email, text or both. {} never calls you \
with a recording.",
            brand.display_name()
        ),
        format!(
            "Elija cómo recibir las solicitudes de trabajo y las selecciones: correo electrónico, \
texto o ambos. {} nunca le llama con una grabación.",
            brand.es().display_name()
        ),
    ]
}

/// LIC-047, D-417: the pest-control attestation and its record categories.
pub const PEST_ATTESTATION: &str =
    "I hold an active Arizona Department of Agriculture pest-management business license";

/// WEB-024, TXT-035, D-493: the notice line for the business-texting switch, in `language`, with
/// its `lang` code: the sign-up page is English, so a Spanish line is marked `lang="es"` (A11Y).
#[must_use]
pub fn notice_line(texting_on: bool, language: PageLanguage) -> (String, &'static str) {
    let [en, es] = if texting_on {
        notice_on()
    } else {
        NOTICE_OFF.map(String::from)
    };
    match language {
        PageLanguage::En => (en, "en"),
        PageLanguage::Es => (es, "es"),
    }
}

/// A category code as the form shows it: "plumbing_repair" reads "Plumbing repair". The catalog
/// has no display names, so the code is the name (D-1709).
fn category_name(code: &str) -> String {
    let words = code.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// BIZ-030, D-173: the fee box's numbers from the fee settings: the three tiers
/// (`payments::api::Tier::cents`), the Confirm window and the emergency window.
#[must_use]
pub fn fee_numbers(confirm: ConfirmSettings, rules: WindowRules) -> FeeNumbers {
    FeeNumbers {
        tier_cents: [Tier::Small, Tier::Medium, Tier::Large].map(Tier::cents),
        confirm_minutes: confirm.window_minutes,
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
    /// The form again after a refused name, license number or link, with what was typed and the
    /// reason by its field (BIZ-022, BIZ-052, D-2943).
    FormAgain(&'a [(String, &'static str)], &'a Entered<'a>),
    /// The base ZIP and service radius, after the towns question (BIZ-058, BIZ-049).
    ServiceArea(&'a AreaEntered<'a>),
    /// After `Contact`: the code sent to the masked record contact, and a refused or expired one
    /// (BIZ-002, BIZ-026, A11Y-030).
    IdentityCode(&'a MaskedContact, Option<CodeProblem>),
    /// The code emailed to the sign-up address, with the resend button (BIZ-040, BIZ-042).
    EmailCode(Option<CodeProblem>),
}

/// A11Y-030, BIZ-002, BIZ-040: why a typed sign-up code was refused, shown beside the code field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeProblem {
    /// The code does not match.
    Refused,
    /// The code has expired.
    Expired,
}

impl CodeProblem {
    /// A11Y-030: the message in the summary and beside the field.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Refused => "That code doesn't match. Check it and type it again.",
            Self::Expired => "That code has expired. Use the button below to get a new one.",
        }
    }
}

/// BIZ-042: the email code's hint, word for word.
pub const NO_EMAIL_HINT: &str = "No email? Check your Spam or Junk folder.";

/// BIZ-058: the service-area screen posts here; the route waits with the rest of sign-up (D-1709).
pub const SERVICE_AREA_ROUTE: &str = "/b/signup/service-area";
/// BIZ-058, WORD-028: the radius question, draft copy for the copy panel; its Spanish draft is in
/// `docs/for-casey-review.md` (D-3116).
pub const RADIUS_QUESTION: &str = "How many miles will you travel for a job?";

/// BIZ-058, A11Y-030: the form field a service-area problem links to.
#[must_use]
pub const fn area_field(p: ServiceAreaProblem) -> &'static str {
    match p {
        ServiceAreaProblem::BaseZip => "base_zip",
        ServiceAreaProblem::Radius => "radius",
    }
}

/// BIZ-058, A11Y-030: what to do about a service-area problem, in the summary and by the field.
#[must_use]
pub const fn area_message(p: ServiceAreaProblem) -> &'static str {
    match p {
        ServiceAreaProblem::BaseZip => "Enter your 5-digit base ZIP code.",
        ServiceAreaProblem::Radius => "Enter a number of miles from 1 to 100, like 25.",
    }
}

/// What the service-area screen shows (BIZ-058, A11Y-012): the typed values, kept after a
/// refusal, the problems from `ServiceArea::parse`, and the other sign-up answers as hidden
/// fields ([`CapabilityValues::carried`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct AreaEntered<'a> {
    /// The typed base ZIP.
    pub base_zip: &'a str,
    /// The typed radius.
    pub radius: &'a str,
    /// The refused fields, in page order; empty at first render.
    pub problems: &'a [ServiceAreaProblem],
    /// The other answers, posted again as hidden fields.
    pub carried: &'a [(&'static str, String)],
}

impl AreaEntered<'_> {
    /// The message by `field`, if it was refused (A11Y-030).
    fn error_for(&self, field: &str) -> Option<&'static str> {
        let found = self.problems.iter().find(|p| area_field(**p) == field);
        found.map(|p| area_message(*p))
    }

    /// The carried answers to post again as hidden fields, less `base_zip` and `radius`, which
    /// this screen posts as its own inputs, so neither is ever posted twice (BIZ-058).
    fn hidden(&self) -> impl Iterator<Item = &(&'static str, String)> {
        let own = |n: &str| n == "base_zip" || n == "radius";
        self.carried.iter().filter(move |(n, _)| !own(n))
    }
}

/// BIZ-030, BIZ-058: the service area read back on the check screen.
#[must_use]
pub fn service_area_lines(area: &ServiceArea) -> [String; 2] {
    [
        format!("Base ZIP code: {}", area.base_zip()),
        format!("Miles you travel for a job: {}", area.radius_miles()),
    ]
}

/// BIZ-058, BIZ-049: sign-up can finish only once the business has towns served and a base ZIP
/// with a radius stored.
///
/// # Errors
/// `BusinessesError::Database`.
pub async fn signup_can_finish(
    db: &mut PgConnection,
    business: Uuid,
) -> Result<bool, BusinessesError> {
    let has_towns = !stored_towns(db, business).await?.is_empty();
    Ok(has_towns && service_area(db, business).await?.is_some())
}

/// What the business typed into the form's name, license and link fields, shown again with the
/// reason when one is refused (BIZ-022, BIZ-052, LIC-011, D-2943, D-2954). The fields post as
/// `business_name`, `trade_name`, `roc_number`, `btr_number`, `city_license_number`,
/// `google_link` and `yelp_link`; the server checks them, so none carries `required`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Entered<'a> {
    /// The business name (required by the server, D-2943).
    pub business_name: &'a str,
    /// The trade name or DBA.
    pub trade_name: &'a str,
    /// The ROC license number.
    pub roc_number: &'a str,
    /// The Board of Technical Registration number.
    pub btr_number: &'a str,
    /// The city license number of a no-license business (LIC-011).
    pub city_license_number: &'a str,
    /// The Google Business Profile link (BIZ-052).
    pub google_link: &'a str,
    /// The Yelp page link (BIZ-052).
    pub yelp_link: &'a str,
    /// The refused field's form name and the reason, from `businesses::api::NameRefusal` or
    /// `LinkRefusal`, or `consent` with [`refusal_problem`]'s message (CON-019).
    pub problem: Option<(&'static str, &'a str)>,
    /// The business chose texts (BIZ-036), so the consent box names texts too.
    pub text_chosen: bool,
    /// The notice choice as posted, kept on the form again (BIZ-036, A11Y-012).
    pub notice: NoticeChoice,
}

impl Entered<'_> {
    /// BIZ-036: the choice to check again; texts chosen with no choice posted reads as both.
    fn shown_choice(&self) -> NoticeChoice {
        if self.text_chosen && !self.notice.texts() {
            NoticeChoice::Both
        } else {
            self.notice
        }
    }
}

/// BIZ-036, TXT-035: how the business gets job requests and picks; email is the default and the
/// only choice while business texting is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeChoice {
    /// Email only.
    #[default]
    Email,
    /// Texts (account notices still come by email, CON-019).
    Text,
    /// Email and texts.
    Both,
}

impl NoticeChoice {
    /// Every choice, in page order.
    pub const ALL: [Self; 3] = [Self::Email, Self::Text, Self::Both];

    /// The posted `notice_channel` value.
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Text => "text",
            Self::Both => "both",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Email => "Email",
            Self::Text => "Text messages",
            Self::Both => "Both",
        }
    }

    /// The choice a posted value names; anything else is `None`.
    #[must_use]
    pub fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.value() == value)
    }

    /// BIZ-036: the choice includes texts.
    #[must_use]
    pub const fn texts(self) -> bool {
        matches!(self, Self::Text | Self::Both)
    }
}

/// CON-019, A11Y-030: the error beside the consent box when the form came without its tick.
pub const CONSENT_MISSING: &str = "Tick the box to agree to get notices. We send job requests by email, so we can't sign you up without it.";

/// CON-019, CALL-012, A11Y-030: the error beside the consent box when texts were chosen but no
/// phone came with the form (the box was ticked, so [`CONSENT_MISSING`] would mislead).
pub const TEXT_PHONE_MISSING: &str = "Add the phone number for job texts, or choose email only.";

/// CON-019, BIZ-036, TXT-035: the channels the sign-up's consent box covers: email, plus texts
/// when the business chose them while business texting is on.
#[must_use]
pub fn signup_channels(text_chosen: bool, texting_on: bool) -> Vec<ConsentChannel> {
    let mut channels = vec![ConsentChannel::Email];
    if text_chosen && texting_on {
        channels.push(ConsentChannel::Text);
    }
    channels
}

/// CON-019, A11Y-030, D-3602: the error by the box when the posted choice is not covered by the
/// wording she saw (or its seal failed); the box comes back unticked with the right wording.
pub const CONSENT_AGAIN: &str = "You changed how you get job requests, so the agreement below changed too. Read it, then tick \
the box to agree.";

/// The channels as the form's `consent_channels` field carries them, in a fixed order.
fn channels_value(channels: &[ConsentChannel]) -> String {
    let named = [
        (ConsentChannel::Email, "email"),
        (ConsentChannel::Text, "text"),
    ];
    let kept = named.iter().filter(|(c, _)| channels.contains(c));
    kept.map(|(_, n)| *n).collect::<Vec<_>>().join(",")
}

/// The channels a posted `consent_channels` value names; anything else is `None`.
fn channels_from(value: &str) -> Option<Vec<ConsentChannel>> {
    let (e, t) = (ConsentChannel::Email, ConsentChannel::Text);
    [vec![e], vec![t], vec![e, t]]
        .into_iter()
        .find(|c| channels_value(c) == value)
}

/// CON-019, SEC-037, D-3602: the seal over the shown channels, the wording `version` and the
/// page's CSRF token: `link_token` keyed by `ID_SECRET`, its record and salt the two halves of the
/// SHA256 of the length-prefixed fields.
pub(crate) fn seal_for(
    secret: &IdSecret,
    channels: &[ConsentChannel],
    version: &str,
    csrf: &str,
) -> Option<LinkToken> {
    let named = channels_value(channels);
    let mut hash = Sha256::new();
    for field in ["signup-consent", named.as_str(), version, csrf] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field.as_bytes());
    }
    let digest: [u8; 32] = hash.finalize().into();
    let (record, salt) = digest.split_at(16);
    let record = Uuid::from_bytes(record.try_into().ok()?);
    let salt = LinkSalt::from_bytes(salt).ok()?;
    Some(link_token(secret, LinkKind::Action, record, &salt))
}

/// CON-019, D-3602: the posted channels, only when the posted seal is theirs for this CSRF token
/// and [`CONSENT_VERSION`], compared in constant time.
fn verify_seal(
    (secret, csrf): (&IdSecret, &str),
    channels: Option<&str>,
    seal: Option<&str>,
) -> Option<Vec<ConsentChannel>> {
    let channels = channels_from(channels?)?;
    let want = seal_for(secret, &channels, CONSENT_VERSION, csrf)?;
    let same: bool = want.as_str().as_bytes().ct_eq(seal?.as_bytes()).into();
    same.then_some(channels)
}

/// BIZ-036, CON-019, TXT-035, D-3602: the form's channel, shown-wording and phone fields as
/// posted. Its `Debug` never shows the phone.
#[derive(Debug)]
pub struct ChannelPost {
    /// The notice choice; email whenever business texting is off (TXT-035).
    pub choice: NoticeChoice,
    /// The typed text phone, while texting is on. Never log it.
    pub text_phone: Option<SecretString>,
    /// The channels the shown wording named, only when their seal verified.
    pub shown: Option<Vec<ConsentChannel>>,
    texting_on: bool,
}

impl ChannelPost {
    /// BIZ-036, CON-019, TXT-035: reads `notice_channel`, `text_phone`, `consent_channels` and
    /// `consent_seal`; the caller passes `ID_SECRET` and the page's CSRF token.
    #[must_use]
    pub fn parse(pairs: &[(String, String)], texting_on: bool, seal: (&IdSecret, &str)) -> Self {
        let field = |name: &str| {
            pairs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        let posted = field("notice_channel").and_then(NoticeChoice::from_value);
        let phone = field("text_phone").map(str::trim);
        let phone = phone.filter(|p| texting_on && !p.is_empty());
        Self {
            choice: posted.filter(|_| texting_on).unwrap_or_default(),
            text_phone: phone.map(|p| SecretString::from(p.to_owned())),
            shown: verify_seal(seal, field("consent_channels"), field("consent_seal")),
            texting_on,
        }
    }

    /// BIZ-036, TXT-035: texts were chosen while business texting is on.
    #[must_use]
    pub const fn text_chosen(&self) -> bool {
        self.texting_on && self.choice.texts()
    }

    /// CON-019: the channels to store, only when the verified shown channels cover the choice;
    /// `None` stores nothing and shows `FormAgain` with [`Self::entered`].
    #[must_use]
    pub fn consent_channels(&self) -> Option<Vec<ConsentChannel>> {
        let chosen = signup_channels(self.text_chosen(), self.texting_on);
        let shown = self.shown.as_ref()?;
        chosen.iter().all(|c| shown.contains(c)).then_some(chosen)
    }

    /// CON-019: the wording to store, built on the server from the verified shown channels.
    #[must_use]
    pub fn stored_wording(&self) -> Option<String> {
        self.consent_channels()?;
        self.shown.as_deref().map(consent_wording)
    }

    /// CON-019, A11Y-030: the problem `FormAgain` shows: an uncovered or forged choice first
    /// ([`CONSENT_AGAIN`]), then texts with no phone ([`TEXT_PHONE_MISSING`]).
    #[must_use]
    pub fn problem(&self) -> Option<(&'static str, &'static str)> {
        if self.consent_channels().is_none() {
            return Some(("consent", CONSENT_AGAIN));
        }
        let no_phone = self.text_chosen() && self.text_phone.is_none();
        no_phone.then_some(("consent", TEXT_PHONE_MISSING))
    }

    /// BIZ-036, CON-019: what `FormAgain` shows: the choice kept, texts named when chosen.
    #[must_use]
    pub fn entered<'a>(&self) -> Entered<'a> {
        Entered {
            problem: self.problem(),
            text_chosen: self.text_chosen(),
            notice: self.choice,
            ..Entered::default()
        }
    }
}

/// The consent box as posted: its tick and, when texts were chosen, the phone to link as the
/// main phone with the composition's sender directory (CALL-012).
pub struct SignupConsent<'a, L> {
    /// Whether the box was ticked.
    pub ticked: bool,
    /// The channels the box covered ([`signup_channels`]).
    pub channels: &'a [ConsentChannel],
    /// The directory and the number for a text box.
    pub text_phone: Option<(&'a L, &'a SecretString)>,
}

/// Why [`submit_signup`] stored nothing, or stopped after the sign-up.
#[derive(Debug)]
pub enum SignupRefusal {
    /// The box was not ticked: nothing is stored ([`CONSENT_MISSING`]).
    NoConsent,
    /// The box was ticked for texts but no phone came with it: nothing is stored
    /// ([`TEXT_PHONE_MISSING`]).
    NoTextPhone,
    /// The sign-up itself was refused (nothing stored) or failed.
    Signup(SignupError),
    /// The owner or the phone could not be stored.
    Team(TeamError),
    /// The consent could not be stored.
    Consent(ConsentError),
}

/// CON-019, BIZ-001, CALL-012, D-1715: the form's post. Without the tick it stores nothing; else
/// `businesses::api::sign_up` (its own transaction, the Stripe customer after it, R11), then one
/// transaction holding the owner, the text phone when chosen and one consent row per channel with
/// its version and time. A text box with no phone is refused before anything is stored.
///
/// # Errors
/// [`SignupRefusal`].
pub async fn submit_signup<L: SenderLink>(
    pool: &PgPool,
    (stripe, id_secret, map): (&impl PaymentsAdapter, &SecretString, &ClassMap),
    form: &SignUpForm,
    consent: SignupConsent<'_, L>,
    now: Timestamp,
) -> Result<SignedUp, SignupRefusal> {
    let text = consent.channels.contains(&ConsentChannel::Text);
    if !consent.ticked || consent.channels.is_empty() {
        return Err(SignupRefusal::NoConsent);
    }
    if text && consent.text_phone.is_none() {
        return Err(SignupRefusal::NoTextPhone);
    }
    let up = sign_up(pool, (stripe, id_secret), map, form, now)
        .await
        .map_err(SignupRefusal::Signup)?;
    let mailbox = form
        .mailbox_id
        .ok_or(SignupRefusal::Signup(SignupError::NoEmail))?;
    let db = |_| SignupRefusal::Team(TeamError::Database);
    let mut tx = pool.begin().await.map_err(db)?;
    let owner = add_owner(&mut tx, up.business, mailbox, now)
        .await
        .map_err(SignupRefusal::Team)?;
    let phone = match consent.text_phone.filter(|_| text) {
        Some((dir, number)) => {
            let by = Acting {
                business: up.business,
                member: owner,
            };
            let line = link_phone(&mut tx, dir, (by, true), number, now).await;
            Some(line.map_err(SignupRefusal::Team)?)
        }
        None => None,
    };
    let who = (up.business, owner, phone);
    record_consent(&mut tx, who, consent.channels, true, now)
        .await
        .map_err(SignupRefusal::Consent)?;
    tx.commit().await.map_err(db)?;
    Ok(up)
}

/// A11Y-030, CON-019: the form field and message [`Entered::problem`] shows for a refusal that
/// stored nothing, or `None` for one the form cannot fix (a sign-up, team or consent failure).
#[must_use]
pub fn refusal_problem(refusal: &SignupRefusal) -> Option<(&'static str, &'static str)> {
    match refusal {
        SignupRefusal::NoConsent => Some(("consent", CONSENT_MISSING)),
        SignupRefusal::NoTextPhone => Some(("consent", TEXT_PHONE_MISSING)),
        SignupRefusal::Signup(_) | SignupRefusal::Team(_) | SignupRefusal::Consent(_) => None,
    }
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

/// A code screen's form: where it posts, the hint under the label, and the refusal (A11Y-030).
#[derive(Debug, Clone, Copy)]
struct CodeForm {
    action: &'static str,
    hint: &'static str,
    error: Option<&'static str>,
}

#[derive(Template, Default)]
#[template(path = "business_signup.html")]
struct SignupView<'a> {
    title: &'static str,
    notice: String,
    notice_lang: &'static str,
    someone_else: &'static str,
    lines: Vec<String>,
    categories: Vec<(String, String, &'static str)>,
    pest: bool,
    pest_attestation: &'static str,
    fee_lines: Vec<String>,
    links: Vec<(String, String)>,
    buttons: Vec<(&'static str, &'static str, &'static str)>,
    csrf: &'a str,
    entered: Entered<'a>,
    area: Option<AreaEntered<'a>>,
    consent: String,
    texting_on: bool,
    choices: Vec<(&'static str, &'static str, bool)>,
    consent_channels: String,
    consent_seal: String,
    code_form: Option<CodeForm>,
}

impl SignupView<'_> {
    /// The reason shown by `field`, if it was the refused one (A11Y-030); a missing text phone
    /// shows by the phone field instead of the box.
    fn error_for(&self, field: &str) -> Option<&str> {
        let by_phone = self.phone_error().is_some();
        self.entered
            .problem
            .filter(|(f, _)| *f == field && !(by_phone && field == "consent"))
            .map(|(_, r)| r)
    }

    /// CON-019, A11Y-030: [`TEXT_PHONE_MISSING`] beside the text phone field, while it is shown.
    fn phone_error(&self) -> Option<&str> {
        let shown = |r: &&str| self.texting_on && *r == TEXT_PHONE_MISSING;
        self.entered.problem.map(|(_, r)| r).filter(shown)
    }

    /// A11Y-030: the summary link's target, the field the message sits beside.
    fn problem_anchor(&self) -> String {
        match self.entered.problem {
            Some(_) if self.phone_error().is_some() => "text-phone".to_owned(),
            Some((field, _)) => field.replace('_', "-"),
            None => String::new(),
        }
    }
}

/// BIZ-022, BIZ-030, BIZ-036, LIC-047, CON-019, TXT-035: the form's title, categories, pest
/// fields, fee box, the notice choice while texting is on, and the box with its channels and their
/// seal; the title opens with "Error: " when a typed field was refused (A11Y-030).
fn fill_form<'a>(
    v: &mut SignupView<'a>,
    categories: &[(String, &'static str)],
    parts: &Parts<'a>,
    secret: Option<&IdSecret>,
) {
    v.title = if v.entered.problem.is_some() {
        "Error: Sign up your business"
    } else {
        "Sign up your business"
    };
    v.categories = categories
        .iter()
        .map(|(c, l)| (c.clone(), category_name(c), *l))
        .collect();
    v.pest = categories.iter().any(|(c, _)| c == "pest_control");
    v.fee_lines = fee_summary(&parts.fees);
    v.texting_on = parts.texting_on;
    let chosen = v.entered.shown_choice();
    let choices = NoticeChoice::ALL.into_iter();
    v.choices = choices
        .map(|c| (c.value(), c.label(), c == chosen))
        .collect();
    let channels = signup_channels(v.entered.text_chosen, parts.texting_on);
    v.consent = consent_box(&channels).wording;
    v.consent_channels = channels_value(&channels);
    let seal = secret.and_then(|s| seal_for(s, &channels, CONSENT_VERSION, parts.csrf));
    v.consent_seal = seal.map(|t| t.as_str().to_owned()).unwrap_or_default();
}

/// BIZ-026: the identity code's hint; only a texted code may be filled in by the phone.
fn code_hint(to: &MaskedContact) -> &'static str {
    match to.kind() {
        ContactKind::Mobile => "Type or paste the code. Your phone may fill it in.",
        _ => "Type or paste the code.",
    }
}

/// BIZ-002, BIZ-026, BIZ-040, BIZ-042, A11Y-030: a code screen's title, lines, form and button.
fn fill_code(v: &mut SignupView<'_>, to: Option<&MaskedContact>, problem: Option<CodeProblem>) {
    let error = problem.map(CodeProblem::message);
    let (title, line, form, button) = match to {
        Some(to) => (
            [
                "Show this business is yours",
                "Error: Show this business is yours",
            ],
            format!(
                "We sent a 6-digit code to the {} on your ROC or city license record: {}.",
                to.kind().word(),
                to.shown()
            ),
            (IDENTITY_CHECK_ROUTE, code_hint(to)),
            (CODE_SEND_ROUTE, "new-code", "Send a new code"),
        ),
        None => (
            [
                "Check your email for a code",
                "Error: Check your email for a code",
            ],
            "We emailed a 6-digit code to the address you gave for job requests. Type it below."
                .to_owned(),
            (EMAIL_CHECK_ROUTE, NO_EMAIL_HINT),
            (EMAIL_RESEND_ROUTE, "resend", "Send the email again"),
        ),
    };
    v.title = title[usize::from(error.is_some())];
    v.lines = vec![line];
    v.code_form = Some(CodeForm {
        action: form.0,
        hint: form.1,
        error,
    });
    v.buttons = vec![button];
}

/// BIZ-058, A11Y-030: the service-area screen's title, opening with "Error: " after a refusal.
fn fill_area<'a>(v: &mut SignupView<'a>, area: &AreaEntered<'a>) {
    v.title = if area.problems.is_empty() {
        "How far do you travel for jobs?"
    } else {
        "Error: How far do you travel for jobs?"
    };
    v.area = Some(*area);
}

/// BIZ-024, BIZ-026, BIZ-030, BIZ-031, LIC-047, WEB-024: the page for `screen`; see the module
/// docs. The fee box (form only) sits above the Terms link. Its form carries no consent seal, so
/// a post of it stores no consent: serve a form through [`sealed_signup_page`] (D-3602).
///
/// # Errors
/// The template's render error.
pub fn signup_page(screen: Screen<'_>, parts: &Parts<'_>) -> Result<String, askama::Error> {
    render_signup(screen, parts, None)
}

/// CON-019, D-3602: [`signup_page`] with the form's shown channels sealed with `secret`
/// (`ID_SECRET`) for `parts.csrf`, so [`ChannelPost::parse`] can verify them.
///
/// # Errors
/// The template's render error.
pub fn sealed_signup_page(
    screen: Screen<'_>,
    parts: &Parts<'_>,
    secret: &IdSecret,
) -> Result<String, askama::Error> {
    render_signup(screen, parts, Some(secret))
}

fn render_signup(
    screen: Screen<'_>,
    parts: &Parts<'_>,
    secret: Option<&IdSecret>,
) -> Result<String, askama::Error> {
    let (notice, notice_lang) = notice_line(parts.texting_on, parts.language);
    let mut v = SignupView {
        notice,
        notice_lang,
        someone_else: SOMEONE_ELSE_LINE,
        pest_attestation: PEST_ATTESTATION,
        csrf: parts.csrf,
        ..SignupView::default()
    };
    match screen {
        Screen::Form(categories) => fill_form(&mut v, categories, parts, secret),
        Screen::FormAgain(categories, entered) => {
            v.entered = *entered;
            fill_form(&mut v, categories, parts, secret);
        }
        Screen::IdentityCode(to, problem) => fill_code(&mut v, Some(to), problem),
        Screen::EmailCode(problem) => fill_code(&mut v, None, problem),
        Screen::ServiceArea(area) => fill_area(&mut v, area),
        other => fill_record(&mut v, other),
    }
    v.render()
}

/// BIZ-026, BIZ-031: the record-contact, saved, letter and done screens.
fn fill_record(v: &mut SignupView<'_>, screen: Screen<'_>) {
    match screen {
        Screen::Contact(to, landline) => {
            v.title = "Where your code goes";
            let shown = landline.into_iter().chain([to]);
            v.lines = shown.map(MaskedContact::line).collect();
            v.buttons = vec![
                (CODE_SEND_ROUTE, "send", "Send the code"),
                (NUMBER_OLD_ROUTE, "old", NUMBER_OLD_BUTTON),
            ];
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
            v.buttons = vec![
                (LETTER_ROUTE, "letter", "Mail the letter"),
                (NUMBER_OLD_ROUTE, "old", STOP_FIX_BUTTON),
            ];
        }
        Screen::Done(facts) => {
            v.title = "You're signed up";
            (v.lines, v.links) = done_lines(facts);
        }
        _ => {}
    }
}

/// BIZ-049: the capability step's path; `?step=<slug>` picks one guided screen.
pub const CAPABILITY_ROUTE: &str = "/b/signup/capability";

/// BIZ-049, D-393: one screen of the capability step. `All` is every question as checkboxes on
/// one page; the others are the guided sequence, one question per screen, in [`Self::GUIDED`]
/// order, ending with `Check`, which reads the answers back (BIZ-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityStep {
    /// Every question on one page.
    All,
    /// The job-type checklist.
    JobTypes,
    /// The towns served (BIZ-010).
    Towns,
    /// The largest job.
    Largest,
    /// Recurring work.
    Recurring,
    /// After-hours emergency jobs (BIZ-007).
    AfterHours,
    /// The answers read back before saving.
    Check,
}

impl CapabilityStep {
    /// The guided sequence, in order.
    pub const GUIDED: [Self; 6] = [
        Self::JobTypes,
        Self::Towns,
        Self::Largest,
        Self::Recurring,
        Self::AfterHours,
        Self::Check,
    ];

    /// The `step` value in the path.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::JobTypes => "jobs",
            Self::Towns => "towns",
            Self::Largest => "largest",
            Self::Recurring => "recurring",
            Self::AfterHours => "after-hours",
            Self::Check => "check",
        }
    }

    /// The step a `step` value names; unknown is `None`.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        [Self::All]
            .into_iter()
            .chain(Self::GUIDED)
            .find(|s| s.slug() == slug)
    }

    /// The guided screen after this one; `None` after `Check` and for `All`.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        let at = Self::GUIDED.iter().position(|s| *s == self)?;
        Self::GUIDED.get(at + 1).copied()
    }
}

/// BIZ-049: a missing or bad capability answer, with its field and message (A11Y-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityProblem {
    /// No town chosen.
    Towns,
    /// No largest-job choice, or a typed amount that is not money.
    Largest,
    /// No recurring-work answer.
    Recurring,
    /// No after-hours answer.
    AfterHours,
}

impl CapabilityProblem {
    /// The form element the summary links to.
    #[must_use]
    pub const fn field(self) -> &'static str {
        match self {
            Self::Towns => "towns",
            Self::Largest => "largest",
            Self::Recurring => "recurring",
            Self::AfterHours => "after_hours",
        }
    }

    /// What to do, shown in the summary and by the field.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Towns => "Choose at least one town you work in.",
            Self::Largest => {
                "Choose the largest job you'll quote, or type an amount in dollars, like 2500."
            }
            Self::Recurring => "Choose yes or no for recurring work.",
            Self::AfterHours => "Choose yes or no for after-hours emergency jobs.",
        }
    }
}

/// BIZ-049, A11Y-012: the capability answers so far, read from posted `(name, value)` pairs.
/// Every screen carries the other answers as hidden fields, so the guided sequence and the
/// one-page form post the same pairs and reach the same answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityValues {
    /// The ticked job types.
    pub ticked: BTreeSet<JobTypeChoice>,
    /// The ticked town codes.
    pub towns: BTreeSet<String>,
    /// The largest-job choice: `none`, `1500`, `5000` or `typed`.
    pub largest: String,
    /// The typed amount.
    pub amount: String,
    /// `yes` or `no` for recurring work.
    pub recurring: String,
    /// `yes` or `no` for after-hours emergency jobs.
    pub after_hours: String,
    /// The typed base ZIP, carried from the service-area screen (BIZ-058).
    pub base_zip: String,
    /// The typed radius in miles, carried from the service-area screen (BIZ-058).
    pub radius: String,
}

/// A job type's form value, "category:job type".
fn choice_value(c: &JobTypeChoice) -> String {
    format!("{}:{}", c.category, c.job_type)
}

impl CapabilityValues {
    /// BIZ-049: the first screen's values: every job type the license covers ticked.
    #[must_use]
    pub fn fresh(offer: &[TradeOffer]) -> Self {
        Self {
            ticked: pre_ticked(offer).into_iter().collect(),
            ..Self::default()
        }
    }

    /// The values a post holds; `job_type` and `town` repeat, the rest keep the last value.
    #[must_use]
    pub fn from_pairs(pairs: &[(String, String)]) -> Self {
        let mut v = Self::default();
        for (name, value) in pairs {
            match name.as_str() {
                "job_type" => v
                    .ticked
                    .extend(value.split_once(':').map(|(c, t)| JobTypeChoice::new(c, t))),
                "town" => drop(v.towns.insert(value.clone())),
                "largest" => v.largest.clone_from(value),
                "largest_amount" => v.amount.clone_from(value),
                "recurring" => v.recurring.clone_from(value),
                "after_hours" => v.after_hours.clone_from(value),
                "base_zip" => v.base_zip.clone_from(value),
                "radius" => v.radius.clone_from(value),
                _ => {}
            }
        }
        v
    }

    /// The hidden fields that carry every answer but `step`'s own (A11Y-012).
    #[must_use]
    pub fn carried(&self, step: CapabilityStep) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if step != CapabilityStep::JobTypes {
            out.extend(self.ticked.iter().map(|c| ("job_type", choice_value(c))));
        }
        if step != CapabilityStep::Towns {
            out.extend(self.towns.iter().map(|t| ("town", t.clone())));
        }
        let singles = [
            (CapabilityStep::Largest, "largest", &self.largest),
            (CapabilityStep::Largest, "largest_amount", &self.amount),
            (CapabilityStep::Recurring, "recurring", &self.recurring),
            (CapabilityStep::AfterHours, "after_hours", &self.after_hours),
        ];
        let kept = singles
            .into_iter()
            .filter(|(s, _, v)| *s != step && !v.is_empty());
        out.extend(kept.map(|(_, n, v)| (n, v.clone())));
        out.extend(self.area_fields());
        out
    }

    /// The typed base ZIP and radius as hidden fields, on every capability screen, the one-page
    /// form included (BIZ-058, A11Y-012).
    #[must_use]
    pub fn area_fields(&self) -> Vec<(&'static str, String)> {
        let pairs = [("base_zip", &self.base_zip), ("radius", &self.radius)];
        let typed = pairs.into_iter().filter(|(_, v)| !v.is_empty());
        typed.map(|(n, v)| (n, v.clone())).collect()
    }

    /// BIZ-049: the answers, or every problem in page order.
    ///
    /// # Errors
    /// The [`CapabilityProblem`]s found.
    pub fn answers(&self) -> Result<CapabilityAnswers, Vec<CapabilityProblem>> {
        let yes_no = |s: &str| match s {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        };
        let largest = LargestJob::parse(&self.largest, &self.amount);
        let (recurring, after_hours) = (yes_no(&self.recurring), yes_no(&self.after_hours));
        let problems: Vec<CapabilityProblem> = [
            (self.towns.is_empty(), CapabilityProblem::Towns),
            (largest.is_none(), CapabilityProblem::Largest),
            (recurring.is_none(), CapabilityProblem::Recurring),
            (after_hours.is_none(), CapabilityProblem::AfterHours),
        ]
        .into_iter()
        .filter_map(|(bad, p)| bad.then_some(p))
        .collect();
        match (largest, recurring, after_hours) {
            (Some(largest), Some(recurring), Some(after_hours)) if problems.is_empty() => {
                Ok(CapabilityAnswers {
                    ticked: self.ticked.iter().cloned().collect(),
                    towns: self.towns.iter().cloned().collect(),
                    capability: Capability { largest, recurring },
                    after_hours,
                })
            }
            _ => Err(problems),
        }
    }
}

/// BIZ-049, BIZ-030: the answers read back on the check screen, towns by their names in
/// `towns`; a recurring "no" leaves the recurring job types out, as saving does.
#[must_use]
pub fn capability_summary(
    offer: &[TradeOffer],
    towns: &[(String, String)],
    answers: &CapabilityAnswers,
) -> Vec<String> {
    let kept = kept_ticks(offer, answers);
    let mut lines: Vec<String> = offer
        .iter()
        .filter_map(|t| {
            let jobs: Vec<&str> = t
                .rows
                .iter()
                .filter(|r| kept.contains(&r.choice))
                .map(|r| r.choice.job_type.as_str())
                .collect();
            (!jobs.is_empty()).then(|| format!("{}: {}", t.name, jobs.join(", ")))
        })
        .collect();
    if lines.is_empty() {
        lines.push("Jobs: none ticked".to_owned());
    }
    let named = towns.iter().filter(|(c, _)| answers.towns.contains(c));
    let names: Vec<&str> = named.map(|(_, n)| n.as_str()).collect();
    let yes = |b: bool| if b { "Yes" } else { "No" };
    lines.extend([
        format!("Towns: {}", names.join(", ")),
        format!(
            "Largest job you'll quote: {}",
            answers.capability.largest.line()
        ),
        format!("Recurring work: {}", yes(answers.capability.recurring)),
        format!("After-hours emergency jobs: {}", yes(answers.after_hours)),
    ]);
    lines
}

/// What the capability step's page shows (BIZ-049).
#[derive(Debug, Clone, Copy)]
pub struct CapabilityPage<'a> {
    /// The screen.
    pub step: CapabilityStep,
    /// The checklist from `businesses::api::checklist_offer`.
    pub offer: &'a [TradeOffer],
    /// The towns the business may pick, as (code, name).
    pub towns: &'a [(String, String)],
    /// The answers so far.
    pub values: &'a CapabilityValues,
    /// The problems after a failed save; empty at first render.
    pub problems: &'a [CapabilityProblem],
    /// The session's CSRF token (SEC-042).
    pub csrf: &'a str,
}

#[derive(Template)]
#[template(path = "capability_setup.html")]
struct CapabilityView<'a> {
    page: &'a CapabilityPage<'a>,
    problems: Vec<CapabilityProblem>,
    title: &'static str,
    show: [bool; 5],
    carried: Vec<(&'static str, String)>,
    summary: Vec<String>,
    next: String,
    button: &'static str,
}

impl CapabilityView<'_> {
    /// Whether the job type is ticked now.
    fn ticked(&self, c: &JobTypeChoice) -> bool {
        self.page.values.ticked.contains(c)
    }

    /// The form value of a job type.
    fn value(c: &JobTypeChoice) -> String {
        choice_value(c)
    }

    /// Whether the town `code` is ticked now.
    fn town_ticked(&self, code: &str) -> bool {
        self.page.values.towns.contains(code)
    }

    /// Whether no row of `trade` needs a license, so its legend says "(no license needed)"; in a
    /// mixed trade each no-license row says it instead.
    fn no_license(trade: &TradeOffer) -> bool {
        trade.rows.iter().all(|r| !r.licensed)
    }

    /// The problem message for `field`, if any.
    fn error_for(&self, field: &str) -> Option<&'static str> {
        let found = self.problems.iter().find(|p| p.field() == field);
        found.map(|p| p.message())
    }
}

/// BIZ-030, BIZ-058: the check screen's lines: the capability answers, then the carried base ZIP
/// and radius when they parse.
fn check_summary(page: &CapabilityPage<'_>, answers: &CapabilityAnswers) -> Vec<String> {
    let area = ServiceArea::parse(&page.values.base_zip, &page.values.radius);
    let area_lines = area.iter().flat_map(service_area_lines);
    let lines = capability_summary(page.offer, page.towns, answers);
    lines.into_iter().chain(area_lines).collect()
}

/// BIZ-049, BIZ-004, BIZ-007, BIZ-010, BIZ-030, A11Y-012, A11Y-030: the capability step's page
/// over `templates/capability_setup.html`: the checklist pre-ticked from the license, the
/// no-license extras, towns, largest job, recurring work and after-hours, all on one page or one
/// per guided screen; the check screen reads the answers back. The form posts to
/// [`CAPABILITY_ROUTE`] with `step` set to the next screen. D-2511: a check screen whose answers
/// are incomplete offers no "Save and continue"; it shows the one-page form with the missing
/// answers as problems.
///
/// # Errors
/// The template's render error.
pub fn capability_page(page: &CapabilityPage<'_>) -> Result<String, askama::Error> {
    use CapabilityStep as S;
    let answers = page.values.answers();
    let mut problems = page.problems.to_vec();
    let step = match (page.step, &answers) {
        (S::Check, Err(found)) => {
            problems.extend(found.iter().filter(|f| !page.problems.contains(f)));
            S::All
        }
        (s, _) => s,
    };
    let all = step == S::All;
    let show = [
        S::JobTypes,
        S::Towns,
        S::Largest,
        S::Recurring,
        S::AfterHours,
    ]
    .map(|s| all || s == step);
    let summary = match (step, answers) {
        (S::Check, Ok(a)) => check_summary(page, &a),
        _ => Vec::new(),
    };
    let (title, button) = match step {
        S::All => ("Pick the jobs we send you", "Check your answers"),
        S::Check => ("Check your answers", "Save and continue"),
        _ => ("Pick the jobs we send you", "Next"),
    };
    let carried = if all {
        page.values.area_fields()
    } else {
        page.values.carried(step)
    };
    let next = step.next().unwrap_or(S::Check).slug().to_owned();
    CapabilityView {
        page,
        problems,
        title,
        show,
        carried,
        summary,
        next,
        button,
    }
    .render()
}
