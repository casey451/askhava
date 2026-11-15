//! The business's Confirm review step: what the page shows and what a press answers (PICK-021,
//! FEE-014, FEE-015, FEE-016, FEE-045, FEE-047, CON-029; T224, D-1479 to D-1481).
//!
//! [`confirm_page`] renders `templates/confirm_pay.html` from the server's [`ConfirmFacts`]: the
//! job number, the introduction fee by job size with its tier and how its basis was set, the
//! refund line, the terms version, "Go back", "Can't take it" (PICK-050) and one button, "Confirm
//! and pay $<fee>", or "Confirm" alone when no fee is due (a free first win, a repeat customer,
//! or a fee-free job type, which also says "No introduction fee for this job.", FEE-045). A paid
//! Confirm carries the Stripe Elements card field and the unticked "Save this card for next
//! time" box with the FEE-015 authorization text beside it (FEE-014, FEE-015); a $0 Confirm
//! carries the box with "Next time it's one tap." (D-465). Only this page loads Stripe.js, from
//! `js.stripe.com` (TD-SEC-04); `static/confirm_pay.ts` mounts Elements, posts the `pm_…` with the
//! form's one-time press ID and shows each [`PressReply`] at once; the form's `data-paid="no"` on a
//! $0 Confirm makes it post no card unless the save box is ticked. A job paused by the homeowner
//! (CON-029) gets the refusal instead of the form: no button, so no charge and no release.
//! The fee comes from the server (`payments::api::fee_at_pick`), never from the browser.
//!
//! [`press_reply`] turns `payments::api::confirm_and_pay`'s [`ChargeOutcome`] into what the page
//! shows: a decline at once, with a new press ID so the next press is the next attempt key
//! (`pick-<id>-try2`, FEE-016); a bank approval as the in-page 3-D Secure step, with the
//! PaymentIntent's client secret the route passes in; an over-limit end with [`PORTAL_LINK`]
//! (FEE-041); success as
//! "waiting for Stripe's confirmation", because details are released only from the verified
//! webhook (`payments::api::stripe_webhook_routes`, PICK-022), never from this answer.
//! The route that reads the facts, opens the pick's one PaymentIntent on the first load
//! (`open_intent`, FEE-047) and charges is wired once its readers exist (D-1480).
//!
//! [`may_confirm`] scopes that route to the signed-in principal: only the owner or a "Confirm and
//! billing" member of the picked business may see the page or press (PICK-033, SEC-019, D-492,
//! T084); the route calls it before reading any fact, and anyone else gets the denial.
//!
//! T440 (PICK-021, PICK-053, D-2332, D-3627): [`confirm_page_with_tier`] adds, for a pick of a
//! tiers quote, the package the homeowner chose and its price above the fee line, whose basis
//! line then reads that price; [`confirm_page_with_code`] is it with no package.

use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use askama::Template;
use askhava_adapters::api::{
    AdapterError, Charge, CodeChannel, IdempotencyKey, PaymentIntent, PaymentsAdapter,
    VerifyAdapter,
};
use askhava_businesses::api::{ConfirmTerms, confirm_assent_line, members, record_confirm_terms};
use askhava_intake::api::{Admission, job_homeowner, jobs_of_contact, paused_by_homeowner};
use askhava_jobs::api::job_number;
use askhava_messaging::api::{NoticeKind, notice_log, record_first_open};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, ChargeError, ChargeOutcome, ConfirmError, Fee, FeeJob, FeeMode, FeeQuote,
    FreeWinError, IntentStripe, PeriodJob, PickNow, PickedFee, Press, RecurringCap, ReleaseSink,
    Service, StoredPickFee, StripeRefund, StripeWebhook, Tier, VisitTier, Waiver, confirm_and_pay,
    end_intent_unless_charging, ensure_customer, fee_at_pick, fee_terms_in_force,
    may_confirm as fee_terms_accepted, open_intent, pick_fee,
};
use askhava_picks::api::{Backup, ChosenTier, cant_take_job, end_for_fee_terms};
use axum::Router;
use axum::extract::{FromRef, Path, State};
use axum::http::header::{ACCEPT, VARY};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use axum_extra::extract::CookieJar;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::action_links::{ActionKind, fresh_code_needed};
use crate::business_auth::{
    BusinessPrincipal, CodeAddress, CodeTo, Need, SIGNIN_ROUTE, business_principal,
};
use crate::business_portal::inbox_shared;
use crate::confirm_waiver::{
    ConfirmFee, ConfirmWaiverError, ConfirmedPick, decide_at_confirm, release_no_fee,
};
use crate::limits::{admit_code_send, admit_code_try};
use crate::security::{
    CsrfField, CsrfForm, StripeConfirmCsp, WebSecurity, no_store, refuse_cross_site,
};
use crate::session::{PortalsError, csrf_token};

/// The one refund line every Confirm page shows (PICK-021).
pub const REFUND_LINE: &str = "Refund if you can't reach them: 2 call attempts at least 3 hours \
apart, inside their window, within 72 hours. They can be on different days.";

/// The FEE-045 line of a fee-free job type's Confirm page.
pub const FEE_FREE_LINE: &str = "No introduction fee for this job.";

/// Why no fee is due on a $0 Confirm (PICK-021, FEE-010, FEE-013, FEE-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoFee {
    /// The business's free first win (FEE-010).
    FreeFirstWin,
    /// The same homeowner hired this business before (FEE-013).
    RepeatCustomer,
}

/// What the server knows about one pick when its Confirm page loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmFacts {
    /// The pick.
    pub pick: Uuid,
    /// The job's request number.
    pub job_number: i64,
    /// The fee fixed at the pick (FEE-005).
    pub fee: Fee,
    /// A waiver already decided for this business and job, if any (FEE-010, FEE-013).
    pub waived: Option<NoFee>,
    /// The business terms version the business accepted (FEE-007, FEE-015).
    pub terms_version: i32,
    /// The job is paused by the homeowner (CON-029).
    pub paused: bool,
    /// The form's one-time press ID (FEE-016).
    pub press: Uuid,
    /// The form's CSRF token (SEC-042).
    pub csrf: String,
    /// Stripe's publishable key for this mode (`pk_test_…` or `pk_live_…`; public by design).
    pub stripe_key: String,
}

#[derive(Template)]
#[template(path = "confirm_pay.html")]
struct ConfirmView<'a> {
    pick: Uuid,
    job_number: i64,
    paused: bool,
    fee_dollars: u64,
    basis: String,
    no_fee_line: &'static str,
    button: String,
    paid: bool,
    authorization: &'static str,
    refund: &'static str,
    terms_version: i32,
    press: Uuid,
    csrf: &'a str,
    stripe_key: &'a str,
    code: Option<CodeStep>,
    package: Option<String>,
    card_off: Option<&'static str>,
}

/// The page's name for the tier (FEE-001).
const fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Small => "small job",
        Tier::Medium => "medium job",
        Tier::Large => "large job",
    }
}

/// `$1,234.56` for `cents`; the one formatter of the portals' money pages (the receipt's "Why
/// was I charged?" page uses it too).
pub(crate) fn money(cents: u64) -> String {
    let dollars = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, digit) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("${grouped}.{:02}", cents % 100)
}

/// How the basis was set, in the page's words (PICK-021, FEE-003, FEE-004).
fn basis_line(basis: Option<u64>, tier: &str) -> String {
    basis.map_or_else(
        || format!("The price gets set on site, so the fee goes by this trade's usual visit fee: the {tier}."),
        |cents| format!("Your {} quote makes this a {tier}.", money(cents)),
    )
}

/// The page for `facts` (PICK-021, FEE-014, FEE-015, FEE-045, CON-029); see the module docs.
///
/// # Errors
/// The template's render error.
pub fn confirm_page(facts: &ConfirmFacts) -> Result<String, askama::Error> {
    confirm_page_with_code(facts, None)
}

/// The page for `facts` with SEC-027's code step when `code` is given (the route gives it when
/// [`confirm_code_needed`]); PICK-050's "Can't take it" stays on every page with a form.
///
/// # Errors
/// The template's render error.
pub fn confirm_page_with_code(
    facts: &ConfirmFacts,
    code: Option<CodeStep>,
) -> Result<String, askama::Error> {
    confirm_page_with_tier(facts, code, None)
}

/// [`confirm_page_with_code`] for a pick of a tiers quote: `package` is the package the homeowner
/// chose (`picks.pick`'s stored package), shown as "The homeowner chose: <name>, $<price>." beside the
/// fee and its basis (PICK-021, PICK-053, D-2332); `None` shows no package line.
///
/// # Errors
/// The template's render error.
pub fn confirm_page_with_tier(
    facts: &ConfirmFacts,
    code: Option<CodeStep>,
    package: Option<&ChosenTier>,
) -> Result<String, askama::Error> {
    render_page(facts, (code, package), None)
}

/// The page, with `card_off` (the served route's [`CARD_OFF_LINE`] or [`FEE_TERMS_LINE`], D-3813)
/// shown in place of the card section and both scripts dropped.
fn render_page(
    facts: &ConfirmFacts,
    (code, package): (Option<CodeStep>, Option<&ChosenTier>),
    card_off: Option<&'static str>,
) -> Result<String, askama::Error> {
    let (tier, no_fee_line) = match (facts.fee, facts.waived) {
        (Fee::FeeFree { .. }, _) => ("", FEE_FREE_LINE),
        (Fee::Charged { tier, .. }, None) => (tier_name(tier), ""),
        (Fee::Charged { tier, .. }, Some(_)) => (tier_name(tier), "No fee is due for this job."),
    };
    let paid = no_fee_line.is_empty();
    let fee_dollars = if paid { facts.fee.cents() / 100 } else { 0 };
    let button = if paid {
        format!("Confirm and pay ${fee_dollars}")
    } else {
        "Confirm".to_owned()
    };
    ConfirmView {
        pick: facts.pick,
        job_number: facts.job_number,
        paused: facts.paused,
        fee_dollars,
        basis: basis_line(facts.fee.basis(), tier),
        no_fee_line,
        button,
        paid,
        authorization: AUTHORIZATION_TEXT,
        refund: REFUND_LINE,
        terms_version: facts.terms_version,
        press: facts.press,
        csrf: &facts.csrf,
        stripe_key: &facts.stripe_key,
        code,
        package: package.map(|p| format!("The homeowner chose: {}, {}.", p.name, money(p.cents))),
        card_off,
    }
    .render()
}

/// Where the business changes its monthly fee limit: its portal (FEE-041).
pub const PORTAL_LINK: &str = "/b";

/// What one press answers to the page script (FEE-016, FEE-018, FEE-020, FEE-041, PICK-022).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PressReply {
    /// `waiting`, `declined`, `approve`, `try_later`, `ended` or `paid`.
    pub state: &'static str,
    /// The words shown in the page's live region.
    pub message: &'static str,
    /// The press ID the next press must use: new after a decline, so it is the next attempt.
    pub next_press: Option<Uuid>,
    /// The PaymentIntent's client secret for Stripe's in-page 3-D Secure step, only on `approve`
    /// (FEE-014); it is Stripe's browser-side handle, not an API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// The portal link shown with an over-limit answer (FEE-041).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<&'static str>,
}

/// The state, words and whether a new press ID goes back, for `outcome` (FEE-016, FEE-041).
fn words(outcome: &ChargeOutcome) -> (&'static str, &'static str, bool) {
    match outcome {
        ChargeOutcome::Succeeded(_) | ChargeOutcome::Pending => (
            "waiting",
            "Payment sent. You'll get the homeowner's details as soon as Stripe confirms it.",
            false,
        ),
        ChargeOutcome::Declined(_) => (
            "declined",
            "Your card was declined. Nothing was charged. Try again or use another card.",
            true,
        ),
        ChargeOutcome::NeedsApproval(_) => (
            "approve",
            "Your bank wants to check this payment. Follow the steps in the window it opens.",
            false,
        ),
        ChargeOutcome::StripeDown | ChargeOutcome::Paused => (
            "try_later",
            "We can't take payments right now. Nothing was charged. Try again in a few minutes.",
            true,
        ),
        ChargeOutcome::OverLimit => (
            "ended",
            "This would go over your monthly fee limit, so nothing was charged and the pick has ended. You can change the limit in your portal.",
            false,
        ),
        ChargeOutcome::MovedOn => (
            "ended",
            "This pick can't be confirmed anymore. Nothing was charged.",
            false,
        ),
        ChargeOutcome::AlreadyPaid => ("paid", "This pick is already paid.", false),
    }
}

/// The page's answer to `outcome`; `fresh` is a new press ID for the next attempt (FEE-016);
/// `client_secret` is the pick's PaymentIntent's, passed on only for a bank approval (FEE-014);
/// an over-limit answer carries [`PORTAL_LINK`] (FEE-041).
#[must_use]
pub fn press_reply(
    outcome: &ChargeOutcome,
    fresh: Uuid,
    client_secret: Option<String>,
) -> PressReply {
    let (state, message, next) = words(outcome);
    PressReply {
        state,
        message,
        next_press: next.then_some(fresh),
        client_secret: client_secret.filter(|_| matches!(outcome, ChargeOutcome::NeedsApproval(_))),
        link: matches!(outcome, ChargeOutcome::OverLimit).then_some(PORTAL_LINK),
    }
}

/// Whether `principal` may open or press the Confirm of a pick of `picked_business`: a signed-in
/// owner or "Confirm and billing" member of that business only; no session, another business or
/// a "Cards and quotes" member alone may not (PICK-033, SEC-019, D-492).
#[must_use]
pub fn may_confirm(principal: Option<&BusinessPrincipal>, picked_business: Uuid) -> bool {
    principal.is_some_and(|p| p.may(picked_business, Need::ConfirmAndBilling))
}

/// SEC-027's code step on the Confirm page: the line about the last send and the code field's
/// error (A11Y-011, A11Y-012). `Default` is the step before any code was asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodeStep {
    /// What the last "Text me a code" did ([`CodeSent::line`]).
    pub line: Option<&'static str>,
    /// Why the code field failed ([`CodeCheck::error`]); the page then says "Error: " first.
    pub error: Option<&'static str>,
}

/// SEC-027, D-2410: whether `member`'s Confirm needs a fresh Verify code: with "This inbox is
/// shared" on, even inside a live session (`signed_in`), by T199's `fresh_code_needed`.
///
/// # Errors
/// [`PortalsError::Database`].
pub async fn confirm_code_needed(
    db: &mut PgConnection,
    member: Uuid,
    signed_in: bool,
) -> Result<bool, PortalsError> {
    let shared = inbox_shared(db, member)
        .await
        .map_err(|_| PortalsError::Database)?;
    Ok(fresh_code_needed(ActionKind::Confirm, signed_in, shared))
}

/// OPS-037, D-2412: the Confirm page's `GET` stamps the first opening of the pick notice that
/// went to `member`'s own address for `job` (the latest one), once, through
/// `messaging::api::record_first_open`; a later opening, another business's notice or a person
/// with no pick notice changes nothing. True when this opening was the first.
///
/// # Errors
/// [`PortalsError::Database`].
pub async fn open_confirm_page(
    db: &mut PgConnection,
    (business, job, member): (Uuid, Uuid, Uuid),
    now: Timestamp,
) -> Result<bool, PortalsError> {
    let people = members(db, business)
        .await
        .map_err(|_| PortalsError::Database)?;
    let Some(person) = people.into_iter().find(|m| m.id == member) else {
        return Ok(false);
    };
    let log = notice_log(db, business, job)
        .await
        .map_err(|_| PortalsError::Database)?;
    let pick = log
        .iter()
        .rev()
        .find(|e| e.kind == NoticeKind::Pick && e.mailbox == person.mailbox);
    match pick {
        Some(entry) => record_first_open(db, entry.id, now)
            .await
            .map_err(|_| PortalsError::Database),
        None => Ok(false),
    }
}

/// The Confirm code's parts (SEC-027, TXT-038): the same Verify, [`CodeAddress`] and key secret
/// [`crate::business_auth::DeviceCodes`] is built from; never the outbox.
pub struct ConfirmCodes<V> {
    verify: V,
    to: Box<dyn CodeAddress>,
    key: SecretString,
}

impl<V: VerifyAdapter + Send + Sync> ConfirmCodes<V> {
    /// Codes sent and checked by `verify`, to the phone `to` names, keyed with `key`.
    pub fn new(verify: V, to: impl CodeAddress, key: SecretString) -> Self {
        Self {
            verify,
            to: Box::new(to),
            key,
        }
    }

    /// The person's own linked phone that takes texts; none for an email-only person.
    async fn phone(&self, pool: &PgPool, person: Uuid) -> Result<Option<String>, PortalsError> {
        let mut conn = pool.acquire().await.map_err(|_| PortalsError::Database)?;
        Ok(match self.to.code_to(&mut conn, person).await? {
            CodeTo::Phone(phone) => Some(phone.expose_secret().to_owned()),
            CodeTo::Email(_) => None,
        })
    }
}

/// What one "Text me a code" did (SEC-014, SEC-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeSent {
    /// Verify texted a code to the person's own phone.
    Sent,
    /// The person has no phone that takes texts, so nothing was sent.
    NoPhone,
    /// A SEC-014 send limit refused it; nothing was sent.
    Limited,
    /// Verify did not take it.
    Failed,
}

impl CodeSent {
    /// The line the page shows after the send.
    #[must_use]
    pub const fn line(self) -> &'static str {
        match self {
            Self::Sent => "We texted a code to your phone. Type it below.",
            Self::NoPhone => {
                "We can't text you a code, so you can't confirm yet. In your portal, link a phone that gets texts."
            }
            Self::Limited => {
                "You've asked for too many codes. Try again in an hour, or tomorrow if you see this again."
            }
            Self::Failed => "We couldn't send the code. Press \"Text me a code\" again.",
        }
    }
}

/// SEC-027, SEC-014, TXT-038: texts `person` a fresh Confirm code for `pick` through Verify, to
/// their own phone only, after `admit_code_send` (per IP, 5 a day per person, the global
/// `threshold`) admits it. The phone is looked up first, so an email-only person (`NoPhone`)
/// spends no send. No connection is held across Verify (PT-04).
///
/// # Errors
/// [`PortalsError::Database`] when a limit or the phone lookup fails.
pub async fn send_confirm_code<V: VerifyAdapter + Send + Sync>(
    codes: &ConfirmCodes<V>,
    pool: &PgPool,
    (ip, person, pick): (IpAddr, Uuid, Uuid),
    (now, threshold): (Timestamp, i32),
) -> Result<CodeSent, PortalsError> {
    let Some(phone) = codes.phone(pool, person).await? else {
        return Ok(CodeSent::NoPhone);
    };
    let admitted = admit_code_send(pool, (ip, Some(person)), (now, threshold)).await;
    if matches!(
        admitted.map_err(|_| PortalsError::Database)?,
        Admission::Refused(_)
    ) {
        return Ok(CodeSent::Limited);
    }
    let parts = [pick.to_string(), person.to_string(), now.to_string()];
    let key = IdempotencyKey::derive(
        &codes.key,
        "verify.confirm_code",
        &parts.each_ref().map(String::as_str),
    );
    let sent = codes.verify.send_code(&key, &phone, CodeChannel::Sms).await;
    Ok(if sent.is_ok() {
        CodeSent::Sent
    } else {
        CodeSent::Failed
    })
}

/// What one Confirm code try answered (SEC-014, SEC-020, SEC-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCheck {
    /// Verify accepted it: the press may go on.
    Right,
    /// Not 6 digits: no try was spent.
    Missing,
    /// Verify said no.
    Wrong,
    /// Verify ended the code (its 5 checks, its 10 minutes), or the person has no phone.
    Ended,
    /// Verify was not reached or gave no answer (a network failure): the code may still be good.
    Unchecked,
    /// A SEC-020 try limit refused it; Verify was not asked.
    Limited,
}

impl CodeCheck {
    /// The code field's error, none when it was right.
    #[must_use]
    pub const fn error(self) -> Option<&'static str> {
        match self {
            Self::Right => None,
            Self::Missing => Some("Type the 6-digit code from our text."),
            Self::Wrong => Some("That code isn't right. Check the text and type it again."),
            Self::Ended => Some("That code has run out. Press \"Text me a code\" for a new one."),
            Self::Unchecked => Some("We couldn't check the code. Type it again, then confirm."),
            Self::Limited => Some(
                "Too many wrong codes. Wait an hour, then press \"Text me a code\" for a new one.",
            ),
        }
    }
}

/// SEC-027, SEC-020: checks `code` against Verify for `person`'s own phone, after
/// `admit_code_try` (per IP and per `business`, 20 an hour each) admits the try. No
/// `portals.code_try` row is written (a Confirm code has no sign-in link) and no connection is
/// held across Verify. Verify refusing the code (a 4xx answer: its 5-check cap 60202, an expired
/// or unknown code) ends it; no answer, a 5xx or an unreadable answer is [`CodeCheck::Unchecked`].
///
/// # Errors
/// [`PortalsError::Database`] when a limit or the phone lookup fails.
pub async fn check_confirm_code<V: VerifyAdapter + Send + Sync>(
    codes: &ConfirmCodes<V>,
    pool: &PgPool,
    (ip, person, business): (IpAddr, Uuid, Uuid),
    code: &str,
    now: Timestamp,
) -> Result<CodeCheck, PortalsError> {
    let code = code.trim();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(CodeCheck::Missing);
    }
    let admitted = admit_code_try(pool, (ip, Some(business)), now).await;
    if matches!(
        admitted.map_err(|_| PortalsError::Database)?,
        Admission::Refused(_)
    ) {
        return Ok(CodeCheck::Limited);
    }
    let Some(phone) = codes.phone(pool, person).await? else {
        return Ok(CodeCheck::Ended);
    };
    Ok(match codes.verify.check_code(&phone, code).await {
        Ok(true) => CodeCheck::Right,
        Ok(false) => CodeCheck::Wrong,
        Err(AdapterError::Vendor { status, .. }) if status < 500 => CodeCheck::Ended,
        Err(_) => CodeCheck::Unchecked,
    })
}

/// The served Confirm route (D-1480, D-3813, T261): the page, its `Accept: application/json`
/// state and the press.
pub const CONFIRM_ROUTE: &str = "/b/picks/{pick}/confirm";

/// D-3813: shown in place of the card form when the route has no settings part (dev and CI
/// without the keys, the door tests); a press then charges nothing.
pub const CARD_OFF_LINE: &str = "Card payments can't be taken right now.";

/// D-1871, D-3722: shown in place of the card form when the fee mode in force differs from the
/// one the pick's fee was fixed under; no PaymentIntent is opened.
pub const FEE_TERMS_LINE: &str = "The fee terms changed after the homeowner picked you, so you \
can't confirm this one. Nothing was charged.";

/// CON-029: the press reply for a job the homeowner paused, said only when no charge attempt
/// can still succeed.
const PAUSED_LINE: &str =
    "The homeowner has paused this job, so it can't be confirmed now. Nothing was charged.";

/// D-1871, D-3722: the press reply when the fee terms changed: the pick ended with no fee.
const FEE_TERMS_ENDED: &str = "The fee terms changed after the homeowner picked you, so you \
can't confirm this one. Nothing was charged and this pick is now closed.";

/// D-3813: the press reply without the settings part.
const CARD_OFF_REPLY: &str =
    "Card payments can't be taken right now. Nothing was charged. Try again later.";

/// SEC-027, D-3717: a shared inbox needs a fresh code, which this page can't send yet.
const CODE_LINE: &str = "This inbox is shared, so you need a code sent to your own phone to \
confirm. This page can't send that code. Nothing was charged.";

/// FEE-007, FEE-015: the business has not accepted the card authorization the terms need.
const AUTHORIZATION_LINE: &str = "Accept the updated card authorization in your portal, then \
confirm. Nothing was charged.";

/// FEE-014: a paid press that carried no card.
const NO_CARD_LINE: &str =
    "Enter a card, then press \"Confirm and pay\" again. Nothing was charged.";

/// FEE-016: the attempt's state when nothing is charging and the pick still waits.
const NOT_CHARGED_LINE: &str = "Nothing was charged. Check your card and try again.";

/// FEE-012, PICK-022: a $0 Confirm, released through the production sink.
const CONFIRMED_LINE: &str =
    "Confirmed. No fee is due. The homeowner's details are in your portal.";

/// The served route's settings part (D-3813, D-3717): Stripe's publishable key, the launch
/// instant and `ASKHAVA_ID_SECRET` (the idempotency and entity-key secret), which `main` builds.
#[derive(Clone)]
pub struct ConfirmSettings {
    /// `pk_test_…` or `pk_live_…`, public by design.
    pub publishable_key: String,
    /// The launch instant of FEE-010's launch window.
    pub launch: Timestamp,
    /// `ASKHAVA_ID_SECRET`.
    pub secret: SecretString,
}

/// What [`ApprovalSecret::confirm_keeping_secret`] answers: the intent after the confirm and,
/// only while the bank must approve, its client secret.
pub type SecretAnswer<'a> = Pin<
    Box<
        dyn Future<Output = Result<(PaymentIntent, Option<SecretString>), AdapterError>>
            + Send
            + 'a,
    >,
>;

/// FEE-014, FEE-016, D-2076, D-3717, D-3719: one attempt's confirm with the client secret Stripe
/// answers it with, which `IntentStripe::confirm_intent` drops; `server` posts it (only `server`
/// may name the Stripe client). The secret comes from that confirm's own answer, never from
/// replaying the pick's PaymentIntent create, whose key Stripe forgets after about 24 hours (a
/// weekend's Confirm window), so a bank approval never makes a second PaymentIntent.
pub trait ApprovalSecret: Send + Sync {
    /// Confirms `intent` on-session with `payment_method` under one attempt's `key`.
    fn confirm_keeping_secret<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        intent: &'a str,
        payment_method: &'a str,
    ) -> SecretAnswer<'a>;
}

/// FEE-014, D-3719: the Stripe client one press charges through: `stripe` for every call but the
/// confirm, which goes through `approval`, keeping its client secret for the page.
struct Catching<'a, S> {
    stripe: &'a S,
    approval: &'a dyn ApprovalSecret,
    secret: Mutex<Option<SecretString>>,
}

impl<S: PaymentsAdapter + Sync> PaymentsAdapter for Catching<'_, S> {
    async fn create_customer(
        &self,
        key: &IdempotencyKey,
        business_id: &str,
    ) -> Result<String, AdapterError> {
        self.stripe.create_customer(key, business_id).await
    }

    async fn create_payment_intent(
        &self,
        key: &IdempotencyKey,
        charge: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        self.stripe.create_payment_intent(key, charge).await
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        self.stripe.fetch_payment_intent(id).await
    }

    async fn create_refund(
        &self,
        key: &IdempotencyKey,
        intent: &str,
    ) -> Result<String, AdapterError> {
        self.stripe.create_refund(key, intent).await
    }

    async fn send_refund(
        &self,
        key: &IdempotencyKey,
        intent: &str,
    ) -> Result<StripeRefund, AdapterError> {
        self.stripe.send_refund(key, intent).await
    }

    async fn read_refund(&self, id: &str) -> Result<StripeRefund, AdapterError> {
        self.stripe.read_refund(id).await
    }
}

impl<S: IntentStripe + Sync> IntentStripe for Catching<'_, S> {
    async fn create_intent(
        &self,
        key: &IdempotencyKey,
        customer: &str,
        amount_cents: u32,
        pick_id: &str,
    ) -> Result<String, AdapterError> {
        let made = self
            .stripe
            .create_intent(key, customer, amount_cents, pick_id);
        made.await
    }

    async fn confirm_intent(
        &self,
        key: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        let confirm = self
            .approval
            .confirm_keeping_secret(key, intent, payment_method);
        let (intent, secret) = confirm.await?;
        *self.secret.lock().unwrap_or_else(PoisonError::into_inner) = secret;
        Ok(intent)
    }
}

/// The served route's parts: the Stripe webhook's hooks (pool, Stripe client, release sink,
/// clock; D-3744's pattern) and the optional settings part (D-3813).
pub struct ConfirmRoute<S, R> {
    /// The Stripe webhook's state.
    pub hooks: Arc<StripeWebhook<S, R>>,
    /// `main`'s settings part; none serves the page with [`CARD_OFF_LINE`] and charges nothing.
    pub settings: Option<ConfirmSettings>,
    /// Each attempt's confirm, keeping a bank approval's client secret from its own answer.
    pub approval: Arc<dyn ApprovalSecret>,
}

struct Served<S, R> {
    route: Arc<ConfirmRoute<S, R>>,
    security: WebSecurity,
}

impl<S, R> Clone for Served<S, R> {
    fn clone(&self) -> Self {
        Self {
            route: Arc::clone(&self.route),
            security: self.security.clone(),
        }
    }
}

impl<S, R> FromRef<Served<S, R>> for WebSecurity {
    fn from_ref(served: &Served<S, R>) -> Self {
        served.security.clone()
    }
}

/// PICK-021, FEE-016, SEC-012, SEC-019, SEC-034, SEC-042, D-3813: `GET` and `POST`
/// [`CONFIRM_ROUTE`] behind `no-store` and the cross-site refusal; only the page carries
/// [`StripeConfirmCsp`].
pub fn confirm_routes<S, R>(route: ConfirmRoute<S, R>) -> Router
where
    S: IntentStripe + Send + Sync + 'static,
    R: ReleaseSink,
{
    let security = WebSecurity::new(route.hooks.pool.clone(), Arc::clone(&route.hooks.clock));
    let state = Served {
        route: Arc::new(route),
        security: security.clone(),
    };
    Router::new()
        .route(CONFIRM_ROUTE, get(load::<S, R>).post(press::<S, R>))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// SEC-004: no business person: 401 with the sign-in link, no detail.
fn signed_out() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Sign in</title></head><body><main>\
         <h1>Sign in to your business account</h1><p><a href=\"{SIGNIN_ROUTE}\">Sign in</a> to \
         see this page.</p></main></body></html>"
    );
    (StatusCode::UNAUTHORIZED, Html(page)).into_response()
}

/// SEC-019: the one not-found answer for another business's, a "Cards and quotes" member's or an
/// unknown pick.
fn not_found() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Page not found</title></head><body><main>\
         <h1>We can't find that page</h1><p>The link may be old, or it may be for a different \
         business account. <a href=\"{PORTAL_LINK}\">Go to your business home</a>.</p>\
         </main></body></html>"
    );
    (StatusCode::NOT_FOUND, Html(page)).into_response()
}

/// A failed read or write: a bare 500, logged once without values (R5).
fn failed() -> Response {
    tracing::warn!("confirm route failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// The pick as its own business sees it: the person and the fee fixed at the pick.
struct Mine {
    who: BusinessPrincipal,
    pick: Uuid,
    fee: StoredPickFee,
}

/// PICK-033, SEC-004, SEC-019: the signed-in owner or "Confirm and billing" member of `pick`'s
/// business, else the answer to send.
async fn mine(
    pool: &PgPool,
    jar: &CookieJar,
    pick: &str,
    now: Timestamp,
) -> Result<Mine, Box<Response>> {
    let mut db = pool.acquire().await.map_err(|_| Box::new(failed()))?;
    let who = match business_principal(&mut db, jar, now).await {
        Ok(Some(who)) => who,
        Ok(None) => return Err(Box::new(signed_out())),
        Err(_) => return Err(Box::new(failed())),
    };
    let Ok(pick) = Uuid::try_parse(pick) else {
        return Err(Box::new(not_found()));
    };
    let fee = pick_fee(&mut db, pick)
        .await
        .map_err(|_| Box::new(failed()))?;
    match fee.filter(|f| may_confirm(Some(&who), f.business.0)) {
        Some(fee) => Ok(Mine { who, pick, fee }),
        None => Err(Box::new(not_found())),
    }
}

/// FEE-017, CON-029: in `tx`, the pick's job while it waits for Confirm (row locked) and whether
/// the homeowner paused it; `None` once it moved on.
async fn waiting(tx: &mut PgConnection, mine: &Mine) -> Result<Option<(Uuid, bool)>, ()> {
    let job = cant_take_job(tx, mine.pick, mine.fee.business.0).await;
    let Some(job) = job.map_err(|_| ())? else {
        return Ok(None);
    };
    let paused = paused_by_homeowner(tx, job).await.map_err(|_| ())?;
    Ok(Some((job, paused)))
}

/// A reply with no new press ID and no client secret.
const fn said(
    state: &'static str,
    message: &'static str,
    link: Option<&'static str>,
) -> PressReply {
    PressReply {
        state,
        message,
        next_press: None,
        client_secret: None,
        link,
    }
}

/// FEE-016, CON-029, D-3813: the attempt's state with no new attempt, in `tx`, which the caller
/// rolls back: `end_intent_unless_charging` locks the pick's PaymentIntent row and re-reads its
/// attempts, so "Nothing was charged" is said only while no attempt can still succeed.
async fn attempt_state(
    tx: &mut PgConnection,
    pick: Uuid,
    at: (Option<(Uuid, bool)>, Timestamp),
) -> Result<PressReply, ()> {
    let idle = end_intent_unless_charging(tx, pick, at.1).await;
    let fresh = Uuid::new_v4();
    Ok(match (idle.map_err(|_| ())?, at.0) {
        (false, _) => press_reply(&ChargeOutcome::Pending, fresh, None),
        (true, Some((_, true))) => said("ended", PAUSED_LINE, None),
        (true, Some((_, false))) => PressReply {
            next_press: Some(fresh),
            ..said("declined", NOT_CHARGED_LINE, None)
        },
        (true, None) => press_reply(&ChargeOutcome::MovedOn, fresh, None),
    })
}

/// Whether the request asks for the attempt's state as JSON (D-3813).
fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/json"))
}

/// `GET` [`CONFIRM_ROUTE`] (PICK-021, CON-029, FEE-047, OPS-037, D-3813): the page, or with
/// `Accept: application/json` the attempt's state, which opens nothing and stores nothing.
async fn load<S, R>(
    State(served): State<Served<S, R>>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(pick): Path<String>,
) -> Response
where
    S: IntentStripe + Send + Sync + 'static,
    R: ReleaseSink,
{
    let route = &served.route;
    let now = route.hooks.clock.now();
    let mine = match mine(&route.hooks.pool, &jar, &pick, now).await {
        Ok(mine) => mine,
        Err(answer) => return *answer,
    };
    let answer = if wants_json(&headers) {
        state_json(&route.hooks.pool, &mine, now).await
    } else {
        page(route, &mine, &jar, now).await
    };
    let mut response = answer.unwrap_or_else(|()| failed());
    response
        .headers_mut()
        .insert(VARY, HeaderValue::from_static("accept"));
    response
}

/// D-3813: the attempt's state in the press reply's shape, never a client secret; every write
/// of the read is rolled back.
async fn state_json(pool: &PgPool, mine: &Mine, now: Timestamp) -> Result<Response, ()> {
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let at = waiting(&mut tx, mine).await?;
    let reply = attempt_state(&mut tx, mine.pick, (at, now)).await?;
    tx.rollback().await.map_err(|_| ())?;
    Ok(Json(reply).into_response())
}

/// What the page shows beside the facts: the line in place of the card section and a waiver.
type Shown = (Option<&'static str>, Option<NoFee>);

/// The page for `mine` (PICK-021, CON-029, OPS-037): a waiting pick stamps the pick notice's
/// first opening; a paused one gets the refusal; one that moved on shows the attempt's state.
async fn page<S, R>(
    route: &ConfirmRoute<S, R>,
    mine: &Mine,
    jar: &CookieJar,
    now: Timestamp,
) -> Result<Response, ()>
where
    S: IntentStripe + Send + Sync,
    R: ReleaseSink,
{
    let pool = &route.hooks.pool;
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let at = waiting(&mut tx, mine).await?;
    let (shown, paused) = match at {
        Some((job, false)) => {
            let member = mine.who.member();
            let opened = open_confirm_page(&mut tx, (mine.fee.business.0, job, member), now);
            opened.await.map_err(|_| ())?;
            tx.commit().await.map_err(|_| ())?;
            (card_section(route, mine, now).await?, false)
        }
        Some((_, true)) => ((None, None), true),
        None => {
            let reply = attempt_state(&mut tx, mine.pick, (None, now)).await?;
            tx.rollback().await.map_err(|_| ())?;
            ((Some(reply.message), None), false)
        }
    };
    let mut db = pool.acquire().await.map_err(|_| ())?;
    let number = job_number(&mut db, mine.fee.job).await.map_err(|_| ())?;
    let key = route.settings.as_ref().map(|s| s.publishable_key.clone());
    let csrf = csrf_token(jar).ok_or(())?;
    served_page(
        mine,
        (number, paused),
        shown,
        (csrf, key.unwrap_or_default()),
    )
}

/// The tier a stored fee of `cents` is (FEE-001).
fn tier_of(cents: u64) -> Option<Tier> {
    [Tier::Small, Tier::Medium, Tier::Large]
        .into_iter()
        .find(|t| t.cents() == cents)
}

/// FEE-005, D-1871: the fee fixed at the pick, as the page shows it; no stored basis exists, so
/// the page shows none (D-3717).
fn shown_fee(fee: &StoredPickFee) -> Fee {
    match tier_of(fee.cents).filter(|_| fee.charged) {
        Some(tier) => Fee::Charged { basis: None, tier },
        None => Fee::FeeFree { basis: None },
    }
}

/// FEE-005, D-3717: the stored fee as the [`PickedFee`] `waive_at_confirm` reads (its amount
/// only), rebuilt through `fee_at_pick` with the stored tier as a flat edit.
fn picked_fee(fee: &StoredPickFee) -> Option<PickedFee> {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: !fee.charged,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let tier = tier_of(fee.cents);
    if fee.charged && tier.is_none() {
        return None;
    }
    fee_at_pick(FeeQuote::Fixed(fee.cents), None, job, FeeMode::Flat, tier).ok()
}

/// Renders the served page with the Confirm route's CSP marker (SEC-012, D-3813).
fn served_page(
    mine: &Mine,
    (job_number, paused): (i64, bool),
    (card_off, waived): Shown,
    (csrf, stripe_key): (String, String),
) -> Result<Response, ()> {
    let facts = ConfirmFacts {
        pick: mine.pick,
        job_number,
        fee: shown_fee(&mine.fee),
        waived,
        terms_version: mine.fee.terms_version,
        paused,
        press: Uuid::new_v4(),
        csrf,
        stripe_key,
    };
    let html = render_page(&facts, (None, None), card_off).map_err(|_| ())?;
    Ok((StripeConfirmCsp, Html(html)).into_response())
}

/// D-1871, D-3722, D-3813, FEE-047: the fee-terms line when the mode in force differs from the
/// pick's (no PaymentIntent), the card-off line without settings or when Stripe can't be
/// reached; else the $0 decision as it stands (rolled back) and, for a fee, the pick's one
/// PaymentIntent opened.
async fn card_section<S, R>(
    route: &ConfirmRoute<S, R>,
    mine: &Mine,
    now: Timestamp,
) -> Result<Shown, ()>
where
    S: IntentStripe + Send + Sync,
    R: ReleaseSink,
{
    let pool = &route.hooks.pool;
    let mut db = pool.acquire().await.map_err(|_| ())?;
    let terms = fee_terms_in_force(&mut db).await.map_err(|_| ())?;
    drop(db);
    if terms.mode() != mine.fee.mode {
        return Ok((Some(FEE_TERMS_LINE), None));
    }
    let Some(settings) = &route.settings else {
        return Ok((Some(CARD_OFF_LINE), None));
    };
    let waived = waiver_now(pool, mine, settings).await?;
    if waived.is_some() || !mine.fee.charged {
        return Ok((None, waived));
    }
    let press = press_of(mine, "", Uuid::nil()).ok_or(())?;
    let (stripe, secret) = (&route.hooks.stripe, &settings.secret);
    let opened = match ensure_customer(pool, stripe, secret, mine.fee.business).await {
        Ok(_) => open_intent(pool, stripe, secret, &press, now).await.is_ok(),
        Err(_) => false,
    };
    Ok((if opened { None } else { Some(CARD_OFF_LINE) }, None))
}

/// FEE-010, FEE-013: the waiver a press would record now, decided in a rolled-back transaction.
async fn waiver_now(
    pool: &PgPool,
    mine: &Mine,
    settings: &ConfirmSettings,
) -> Result<Option<NoFee>, ()> {
    let picked = picked_fee(&mine.fee).ok_or(())?;
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let jobs = her_jobs(&mut tx, mine.fee.job).await?;
    let decided = decide_at_confirm(
        &mut tx,
        &settings.secret,
        confirmed(mine, &picked, &jobs),
        Some(settings.launch),
    );
    let decided = decided.await;
    tx.rollback().await.map_err(|_| ())?;
    Ok(match decided {
        Ok(ConfirmFee::NoFee(Waiver::FreeFirstWin { .. })) => Some(NoFee::FreeFirstWin),
        Ok(ConfirmFee::NoFee(Waiver::Repeat(_))) => Some(NoFee::RepeatCustomer),
        Ok(_) | Err(ConfirmWaiverError::FreeWin(FreeWinError::Race)) => None,
        Err(_) => return Err(()),
    })
}

/// FEE-013, D-3719: the jobs of the homeowner who made the pick (her vault contact's, read
/// through `intake::api::job_homeowner` and `jobs_of_contact`), so `confirm_waiver` finds a
/// repeat customer; none when no open request opened the job. Known limit (D-3719): `intake`
/// lists only each request's first job, so a job opened by an ALSO reply (INT-032) is missed and
/// a repeat customer on it is charged the full fee.
async fn her_jobs(conn: &mut PgConnection, job: Uuid) -> Result<Vec<Uuid>, ()> {
    let Some(her) = job_homeowner(conn, job).await.map_err(|_| ())? else {
        return Ok(Vec::new());
    };
    jobs_of_contact(conn, her.contact).await.map_err(|_| ())
}

/// FEE-013: the pick for `confirm_waiver`: its own business and job, and `jobs`, the homeowner's
/// jobs from [`her_jobs`].
const fn confirmed<'a>(mine: &Mine, picked: &'a PickedFee, jobs: &'a [Uuid]) -> ConfirmedPick<'a> {
    ConfirmedPick {
        pick: mine.pick,
        business: mine.fee.business,
        job: mine.fee.job,
        picked,
        same_customer_jobs: jobs,
    }
}

/// FEE-005, FEE-016, D-1871: the press of exactly the stored fee, with `card` and press `id`.
fn press_of<'a>(mine: &Mine, card: &'a str, id: Uuid) -> Option<Press<'a>> {
    Some(Press {
        pick: mine.pick,
        business: mine.fee.business,
        job: mine.fee.job,
        amount_cents: u32::try_from(mine.fee.cents).ok()?,
        payment_method: card,
        press: id,
        emergency: false,
        now: PickNow::Payable,
    })
}

/// The Confirm form's fields (FEE-016, SEC-042); an unknown field is a 422 (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PressForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    press: String,
    #[serde(default)]
    payment_method: String,
    #[serde(default, rename = "code")]
    _code: String,
    #[serde(default, rename = "save_card")]
    _save_card: String,
}

impl CsrfField for PressForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST` [`CONFIRM_ROUTE`] (FEE-014, FEE-016, FEE-041, CON-029, SEC-027, TERMS-033, D-3813):
/// one press, answered as JSON; without the settings part nothing is charged.
async fn press<S, R>(
    State(served): State<Served<S, R>>,
    jar: CookieJar,
    Path(pick): Path<String>,
    CsrfForm(form): CsrfForm<PressForm>,
) -> Response
where
    S: IntentStripe + Send + Sync + 'static,
    R: ReleaseSink,
{
    let route = &served.route;
    let now = route.hooks.clock.now();
    let mine = match mine(&route.hooks.pool, &jar, &pick, now).await {
        Ok(mine) => mine,
        Err(answer) => return *answer,
    };
    let Ok(id) = Uuid::try_parse(&form.press) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let Some(settings) = &route.settings else {
        return Json(said("try_later", CARD_OFF_REPLY, None)).into_response();
    };
    let card = form.payment_method.as_str();
    match pressed(route, settings, &mine, (id, card), now).await {
        Ok(reply) => Json(reply).into_response(),
        Err(()) => failed(),
    }
}

/// The press after the route's checks, all in one transaction until the charge: the pick still
/// waits and is not paused (CON-029, else the attempt's state), no shared-inbox code is owed
/// (SEC-027), `payments::api::may_confirm` passes (FEE-007; a fee-mode switch ends the pick,
/// D-3722), the terms row is stored (TERMS-033), then the $0 decision or the charge.
async fn pressed<S, R>(
    route: &ConfirmRoute<S, R>,
    settings: &ConfirmSettings,
    mine: &Mine,
    (id, card): (Uuid, &str),
    now: Timestamp,
) -> Result<PressReply, ()>
where
    S: IntentStripe + Send + Sync,
    R: ReleaseSink,
{
    let mut tx = route.hooks.pool.begin().await.map_err(|_| ())?;
    let at = waiting(&mut tx, mine).await?;
    let Some((job, false)) = at else {
        let reply = attempt_state(&mut tx, mine.pick, (at, now)).await?;
        tx.rollback().await.map_err(|_| ())?;
        return Ok(reply);
    };
    if confirm_code_needed(&mut tx, mine.who.member(), true)
        .await
        .map_err(|_| ())?
    {
        return Ok(said("ended", CODE_LINE, None));
    }
    match fee_terms_accepted(&mut tx, mine.fee.business, mine.pick).await {
        Ok(_) => {}
        Err(ConfirmError::FeeModeChanged) => return fee_terms_changed(tx, mine, job, now).await,
        Err(ConfirmError::NewAuthorizationNeeded) => {
            return Ok(said("ended", AUTHORIZATION_LINE, Some(PORTAL_LINK)));
        }
        Err(_) => return Err(()),
    }
    let version = mine.fee.terms_version;
    let line = confirm_assent_line(version);
    let terms = ConfirmTerms {
        version,
        published_text: &line,
        language: "en",
    };
    let people = (mine.fee.business.0, mine.who.member());
    let stored = record_confirm_terms(&mut tx, people, mine.pick, terms, now).await;
    stored.map_err(|_| ())?;
    decide_and_pay(route, settings, mine, tx, (id, card, now)).await
}

/// D-1871, D-3722, PICK-050: the fee mode changed after the pick: nothing is charged and the pick
/// ends at once with no fee through `end_for_fee_terms`, which queues her PICK-046 text when no
/// backup takes it; a `Backup::Done` ends nothing and answers the attempt's state.
async fn fee_terms_changed(
    mut tx: sqlx::Transaction<'static, sqlx::Postgres>,
    mine: &Mine,
    job: Uuid,
    now: Timestamp,
) -> Result<PressReply, ()> {
    let by = (Actor::Business(mine.fee.business.0), now);
    let ended = end_for_fee_terms(&mut tx, (mine.pick, job), false, by).await;
    if matches!(ended.map_err(|_| ())?, Backup::Done) {
        let reply = attempt_state(&mut tx, mine.pick, (Some((job, false)), now)).await?;
        tx.rollback().await.map_err(|_| ())?;
        return Ok(reply);
    }
    tx.commit().await.map_err(|_| ())?;
    Ok(said("ended", FEE_TERMS_ENDED, None))
}

/// FEE-010 to FEE-013, FEE-045, PICK-022, D-3813: the $0 decision in the press's transaction;
/// after it commits, a $0 pick's one release through the production sink, else the charge.
async fn decide_and_pay<S, R>(
    route: &ConfirmRoute<S, R>,
    settings: &ConfirmSettings,
    mine: &Mine,
    mut tx: sqlx::Transaction<'static, sqlx::Postgres>,
    (id, card, now): (Uuid, &str, Timestamp),
) -> Result<PressReply, ()>
where
    S: IntentStripe + Send + Sync,
    R: ReleaseSink,
{
    let picked = picked_fee(&mine.fee).ok_or(())?;
    let jobs = her_jobs(&mut tx, mine.fee.job).await?;
    let pick = confirmed(mine, &picked, &jobs);
    let decided = decide_at_confirm(&mut tx, &settings.secret, pick, Some(settings.launch)).await;
    match decided {
        Ok(ConfirmFee::NoFee(_)) => {
            tx.commit().await.map_err(|_| ())?;
            let mut db = route.hooks.pool.acquire().await.map_err(|_| ())?;
            let released = release_no_fee(&mut db, &route.hooks.sink, pick).await;
            released.map_err(|_| ())?;
            Ok(said("confirmed", CONFIRMED_LINE, Some(PORTAL_LINK)))
        }
        Ok(ConfirmFee::Charge) => {
            tx.commit().await.map_err(|_| ())?;
            charge(route, settings, mine, (id, card, now)).await
        }
        Err(ConfirmWaiverError::FreeWin(FreeWinError::Race)) => Ok(press_reply(
            &ChargeOutcome::StripeDown,
            Uuid::new_v4(),
            None,
        )),
        Err(_) => Err(()),
    }
}

/// FEE-014, FEE-016, FEE-041, FEE-047: one charge attempt of the stored fee through
/// `confirm_and_pay`; a bank approval carries the client secret of that confirm's own answer
/// (D-3719), nothing else does.
async fn charge<S, R>(
    route: &ConfirmRoute<S, R>,
    settings: &ConfirmSettings,
    mine: &Mine,
    (id, card, now): (Uuid, &str, Timestamp),
) -> Result<PressReply, ()>
where
    S: IntentStripe + Send + Sync,
    R: ReleaseSink,
{
    if !card.starts_with("pm_") {
        return Ok(said("declined", NO_CARD_LINE, None));
    }
    let (pool, stripe, secret) = (&route.hooks.pool, &route.hooks.stripe, &settings.secret);
    let fresh = Uuid::new_v4();
    if ensure_customer(pool, stripe, secret, mine.fee.business)
        .await
        .is_err()
    {
        return Ok(press_reply(&ChargeOutcome::StripeDown, fresh, None));
    }
    let press = press_of(mine, card, id).ok_or(())?;
    let catching = Catching {
        stripe,
        approval: route.approval.as_ref(),
        secret: Mutex::new(None),
    };
    let outcome = match confirm_and_pay(pool, &catching, secret, &press, now).await {
        Ok(outcome) => outcome,
        Err(ChargeError::Stripe) => ChargeOutcome::StripeDown,
        Err(_) => return Err(()),
    };
    let kept = catching
        .secret
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    let client = kept.map(|kept| kept.expose_secret().to_owned());
    Ok(press_reply(&outcome, fresh, client))
}

#[cfg(test)]
mod tests {
    //! TD-FEE-25, TD-FEE-37, TD-FEE-40 and TD-PICK-08 page cases (fake data only).
    use askhava_payments::api::{ChargeOutcome, Fee, Tier};
    use sqlx::types::Uuid;

    use super::*;

    fn facts(fee: Fee) -> ConfirmFacts {
        ConfirmFacts {
            pick: Uuid::from_u128(7),
            job_number: 1042,
            fee,
            waived: None,
            terms_version: 3,
            paused: false,
            press: Uuid::from_u128(9),
            csrf: "fake-csrf".to_owned(),
            stripe_key: "pk_test_fake".to_owned(),
        }
    }

    fn medium() -> Fee {
        Fee::Charged {
            basis: Some(100_000),
            tier: Tier::Medium,
        }
    }

    fn page(facts: &ConfirmFacts) -> String {
        confirm_page(facts).expect("the page renders")
    }

    #[test]
    fn td_fee_25_pick_021_the_page_shows_everything_before_one_pay_button() {
        let html = page(&facts(medium()));
        assert!(html.contains("Confirm job 1042"));
        assert!(html.contains(
            "Introduction fee by job size: $75. Your $1,000.00 quote makes this a medium job."
        ));
        assert!(html.contains(&REFUND_LINE.replace('\'', "&#39;")));
        assert!(html.contains("You're agreeing to business terms version 3."));
        assert_eq!(html.matches("<button").count(), 1);
        assert!(html.contains(">Confirm and pay $75</button>"));
        assert!(html.contains("data-paid=\"yes\""));
        assert!(html.contains("Go back"));
        assert!(html.contains("Can't take it"));
        assert!(!html.contains(FEE_FREE_LINE));
    }

    #[test]
    fn td_fee_40_the_save_box_is_unticked_with_its_authorization_text() {
        let html = page(&facts(medium()));
        let input = html.split("id=\"save-card\"").nth(1).expect("a save box");
        let tag = input.split('>').next().expect("the tag");
        assert!(!tag.contains("checked"));
        assert!(html.contains("<label for=\"save-card\">Save this card for next time</label>"));
        let end = AUTHORIZATION_TEXT
            .find(" for introduction fees.")
            .expect("FEE-015's wording");
        let line = &AUTHORIZATION_TEXT[..end];
        assert!(
            line.starts_with("I authorize ") && line.ends_with(" to save this card with Stripe")
        );
        assert!(html.contains(line));
    }

    #[test]
    fn td_fee_40_a_free_first_win_says_one_tap_and_confirms_without_a_fee() {
        let mut free = facts(medium());
        free.waived = Some(NoFee::FreeFirstWin);
        let html = page(&free);
        assert!(html.contains(">Confirm</button>"));
        assert!(!html.contains(">Confirm and pay"));
        assert!(html.contains("Next time it's one tap."));
        assert!(
            html.contains("data-paid=\"no\""),
            "the script posts no card unless saving"
        );
        assert!(
            !html
                .split("id=\"save-card\"")
                .nth(1)
                .expect("a save box")
                .split('>')
                .next()
                .expect("the tag")
                .contains("checked")
        );
    }

    #[test]
    fn td_fee_37_a_fee_free_job_type_says_no_introduction_fee() {
        let html = page(&facts(Fee::FeeFree {
            basis: Some(40_000),
        }));
        assert!(html.contains(FEE_FREE_LINE));
        assert!(html.contains(">Confirm</button>"));
        assert!(!html.contains(">Confirm and pay"));
        assert!(!html.contains("$0"));
    }

    #[test]
    fn td_fee_37_a_paused_job_refuses_confirm_with_no_form() {
        let mut paused = facts(medium());
        paused.paused = true;
        let html = page(&paused);
        assert!(html.contains("paused this job, so it can't be confirmed now"));
        assert!(!html.contains("<form"));
        assert!(!html.contains("<button"));
        assert!(!html.contains("js.stripe.com"));
    }

    #[test]
    fn td_sec_04_stripe_js_loads_from_stripe_only_on_this_page() {
        let html = page(&facts(medium()));
        assert!(html.contains("<script src=\"https://js.stripe.com/v3/\"></script>"));
        assert!(html.contains("/static/portals/confirm_pay.js"));
    }

    #[test]
    fn a_diagnostic_visit_names_the_visit_fee_size() {
        let html = page(&facts(Fee::Charged {
            basis: None,
            tier: Tier::Small,
        }));
        assert!(html.contains("$25. The price gets set on site, so the fee goes by this trade&#39;s usual visit fee: the small job."));
        let large = page(&facts(Fee::Charged {
            basis: Some(250_000),
            tier: Tier::Large,
        }));
        assert!(large.contains("$199. Your $2,500.00 quote makes this a large job."));
    }

    #[test]
    fn td_fee_25_a_decline_shows_at_once_and_the_next_press_is_a_new_attempt() {
        let fresh = Uuid::from_u128(11);
        let declined = press_reply(&ChargeOutcome::Declined(None), fresh, None);
        assert_eq!(declined.state, "declined");
        assert!(declined.message.starts_with("Your card was declined."));
        assert_eq!(declined.next_press, Some(fresh));
        let approve = press_reply(&ChargeOutcome::NeedsApproval(None), fresh, None);
        assert_eq!((approve.state, approve.next_press), ("approve", None));
    }

    #[test]
    fn fee_014_a_bank_approval_carries_the_client_secret_and_nothing_else_does() {
        let fresh = Uuid::from_u128(11);
        let secret = || Some("pi_fake_secret_fake".to_owned());
        let approve = press_reply(&ChargeOutcome::NeedsApproval(None), fresh, secret());
        assert_eq!(
            approve.client_secret.as_deref(),
            Some("pi_fake_secret_fake")
        );
        let declined = press_reply(&ChargeOutcome::Declined(None), fresh, secret());
        assert_eq!(declined.client_secret, None);
        let waiting = press_reply(&ChargeOutcome::Pending, fresh, secret());
        assert_eq!(waiting.client_secret, None);
    }

    #[test]
    fn fee_041_an_over_limit_answer_carries_the_portal_link() {
        let fresh = Uuid::from_u128(11);
        let over = press_reply(&ChargeOutcome::OverLimit, fresh, None);
        assert_eq!(over.link, Some(PORTAL_LINK));
        assert!(over.message.contains("change the limit in your portal"));
        assert_eq!(press_reply(&ChargeOutcome::MovedOn, fresh, None).link, None);
    }

    #[test]
    fn pick_022_a_success_answer_releases_nothing_by_itself() {
        let fresh = Uuid::from_u128(11);
        for outcome in [
            ChargeOutcome::Succeeded("pi_fake".to_owned()),
            ChargeOutcome::Pending,
        ] {
            let reply = press_reply(&outcome, fresh, None);
            assert_eq!(reply.state, "waiting");
            assert_eq!(reply.next_press, None);
        }
        let states: Vec<&str> = [
            ChargeOutcome::StripeDown,
            ChargeOutcome::Paused,
            ChargeOutcome::OverLimit,
            ChargeOutcome::MovedOn,
            ChargeOutcome::AlreadyPaid,
        ]
        .iter()
        .map(|o| press_reply(o, fresh, None).state)
        .collect();
        assert_eq!(states, ["try_later", "try_later", "ended", "ended", "paid"]);
    }

    #[test]
    fn money_groups_thousands() {
        assert_eq!(money(123_456_789), "$1,234,567.89");
        assert_eq!(money(5), "$0.05");
    }

    fn person(business: u128, owner: bool, roles: (bool, bool)) -> BusinessPrincipal {
        BusinessPrincipal {
            member: Uuid::from_u128(0xb0),
            business: Uuid::from_u128(business),
            owner,
            roles,
        }
    }

    /// TD-SEC-01, PICK-033: only the owner or a "Confirm and billing" member of the picked
    /// business may Confirm; a "Cards and quotes" member, another business or no session may not.
    #[test]
    fn only_owner_or_confirm_and_billing_of_the_picked_business_confirms() {
        let picked = Uuid::from_u128(0xb1);
        assert!(may_confirm(Some(&person(0xb1, true, (true, true))), picked));
        assert!(may_confirm(
            Some(&person(0xb1, false, (false, true))),
            picked
        ));
        assert!(!may_confirm(
            Some(&person(0xb1, false, (true, false))),
            picked
        ));
        assert!(!may_confirm(
            Some(&person(0xb2, true, (true, true))),
            picked
        ));
        assert!(!may_confirm(None, picked));
    }
}
