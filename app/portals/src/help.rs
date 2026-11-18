//! The business portal's Get help form and the site help page's message form (T200; BIZ-047,
//! OPS-036, CON-034, CON-035, PRIV-031, SEC-014, D-3155, D-4293, D-4372 to D-4375).
//!
//! Get help ([`GET_HELP_ROUTE`]): a signed-in business picks a [`Topic`], may give a job number and
//! writes a short message. [`GetHelpForm::parse`] checks it once at the edge; [`send_get_help`]
//! opens or joins the business's help request through `intake::api::business_help` (the owner is
//! notified at once, OPS-036) with the topic and job number in the scrubbed words, and queues the
//! answer email: the topic's own template (the help answer first, then BIZ-047's "Got it" line) or,
//! for "Something else", `business_get_help_answer` alone. [`get_help_page`] shows the form, its
//! error summary or the same answer on screen. Every page a signed-in business sees extends
//! `business_layout.html`, which puts the "Get help" link in the shared header (D-3155).
//!
//! The site help form ([`HELP_FORM_ROUTE`], `POST`): [`admit_help_form`] runs first, 5 posts per
//! client IP (an IPv6 address by its /64) per hour and 200 for everyone per Arizona day; a refused
//! post shows [`HelpFormScreen::TooMany`] and does nothing else. [`send_help_form`] then runs the
//! opt-out detector (CON-035): an opt-out with a mobile number calls `messaging::api::suppress`
//! with `RevokeChannel::HelpForm` and the screen confirms it, the same whatever `suppress` returns;
//! without a number the screen asks for one or says to reply STOP. Every message opens one help
//! request as a help email does, and a delete or copy request deletes or copies nothing: the answer
//! always links the homeowner sign-in, where the code to the number on file leads to the portal's
//! own request (PRIV-031, D-4293). The number is never stored with the request, logged or shown.
//! T309 serves both routes; this module holds the page functions and route constants only.

use std::net::IpAddr;

use askama::Template;
use askhava_core::api::brand;
use askhava_intake::api::{Admission, Key, Limit, admit, business_help, receive_help_email};
use askhava_messaging::api::{
    Language, MailboxKind, RevokeChannel, detect_opt_out, queue_account_email, render, suppress,
};
use askhava_privacy::api::{
    CommonWords, EmailAuth, Flags, InboundEmail, Known, NameMemory, ScrubbedText, SenderIndex,
    Standing, Vault, scrub,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use thiserror::Error;

use crate::business_auth::BusinessPrincipal;
use crate::homeowner_portal::HOMEOWNER_SIGNIN_ROUTE;

/// The Get help form: `GET` shows it to a signed-in business, `POST` sends it (BIZ-047).
pub const GET_HELP_ROUTE: &str = "/b/help";
/// The site help page's message form posts here (OPS-036, CON-035, D-491).
pub const HELP_FORM_ROUTE: &str = "/help";
/// The longest Get help message, in characters (BIZ-047's "short message").
pub const GET_HELP_MESSAGE_MAX: usize = 500;
/// SEC-014, D-4293: help-form posts one client IP (an IPv6 /64) may send per hour.
pub const HELP_FORM_PER_IP: Limit = Limit::hourly("portals.help_form_per_ip", 5);
/// SEC-014, SEC-015, D-4293: help-form posts everyone together may send per Arizona day; the
/// first refusal raises one High alert.
pub const HELP_FORM_ALL: Limit = Limit::daily("portals.help_form_all", 200);
/// The template the Get help answer email and screen use for "Something else" (BIZ-047).
pub const GET_HELP_ANSWER_TEMPLATE: &str = "business_get_help_answer";

/// Why a help step failed; no variant carries a value (R3, SEC-001).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HelpError {
    /// The rate-limit counter or its alert failed.
    #[error("the help form limit could not be checked")]
    Limit,
    /// The help request could not be opened.
    #[error("the help request could not be opened")]
    Request,
    /// The answer template or email failed.
    #[error("the help answer could not be made")]
    Answer,
}

/// What a Get help message is about (BIZ-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    /// A job.
    Job,
    /// A charge.
    Charge,
    /// Signing in.
    SignIn,
    /// Emails not arriving.
    Emails,
    /// Something else.
    Other,
}

impl Topic {
    /// Every topic in the order the form lists them.
    pub const ALL: [Self; 5] = [
        Self::Job,
        Self::Charge,
        Self::SignIn,
        Self::Emails,
        Self::Other,
    ];

    /// The form value.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Charge => "charge",
            Self::SignIn => "sign_in",
            Self::Emails => "emails",
            Self::Other => "other",
        }
    }

    /// The label the form shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Job => "A job",
            Self::Charge => "A charge",
            Self::SignIn => "Signing in",
            Self::Emails => "Emails not arriving",
            Self::Other => "Something else",
        }
    }

    /// BIZ-047: the answer template: the topic's help answer then the "Got it" line.
    #[must_use]
    pub const fn template(self) -> &'static str {
        match self {
            Self::Job => "business_get_help_job",
            Self::Charge => "business_get_help_charge",
            Self::SignIn => "business_get_help_sign_in",
            Self::Emails => "business_get_help_emails",
            Self::Other => GET_HELP_ANSWER_TEMPLATE,
        }
    }

    fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.code() == code)
    }
}

/// The values a Get help post carried, kept as typed for the form shown again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetHelpInput {
    /// The topic's form value.
    pub topic: String,
    /// The job number as typed.
    pub job: String,
    /// The message as typed.
    pub message: String,
}

/// A checked Get help message (R1, R2): a topic, an optional job number and 1 to
/// [`GET_HELP_MESSAGE_MAX`] characters of message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetHelpForm {
    topic: Topic,
    job: Option<u32>,
    message: String,
}

/// One form problem: the field's ID and what to do.
pub type Problem = (&'static str, &'static str);

impl GetHelpForm {
    /// BIZ-047, A11Y-030: checks `input` once; the problems are in form order.
    ///
    /// # Errors
    ///
    /// Each field that is missing or wrong, with what to do.
    pub fn parse(input: &GetHelpInput) -> Result<Self, Vec<Problem>> {
        let topic = Topic::parse(input.topic.trim());
        let job = input.job.trim();
        let job_number = (!job.is_empty()).then(|| job.parse::<u32>().ok());
        let message = input.message.trim();
        let mut problems = Vec::new();
        if topic.is_none() {
            problems.push(("topic", "Pick what your message is about."));
        }
        if job_number == Some(None) {
            problems.push((
                "job",
                "Enter the job number in digits, like 1042, or leave it blank.",
            ));
        }
        if message.is_empty() {
            problems.push(("message", "Write your message."));
        } else if message.chars().count() > GET_HELP_MESSAGE_MAX {
            problems.push(("message", "Make your message 500 characters or fewer."));
        }
        match (topic, problems.is_empty()) {
            (Some(topic), true) => Ok(Self {
                topic,
                job: job_number.flatten(),
                message: message.to_owned(),
            }),
            _ => Err(problems),
        }
    }

    /// The topic.
    #[must_use]
    pub const fn topic(&self) -> Topic {
        self.topic
    }
}

/// BIZ-047: the answer for `topic` as paragraphs, the "Got it" line last.
///
/// # Errors
///
/// [`HelpError::Answer`] when the template is missing.
pub fn get_help_answer(topic: Topic) -> Result<Vec<String>, HelpError> {
    let text = render(topic.template(), Language::En, false, &[]).map_err(|_| HelpError::Answer)?;
    Ok(text.body.lines().map(str::to_owned).collect())
}

/// What a sent Get help message gave back: the answer shown and the queued email's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetHelpSent {
    /// The answer paragraphs, "Got it" last.
    pub answer: Vec<String>,
    /// The queued answer email; the caller sends it after commit with `messaging::api::send_email`.
    pub email: Uuid,
}

/// BIZ-047, OPS-036, D-4375: opens or joins `who`'s business help request with the topic, job
/// number and message (scrubbed, the CARD-008 gate before the owner sees it), then queues the
/// answer email to the person signed in, keyed by the request so a joined message mails nothing new.
///
/// # Errors
///
/// [`HelpError::Request`] or [`HelpError::Answer`].
pub async fn send_get_help(
    db: &mut PgConnection,
    mail: (&SecretString, (&str, MailboxKind)),
    who: &BusinessPrincipal,
    form: &GetHelpForm,
    now: Timestamp,
) -> Result<GetHelpSent, HelpError> {
    // The job goes last: a number before a word reads to `scrub` as a street address (D-4376).
    let job = form.job.map_or_else(String::new, |n| format!(" Job {n}."));
    let said = format!("Get help: {}. {}{job}", form.topic.label(), form.message);
    let words = scrubbed(&said);
    let opened = business_help(db, (Uuid::new_v4(), who.business()), &words, now)
        .await
        .map_err(|_| HelpError::Request)?;
    let answer = get_help_answer(form.topic)?;
    let subject = format!("{}: Got it", brand().short_name());
    let ids = (form.topic.template(), who.member(), &[opened.request()][..]);
    let (email, _) = queue_account_email(db, mail, ids, (&subject, &answer), now)
        .await
        .map_err(|_| HelpError::Answer)?;
    Ok(GetHelpSent { answer, email })
}

fn scrubbed(raw: &str) -> ScrubbedText {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(raw, &known, &mut NameMemory::default())
}

#[derive(Template)]
#[template(path = "get_help.html")]
struct GetHelpView<'a> {
    csrf: &'a str,
    topics: Vec<(&'static str, &'static str)>,
    kept: &'a GetHelpInput,
    problems: &'a [Problem],
    answer: Option<(&'a [String], &'a str)>,
    message_max: usize,
}

impl GetHelpView<'_> {
    fn error_for(&self, field: &str) -> Option<&'static str> {
        self.problems.iter().find(|p| p.0 == field).map(|p| p.1)
    }
}

/// Which Get help screen to show.
#[derive(Debug, Clone, Copy)]
pub enum GetHelpScreen<'a> {
    /// The empty form.
    Ask,
    /// The form again with the values kept and its problems (A11Y-030).
    Problems(&'a GetHelpInput, &'a [Problem]),
    /// The answer after a send (BIZ-047).
    Sent(&'a [String]),
}

/// BIZ-047, A11Y-030: the Get help page for `screen`, its form carrying `csrf` (SEC-042).
///
/// # Errors
///
/// The template's render error.
pub fn get_help_page(csrf: &str, screen: GetHelpScreen<'_>) -> Result<String, askama::Error> {
    let empty = GetHelpInput::default();
    let (kept, problems, answer) = match screen {
        GetHelpScreen::Ask => (&empty, &[][..], None),
        GetHelpScreen::Problems(kept, problems) => (kept, problems, None),
        GetHelpScreen::Sent(lines) => (&empty, &[][..], lines.split_last()),
    };
    GetHelpView {
        csrf,
        topics: Topic::ALL.iter().map(|t| (t.code(), t.label())).collect(),
        kept,
        problems,
        answer: answer.map(|(last, first)| (first, last.as_str())),
        message_max: GET_HELP_MESSAGE_MAX,
    }
    .render()
}

/// The client IP a limit counts: an IPv4 address as is, an IPv6 address by its /64 (D-4293).
#[must_use]
pub fn help_form_client(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => IpAddr::V6((u128::from(v6) & !((1u128 << 64) - 1)).into()),
    }
}

/// SEC-014, SEC-015, D-4293: admits one help-form post from `ip` (as
/// `portals::api::security_event_client` reads it) before anything else runs.
///
/// # Errors
///
/// [`HelpError::Limit`] when the counter or alert fails.
pub async fn admit_help_form(
    pool: &PgPool,
    ip: IpAddr,
    now: Timestamp,
) -> Result<Admission, HelpError> {
    let checks = [
        (HELP_FORM_PER_IP, Key::Ip(help_form_client(ip))),
        (HELP_FORM_ALL, Key::All),
    ];
    admit(pool, &checks, now)
        .await
        .map_err(|_| HelpError::Limit)
}

/// What the site help form shows after a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpFormScreen {
    /// The message went to the owner.
    Sent,
    /// It asked to stop texts and gave a number: texts to it are off (CON-035).
    OptedOut,
    /// It asked to stop texts with no number: add one or reply STOP.
    NeedsNumber,
    /// The post was refused by [`admit_help_form`]; nothing else ran.
    TooMany,
}

/// OPS-036, CON-035, PRIV-031, D-4293: an admitted help-form post: an opt-out with a `mobile`
/// number suppresses it (`RevokeChannel::HelpForm`, the result never shown), and the message
/// opens one help request with no record tied to it; it deletes or copies nothing.
///
/// # Errors
///
/// [`HelpError::Request`] when the help request cannot be opened.
pub async fn send_help_form(
    db: &mut PgConnection,
    vault: &Vault,
    (message, mobile): (&str, Option<&str>),
    now: Timestamp,
) -> Result<HelpFormScreen, HelpError> {
    let mobile = mobile.map(str::trim).filter(|m| !m.is_empty());
    let screen = match (detect_opt_out(message), mobile) {
        (None, _) => HelpFormScreen::Sent,
        (Some(_), None) => HelpFormScreen::NeedsNumber,
        (Some(how), Some(phone)) => {
            let how = how.label();
            if suppress(db, vault, phone, (RevokeChannel::HelpForm, &how), now)
                .await
                .is_err()
            {
                tracing::warn!("help form opt-out not recorded");
            }
            HelpFormScreen::OptedOut
        }
    };
    receive_help_email(db, &site_email(message), now)
        .await
        .map_err(|_| HelpError::Request)?;
    Ok(screen)
}

/// D-4373: `message` as the help email `intake::api::receive_help_email` takes: scrubbed, with
/// no sender, so it ties to no record.
pub(crate) fn site_email(message: &str) -> InboundEmail {
    InboundEmail {
        flags: Flags::default(),
        text: scrubbed(message),
        auth: EmailAuth {
            spf: true,
            dkim: true,
            dmarc: true,
        },
        standing: Standing::Authenticated,
        sender: SenderIndex {
            business: Vec::new(),
            homeowner: Vec::new(),
        },
    }
}

#[derive(Template)]
#[template(
    ext = "html",
    source = r#"{% extends "base.html" %}{% block title %}Help{% endblock %}{% block main %}
<h1>{% if too_many %}Too many messages{% else %}Message sent{% endif %}</h1>
<div id="help-answer" role="status" tabindex="-1" autofocus>
{% for line in lines %}<p>{{ line }}</p>
{% endfor %}</div>
{% if !too_many %}<p>To get a copy of your data or have it deleted, <a href="{{ sign_in }}">sign in with your phone number</a>. We'll text a code to the number we have on file. Once you're in, ask from your account. We reply within 30 days.</p>
{% endif %}<p><a href="/help">Back to Help</a></p>
{% endblock %}"#
)]
struct HelpFormView<'a> {
    too_many: bool,
    lines: Vec<String>,
    sign_in: &'a str,
}

/// OPS-036, CON-035, PRIV-031, SEC-014, D-4293: the page the site help form answers with; every
/// screen but [`HelpFormScreen::TooMany`] links the homeowner sign-in.
///
/// # Errors
///
/// The template's render error.
pub fn help_form_page(screen: HelpFormScreen) -> Result<String, askama::Error> {
    let b = brand();
    let got = format!(
        "The {} owner has your message. Replies usually come within 2 days, or within 24 hours if \
         your request is urgent. In an emergency, call 911.",
        b.display_name()
    );
    let lines = match screen {
        HelpFormScreen::Sent => vec![got],
        HelpFormScreen::OptedOut => vec![
            "We've stopped texting that number. To turn texts back on, reply START to any text from us.".to_owned(),
            got,
        ],
        HelpFormScreen::NeedsNumber => vec![
            "We can't stop texts without your mobile number. Send this form again with it, or reply \
             STOP to any text from us.".to_owned(),
            got,
        ],
        HelpFormScreen::TooMany => vec![format!(
            "Too many messages. Try again later, or email help@{}; reply STOP to any text to stop texts.",
            b.domain()
        )],
    };
    HelpFormView {
        too_many: screen == HelpFormScreen::TooMany,
        lines,
        sign_in: HOMEOWNER_SIGNIN_ROUTE,
    }
    .render()
}

#[cfg(test)]
mod tests {
    //! The Get help form's checks, pages and answers, and the help-form screens (BIZ-047,
    //! CON-035, PRIV-031, D-4293).

    use super::*;

    fn input(topic: &str, job: &str, message: &str) -> GetHelpInput {
        GetHelpInput {
            topic: topic.to_owned(),
            job: job.to_owned(),
            message: message.to_owned(),
        }
    }

    /// TD-BIZ-23, TD-CON-22: "emails not arriving" answers with its help answer first and the
    /// fixed "Got it ... within 2 days" line last, on screen with data-fixed="BIZ-047".
    #[test]
    fn td_biz_23_the_answer_puts_the_topic_answer_first() {
        let lines = get_help_answer(Topic::Emails).expect("answer");
        let short = brand().short_name();
        let got = format!(
            "{short}: Got it. The {} owner has your message and usually replies within 2 days, \
             or within 24 hours when a pick or an emergency job is waiting.",
            brand().display_name()
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("spam"), "{lines:?}");
        assert_eq!(lines[1], got);
        let page = get_help_page("t", GetHelpScreen::Sent(&lines)).expect("page");
        assert!(
            page.find("spam").expect("answer")
                < page.find("data-fixed=\"BIZ-047\"").expect("fixed")
        );
        assert!(page.contains(&got.replace('\'', "&#39;")) || page.contains(&got));
        let other = get_help_answer(Topic::Other).expect("answer");
        assert_eq!(other, vec![got]);
    }

    #[test]
    fn every_topic_has_an_answer_ending_in_got_it() {
        for topic in Topic::ALL {
            let lines = get_help_answer(topic).expect("answer");
            assert!(lines.last().expect("line").contains("Got it."), "{topic:?}");
        }
    }

    #[test]
    fn the_form_checks_each_field_once() {
        let ok =
            GetHelpForm::parse(&input("emails", " 1042 ", "No emails since Monday.")).expect("ok");
        assert_eq!((ok.topic(), ok.job), (Topic::Emails, Some(1042)));
        assert!(GetHelpForm::parse(&input("other", "", "Hi")).is_ok());
        let bad = GetHelpForm::parse(&input("", "10a", " ")).expect_err("bad");
        assert_eq!(
            bad.iter().map(|p| p.0).collect::<Vec<_>>(),
            ["topic", "job", "message"]
        );
        let long = "x".repeat(GET_HELP_MESSAGE_MAX + 1);
        assert!(GetHelpForm::parse(&input("job", "", &long)).is_err());
    }

    #[test]
    fn a_failed_submit_keeps_values_and_links_the_summary() {
        let kept = input("charge", "10a", "Fake question.");
        let problems = GetHelpForm::parse(&kept).expect_err("bad");
        let page = get_help_page("tok", GetHelpScreen::Problems(&kept, &problems)).expect("page");
        assert!(page.contains("<title>Error: Get help"));
        assert!(page.contains("href=\"#job\"") && page.contains("aria-invalid=\"true\""));
        assert!(page.contains("value=\"10a\"") && page.contains("Fake question."));
        assert!(page.contains("value=\"charge\" checked") && page.contains("value=\"tok\""));
    }

    /// TD-BIZ-23 (D-3155): the Get help page, a business page, carries the header link; the
    /// business help page links the form by [`GET_HELP_ROUTE`].
    #[test]
    fn td_biz_23_business_pages_link_get_help() {
        let page = get_help_page("t", GetHelpScreen::Ask).expect("page");
        let link = format!("<a href=\"{GET_HELP_ROUTE}\" data-get-help>Get help</a>");
        assert!(page.contains(&link), "{page}");
        let site = include_str!("../../site/templates/business_help.html");
        assert!(site.contains(&format!("href=\"{GET_HELP_ROUTE}\"")));
        let form = help_form_page(HelpFormScreen::Sent).expect("page");
        assert!(!form.contains("data-get-help"));
    }

    /// TD-PRIV-16, TD-CON-17 (D-4293): every help-form answer but the refusal links the sign-in;
    /// the opt-out screen confirms on screen; the refusal says what to do.
    #[test]
    fn td_priv_16_help_form_screens() {
        let link = format!("href=\"{HOMEOWNER_SIGNIN_ROUTE}\"");
        for screen in [
            HelpFormScreen::Sent,
            HelpFormScreen::OptedOut,
            HelpFormScreen::NeedsNumber,
        ] {
            assert!(
                help_form_page(screen).expect("page").contains(&link),
                "{screen:?}"
            );
        }
        assert!(
            help_form_page(HelpFormScreen::OptedOut)
                .expect("page")
                .contains("START")
        );
        let refused = help_form_page(HelpFormScreen::TooMany).expect("page");
        assert!(refused.contains("Too many messages. Try again later, or email help@"));
        assert!(!refused.contains(&link));
    }

    #[test]
    fn an_ipv6_client_counts_by_its_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().expect("ip");
        let b: IpAddr = "2001:db8:1:2:bbbb::9".parse().expect("ip");
        assert_eq!(help_form_client(a), help_form_client(b));
        let v4: IpAddr = "192.0.2.7".parse().expect("ip");
        assert_eq!(help_form_client(v4), v4);
    }
}
