//! The business action pages an email link opens and the quote form's checks (T148; BIZ-028,
//! BIZ-032, BIZ-034, BIZ-038, PICK-006, PICK-039, PICK-040, FEE-043, A11Y-023, A11Y-024, D-1956,
//! D-1957).
//!
//! Every page works without JavaScript and says what it will do before its one button acts
//! (SEC-026, A11Y-023); the Confirm window shows as an end time, never a countdown. The quote form
//! shows the introduction fee by job size a quote would carry and its basis before submit, from
//! `payments::api` (`quotes` keeps no fee code, D-491), requires every PICK-040 quote fact,
//! pre-fills all but the earliest visit from the business's last answers for that job type, and
//! offers the optional "minutes this quote took" (OPS-047). A text quote's read-back (PICK-006)
//! is the same form with the price the AI read. "Ask about a charge" enqueues
//! [`HELP_REQUEST_KIND`] with IDs only (T151 opens the help request, OPS-036). The job-card page
//! calls [`sample_pick_on_open`] when a person opens a real job card: the first time for an owner
//! or "Confirm and billing" person it gives the "Send a sample pick" step (BIZ-042, D-462; T149).
//! T199: [`open_quote_form`], the quote form's `GET`, reads the link and stamps the notice's
//! first opening once (SEC-026, OPS-037). T155: [`largest_job_warning`] and
//! [`quote_form_page_warned`] warn before a quote above the business's largest-job answer is sent
//! (BIZ-049, D-393); the send goes through only with the form's `over_largest` field equal to the
//! warned amount in cents, so a raised price is warned again.
//! T209: [`auto_confirm_box`] decides whether the quote form shows the unticked "Auto-confirm and
//! pay if picked" box (PICK-048: Stripe's written answer filed, a saved card, the owner or a
//! "Confirm and billing" member) and [`quote_form_page_full`] shows it with its authorization;
//! [`set_auto_confirm`] stores a tick only after a fresh Verify code (SEC-038) through
//! `quotes::api`, and an untick at any time before the pick (PICK-056).

use askama::Template;
use askhava_businesses::api::{Acting, LargestJob, offer_sample_pick};
use askhava_core::api::{NewTask, brand, enqueue};
use askhava_messaging::api::{notice_log, record_first_open};
use askhava_payments::api::{
    BusinessId, Fee, FeeError, FeeQuote, Period as FeePeriod, PeriodJob, Tier, fee_for,
    period_fee_for, saved_card_of,
};
use askhava_quotes::api::{
    AUTO_CONFIRM_VERSION, AutoConfirmError as QuotesAutoError, AutoConfirmTick, DayPart, Parts,
    Period, Permit, Price, QuoteFacts, Spanish, TripFee, Untick, auto_confirm_authorization,
    auto_confirm_of, stripe_answer_filed, tick_auto_confirm, untick_auto_confirm,
};
use jiff::Timestamp;
use jiff::civil::Date;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::action_links::{
    ActionKind, ActionLinkError, OPEN_IN_BROWSER_LINE, action_link_business,
};
use crate::business_auth::{BusinessPrincipal, Need};
use crate::business_portal::{SensitiveChange, StepUpError, require_step_up};
use crate::confirm_pay::{FEE_FREE_LINE, money};
use crate::email_check::{MailApp, sample_pick_page};
use crate::session::PortalsError;

/// The `core.task` kind "Ask about a charge" raises; T151's handler opens the help request.
pub const HELP_REQUEST_KIND: &str = "help_request_opened";

/// BIZ-032, D-349: the reply line every charge question gets, with the brand's display name.
#[must_use]
pub fn charge_reply_line() -> String {
    let name = brand().display_name();
    format!("The {name} owner gets charge questions right away and usually replies within 2 days.")
}

/// BIZ-028: the longest "Busy for <n> days" setting.
pub const BUSY_DAYS_MAX: u32 = 60;

/// The most minutes the optional "minutes this quote took" field takes (OPS-047).
pub const QUOTE_MINUTES_MAX: i32 = 600;

fn whole(cents: u64) -> String {
    money(cents).trim_end_matches(".00").to_owned()
}

/// BIZ-028: the days of a "Busy for <n> days" form, 1 to 60; anything else is `None`.
#[must_use]
pub fn busy_days(raw: &str) -> Option<u32> {
    raw.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=BUSY_DAYS_MAX).contains(n))
}

/// BIZ-028: the portal line "Busy until <date>" for `days` busy days from `today` (Arizona).
#[must_use]
pub fn busy_until_line(today: Date, days: u32) -> String {
    let until = today.saturating_add(jiff::Span::new().days(i64::from(days)));
    format!("Busy until {}.", until.strftime("%A, %B %-d"))
}

/// BIZ-028: what to do when the "Busy for <n> days" value is not a whole number from 1 to 60.
pub const BUSY_DAYS_ERROR: &str = "Enter a number of days from 1 to 60.";

/// SEC-026, A11Y-023: what the page for `kind` says it will do, before its button. SEC-004: a
/// Confirm link opened with no live session asks for a Verify code; a `signed_in` person goes
/// straight to the review step, so their page promises no code.
#[must_use]
pub const fn does_line(kind: ActionKind, signed_in: bool) -> &'static str {
    match kind {
        ActionKind::Quote => "Send your quote for this job. The homeowner sees it after you send.",
        ActionKind::Confirm if signed_in => "You'll see the job and the fee before you confirm.",
        ActionKind::Confirm => {
            "We'll send you a code. Then you'll see the job and the fee before you confirm."
        }
        ActionKind::Hold => {
            "You get 2 more business hours to confirm this job. You can do this once per job."
        }
        ActionKind::Away => {
            "While you're away, the time to confirm a new pick starts when you're back."
        }
        ActionKind::Pause => "We'll stop sending new job cards until you tap Resume.",
        ActionKind::Resume => "We'll start sending you new job cards again.",
        ActionKind::Busy => "We won't send new job cards for the number of days you choose.",
        ActionKind::LinkPhone => {
            "Add this phone to your business so you can call homeowners from it."
        }
        ActionKind::Approve => "Let this phone call homeowners for your business.",
        ActionKind::AskCharge => {
            "Send us your question about this charge. You'll get the charge details right away."
        }
    }
}

/// The facts an action page shows (A11Y-023): the job, the Confirm window's end time, whether
/// the person is signed in, a refused Busy value and the form's CSRF token.
#[derive(Debug, Clone, Copy)]
pub struct ActionFacts<'a> {
    /// The action.
    pub kind: ActionKind,
    /// The job number, when the action is about one job.
    pub job_number: &'a str,
    /// The end time (Arizona), never a countdown, when a window applies.
    pub ends_at: Option<&'a str>,
    /// Whether the person has a live business session (SEC-004: no code step then).
    pub signed_in: bool,
    /// The "days" value of a refused Busy submit, kept and shown with its error (A11Y-030);
    /// `None` at first render.
    pub days: Option<&'a str>,
    /// The form's CSRF token.
    pub csrf: &'a str,
}

#[derive(Template)]
#[template(path = "business_actions.html")]
struct ActionsView<'a> {
    action: Option<ActionView<'a>>,
    form: Option<&'a QuoteFormFacts>,
    warning: Option<&'a LargestWarning>,
    auto_box: Option<&'a AutoConfirmBox>,
    csrf: &'a str,
}

struct ActionView<'a> {
    title: String,
    does: &'static str,
    ends_at: Option<&'a str>,
    busy: bool,
    max_days: u32,
    days: &'a str,
    days_error: Option<&'static str>,
}

/// BIZ-038, SEC-026, A11Y-023: the page an action link opens, with one button that acts.
///
/// # Errors
/// The template's render error.
pub fn action_page(facts: &ActionFacts<'_>) -> Result<String, askama::Error> {
    let busy = facts.kind == ActionKind::Busy;
    let days = facts.days.filter(|_| busy);
    let action = ActionView {
        title: facts.kind.link_text(facts.job_number),
        does: does_line(facts.kind, facts.signed_in),
        ends_at: facts.ends_at,
        busy,
        max_days: BUSY_DAYS_MAX,
        days: days.unwrap_or_default(),
        days_error: days
            .filter(|d| busy_days(d).is_none())
            .map(|_| BUSY_DAYS_ERROR),
    };
    ActionsView {
        action: Some(action),
        form: None,
        warning: None,
        auto_box: None,
        csrf: facts.csrf,
    }
    .render()
}

/// REC-004, REC-010, D-1407, D-1957: a per-visit price has no visit schedule on the form, and
/// `payments` takes a per-visit price only as `Weekly` with weekly visits or converted to a
/// monthly amount by the caller, so it gets no fee line (`NeedsPeriod`) rather than a guessed
/// weekly one.
const fn fee_period(period: Period) -> Result<FeePeriod, FeeError> {
    match period {
        Period::Visit => Err(FeeError::NeedsPeriod),
        Period::Week => Ok(FeePeriod::Weekly),
        Period::Month => Ok(FeePeriod::Monthly),
        Period::Quarter => Ok(FeePeriod::Quarterly),
        Period::Year => Ok(FeePeriod::Yearly),
    }
}

fn fee_line(fee: Fee, how: &str) -> String {
    match fee {
        Fee::FeeFree { .. } => FEE_FREE_LINE.to_owned(),
        Fee::Charged { basis: None, tier } => format!(
            "If picked, this quote's fee is {} (the set fee for a diagnostic visit in this category).",
            whole(tier.cents())
        ),
        Fee::Charged {
            basis: Some(b),
            tier,
        } => {
            let capped = Tier::for_basis(b) != tier;
            let cap = if capped {
                format!(", capped at {}", whole(tier.cents()))
            } else {
                String::new()
            };
            format!(
                "If picked, this quote's fee is {} ({how} {}{cap}).",
                whole(tier.cents()),
                whole(b)
            )
        }
    }
}

/// PICK-039, FEE-043, D-185, D-262, D-441: the introduction fee by job size each part of `price`
/// would carry if picked, with its basis, computed by `payments::api` for `job`.
///
/// # Errors
/// [`FeeError`] when the quote can't carry a fee yet (an hourly quote with no hours outside a
/// diagnostic-visit category, a missing period, a per-visit price); the form shows the field's
/// error instead.
pub fn fee_lines(price: &Price, job: PeriodJob) -> Result<Vec<String>, FeeError> {
    let one = |quote, how: &str| fee_for(quote, job.job).map(|f| vec![fee_line(f, how)]);
    match price {
        Price::Firm(c) => one(FeeQuote::Fixed(*c), "price"),
        Price::Range { low, high, .. } => one(
            FeeQuote::Range {
                low: *low,
                high: *high,
            },
            "range midpoint",
        ),
        Price::Hourly { rate, hours } => one(
            FeeQuote::Hourly {
                rate: *rate,
                hours: *hours,
            },
            "your rate times the hours, midpoint",
        ),
        Price::VisitFee(v) => one(FeeQuote::DiagnosticVisit { visit_fee: *v }, "price"),
        Price::Monthly { cents, period, .. } => {
            let quote = FeeQuote::Fixed(*cents);
            let fee = period_fee_for(quote, Some(fee_period(*period)?), job)?;
            Ok(vec![fee_line(fee, "4 months of the price")])
        }
        Price::Tiers(tiers) => tiers
            .iter()
            .map(|(name, c)| {
                let fee = fee_for(FeeQuote::Fixed(*c), job.job)?;
                Ok(fee_line(fee, "price").replacen(
                    "If picked,",
                    &format!("If the homeowner picks {name},"),
                    1,
                ))
            })
            .collect(),
    }
}

/// The quote fact fields as the form posts them (PICK-040); empty when not chosen.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct FactsForm {
    /// `counts`, `does_not_count` or `none`.
    #[serde(default)]
    pub trip_fee: String,
    /// `included`, `extra` or `on_site`.
    #[serde(default)]
    pub parts: String,
    /// `included`, `not_included` or `not_needed`.
    #[serde(default)]
    pub permit: String,
    /// `yes` or `no`.
    #[serde(default)]
    pub may_change: String,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    pub visit_date: String,
    /// `morning`, `afternoon`, `evening` or `tonight`.
    #[serde(default)]
    pub visit_part: String,
    /// `phone`, `on_site`, `both` or `no`.
    #[serde(default)]
    pub spanish: String,
    /// The optional "minutes this quote took".
    #[serde(default)]
    pub minutes: String,
}

/// A quote fact field left empty or out of range; the form names it (A11Y-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactProblem {
    /// Trip fee counts toward the repair.
    TripFee,
    /// Parts and materials.
    Parts,
    /// Permit.
    Permit,
    /// Price may change.
    MayChange,
    /// The earliest visit's date.
    VisitDate,
    /// The earliest visit's part of the day.
    VisitPart,
    /// Speaks Spanish.
    Spanish,
    /// Minutes not a whole number from 1 to [`QUOTE_MINUTES_MAX`].
    Minutes,
}

impl FactProblem {
    /// The form element the error summary links to.
    #[must_use]
    pub const fn field(self) -> &'static str {
        match self {
            Self::TripFee => "trip_fee",
            Self::Parts => "parts",
            Self::Permit => "permit",
            Self::MayChange => "may_change",
            Self::VisitDate => "visit_date",
            Self::VisitPart => "visit_part",
            Self::Spanish => "spanish",
            Self::Minutes => "minutes",
        }
    }

    /// A11Y-030: what to do, as the summary and the field say it.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::TripFee => "Choose whether the trip fee counts toward the repair.",
            Self::Parts => "Choose how parts and materials are charged.",
            Self::Permit => "Choose whether a permit is included, not included or not needed.",
            Self::MayChange => "Choose whether the price may change after you see the job.",
            Self::VisitDate => "Enter the date of your earliest visit, like 2026-11-12.",
            Self::VisitPart => "Choose the part of the day for your earliest visit.",
            Self::Spanish => "Choose whether you speak Spanish with homeowners.",
            Self::Minutes => "Enter the minutes as a number from 1 to 600, or leave it blank.",
        }
    }
}

fn trip_fee(code: &str) -> Option<TripFee> {
    match code {
        "counts" => Some(TripFee::Counts),
        "does_not_count" => Some(TripFee::DoesNotCount),
        "none" => Some(TripFee::None),
        _ => None,
    }
}

fn parts(code: &str) -> Option<Parts> {
    match code {
        "included" => Some(Parts::Included),
        "extra" => Some(Parts::Extra),
        "on_site" => Some(Parts::OnSite),
        _ => None,
    }
}

fn permit(code: &str) -> Option<Permit> {
    match code {
        "included" => Some(Permit::Included),
        "not_included" => Some(Permit::NotIncluded),
        "not_needed" => Some(Permit::NotNeeded),
        _ => None,
    }
}

fn spanish(code: &str) -> Option<Spanish> {
    match code {
        "phone" => Some(Spanish::Phone),
        "on_site" => Some(Spanish::OnSite),
        "both" => Some(Spanish::Both),
        "no" => Some(Spanish::No),
        _ => None,
    }
}

fn yes_no(code: &str) -> Option<bool> {
    match code {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// OPS-047, PICK-040: the optional "minutes this quote took": `None` when left empty.
///
/// # Errors
/// [`FactProblem::Minutes`] for anything but a whole number from 1 to [`QUOTE_MINUTES_MAX`].
pub fn quote_minutes(raw: &str) -> Result<Option<i32>, FactProblem> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<i32>()
        .ok()
        .filter(|m| (1..=QUOTE_MINUTES_MAX).contains(m))
        .map(Some)
        .ok_or(FactProblem::Minutes)
}

/// PICK-040: the quote facts and minutes from `form`; every fact is required and the earliest
/// visit needs a date and a part of the day.
///
/// # Errors
/// Every [`FactProblem`] found, in the form's order; the form keeps every value (A11Y-030).
pub fn parse_facts(form: &FactsForm) -> Result<(QuoteFacts, Option<i32>), Vec<FactProblem>> {
    let date = form.visit_date.trim().parse::<Date>().ok();
    let found = (
        trip_fee(&form.trip_fee),
        parts(&form.parts),
        permit(&form.permit),
        yes_no(&form.may_change),
        date,
        DayPart::from_code(&form.visit_part),
        spanish(&form.spanish),
        quote_minutes(&form.minutes),
    );
    match found {
        (Some(t), Some(p), Some(m), Some(c), Some(d), Some(v), Some(s), Ok(min)) => Ok((
            QuoteFacts {
                trip_fee: t,
                parts: p,
                permit: m,
                may_change: c,
                earliest_visit: (d, v),
                spanish: s,
            },
            min,
        )),
        (t, p, m, c, d, v, s, min) => {
            let missing = [
                (t.is_none(), FactProblem::TripFee),
                (p.is_none(), FactProblem::Parts),
                (m.is_none(), FactProblem::Permit),
                (c.is_none(), FactProblem::MayChange),
                (d.is_none(), FactProblem::VisitDate),
                (v.is_none(), FactProblem::VisitPart),
                (s.is_none(), FactProblem::Spanish),
                (min.is_err(), FactProblem::Minutes),
            ];
            Err(missing
                .into_iter()
                .filter(|(x, _)| *x)
                .map(|(_, p)| p)
                .collect())
        }
    }
}

fn trip_code(t: TripFee) -> &'static str {
    match t {
        TripFee::Counts => "counts",
        TripFee::DoesNotCount => "does_not_count",
        TripFee::None => "none",
    }
}

/// PICK-040, A11Y-012 (WCAG 3.3.7): the form's starting values from the business's `last` facts
/// for the same job type, every fact but the earliest visit; empty for a first quote of a job
/// type (`None`). "Same as last time" posts these values as they stand.
#[must_use]
pub fn prefill(last: Option<&QuoteFacts>) -> FactsForm {
    let Some(f) = last else {
        return FactsForm::default();
    };
    let pick = |v: &str| v.to_owned();
    FactsForm {
        trip_fee: pick(trip_code(f.trip_fee)),
        parts: pick(match f.parts {
            Parts::Included => "included",
            Parts::Extra => "extra",
            Parts::OnSite => "on_site",
        }),
        permit: pick(match f.permit {
            Permit::Included => "included",
            Permit::NotIncluded => "not_included",
            Permit::NotNeeded => "not_needed",
        }),
        may_change: pick(if f.may_change { "yes" } else { "no" }),
        spanish: pick(match f.spanish {
            Spanish::Phone => "phone",
            Spanish::OnSite => "on_site",
            Spanish::Both => "both",
            Spanish::No => "no",
        }),
        ..FactsForm::default()
    }
}

/// What the quote form shows (PICK-006, PICK-039, PICK-040, CARD-019).
#[derive(Debug, Clone)]
pub struct QuoteFormFacts {
    /// The job number.
    pub job_number: String,
    /// How many businesses the card went to (CARD-019).
    pub sent: u32,
    /// How many quotes the job has now.
    pub quoted: u32,
    /// The fee lines from [`fee_lines`], shown before submit (PICK-039).
    pub fee_lines: Vec<String>,
    /// The fact values: [`prefill`] at first, the posted values after an error.
    pub values: FactsForm,
    /// Whether the values came from the last quote, so "Same as last time" is offered.
    pub same_as_last: bool,
    /// PICK-006: the price the AI read from a text quote, shown back to confirm.
    pub read_back: Option<String>,
    /// The fields with a problem after a failed submit.
    pub problems: Vec<FactProblem>,
}

impl QuoteFormFacts {
    /// Whether the failed submit found `problem`.
    #[must_use]
    pub fn has(&self, problem: &FactProblem) -> bool {
        self.problems.contains(problem)
    }

    /// A11Y-030: the summary's message for the form element `field`, shown by that field too.
    #[must_use]
    pub fn error_for(&self, field: &str) -> Option<&'static str> {
        self.problems
            .iter()
            .find(|p| p.field() == field)
            .map(|p| p.message())
    }

    /// The "Sent to <n> businesses. <q> quotes so far." line (CARD-019).
    #[must_use]
    pub fn status_line(&self) -> String {
        let plural = |n: u32, one: &str, many: &str| {
            if n == 1 {
                format!("{n} {one}")
            } else {
                format!("{n} {many}")
            }
        };
        format!(
            "Sent to {}. {} so far.",
            plural(self.sent, "business", "businesses"),
            plural(self.quoted, "quote", "quotes")
        )
    }
}

/// PICK-006, PICK-039, PICK-040, BIZ-038: the quote form, with the fee lines before the button
/// and every quote fact as a required choice; the "Open in browser" line is the email's.
///
/// # Errors
/// The template's render error.
pub fn quote_form_page(facts: &QuoteFormFacts, csrf: &str) -> Result<String, askama::Error> {
    quote_form_page_warned(facts, csrf, None)
}

/// BIZ-049, D-393: [`quote_form_page`] with the largest-job `warning` from
/// [`largest_job_warning`] above the button, focused, and the `over_largest` field holding the
/// warned amount in cents, so the next send of that same amount goes through.
///
/// # Errors
/// The template's render error.
pub fn quote_form_page_warned(
    facts: &QuoteFormFacts,
    csrf: &str,
    warning: Option<&LargestWarning>,
) -> Result<String, askama::Error> {
    quote_form_page_full(facts, csrf, warning, None)
}

/// PICK-048, BIZ-049: [`quote_form_page_warned`] with the auto-confirm box from
/// [`auto_confirm_box`] after the fee lines, when it is shown: unticked unless the quote's box is
/// ticked now, with its own authorization and the line saying a tick asks for a new code.
///
/// # Errors
/// The template's render error.
pub fn quote_form_page_full(
    facts: &QuoteFormFacts,
    csrf: &str,
    warning: Option<&LargestWarning>,
    auto_box: Option<&AutoConfirmBox>,
) -> Result<String, askama::Error> {
    ActionsView {
        action: None,
        form: Some(facts),
        warning,
        auto_box,
        csrf,
    }
    .render()
}

/// BIZ-049: the most a quote asks, in cents, for the largest-job check: the firm price, the top of
/// a range, the highest tier, the visit fee, or the hourly rate times the most hours. A monthly
/// price and an hourly rate with no hours have no job size, so `None`.
#[must_use]
pub fn quote_top_cents(price: &Price) -> Option<u64> {
    match price {
        Price::Firm(c) | Price::VisitFee(c) => Some(*c),
        Price::Range { high, .. } => Some(*high),
        Price::Tiers(tiers) => tiers.iter().map(|(_, c)| *c).max(),
        Price::Hourly { rate, hours } => hours.map(|(_, most)| rate * u64::from(most)),
        Price::Monthly { .. } => None,
    }
}

/// BIZ-049, D-393: the largest-job warning: its words and the quote amount it warned about, in
/// cents, which the form posts back as `over_largest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargestWarning {
    /// The warning line shown above the send button.
    pub text: String,
    /// The quote's top amount ([`quote_top_cents`]) the warning was shown for.
    pub cents: u64,
}

/// BIZ-049, D-393, D-2510: the warning shown before a quote above the business's `largest` job is
/// sent; none for no limit, at or under it, or once the business confirmed this same amount
/// (`confirmed`, the posted `over_largest` cents). A confirmation of another amount (the price
/// changed after the warning) warns again.
#[must_use]
pub fn largest_job_warning(
    price: &Price,
    largest: LargestJob,
    confirmed: Option<u64>,
) -> Option<LargestWarning> {
    let top = quote_top_cents(price)?;
    (confirmed != Some(top) && largest.below(top)).then(|| LargestWarning {
        text: format!(
            "This quote is over {}, which you told us is the largest job you'd quote. You can send \
it anyway or change the price.",
            largest.line()
        ),
        cents: top,
    })
}

/// SEC-026, OPS-037, BIZ-038, D-347, D-491: the quote form's `GET`. It only reads the Quote link
/// `token` for `job` (opening never acts or uses it up, so a mail scanner's fetch changes no
/// quote and no expiry), then stamps the first opening of the notice `notice` the link came in
/// through `messaging::api::record_first_open` (once; a later opening changes nothing, and the
/// notice log calls it "may be a mail scanner"). No pixel or redirect is involved: this is the
/// app's own page request. A `notice` that is not the link's business's notice for `job` is
/// left unstamped, so a link can never mark another business's or another job's notice opened.
/// The caller renders [`quote_form_page`] on `Ok`.
///
/// # Errors
/// [`ActionLinkError::Ended`] for a link that would not work (nothing is stamped), or
/// [`ActionLinkError::Store`] when a read or the stamp fails.
pub async fn open_quote_form(
    db: &mut PgConnection,
    (job, token): (Uuid, &str),
    notice: Option<Uuid>,
    now: Timestamp,
) -> Result<(), ActionLinkError> {
    let business = action_link_business(db, (ActionKind::Quote, job), token, now).await?;
    let Some(id) = notice else {
        return Ok(());
    };
    let log = notice_log(db, business, job)
        .await
        .map_err(|_| PortalsError::Database)?;
    if log.iter().any(|entry| entry.id == id) {
        record_first_open(db, id, now)
            .await
            .map_err(|_| PortalsError::Database)?;
    }
    Ok(())
}

/// A11Y-023, A11Y-024, BIZ-038, SEC-025: the plain-text part of an email with one action link:
/// the link text names the action and the job, the full askhava.com link, then the "Open in
/// browser" line.
#[must_use]
pub fn action_email_text(kind: ActionKind, job_number: &str, link: &str) -> String {
    format!(
        "{}: {link}\n\n{OPEN_IN_BROWSER_LINE}\n",
        kind.link_text(job_number)
    )
}

/// The charge a question is about (BIZ-032, FEE-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargeAsked {
    /// The business asking.
    pub business: Uuid,
    /// The charge.
    pub charge: Uuid,
    /// The charge's job.
    pub job: Uuid,
}

/// BIZ-032, OPS-036, D-492: "Ask about a charge": enqueues [`HELP_REQUEST_KIND`] with the
/// business, charge and job IDs only, in the caller's transaction. The business's own words are
/// not carried: no task stores them yet, so the help request has the charge but not the
/// question (a plan gap, D-1958).
///
/// # Errors
/// [`PortalsError::Database`].
pub async fn ask_about_charge(
    db: &mut PgConnection,
    asked: ChargeAsked,
    now: Timestamp,
) -> Result<(), PortalsError> {
    let payload = json!({
        "business": asked.business,
        "charge": asked.charge,
        "job": asked.job,
    });
    enqueue(db, &NewTask::new(HELP_REQUEST_KIND, payload, now))
        .await
        .map(|_| ())
        .map_err(|_| PortalsError::Database)
}

/// BIZ-032, FEE-031, D-348: the answer a charge question gets at once: the job number, amount,
/// tier, the "Why was I charged?" link and [`charge_reply_line`].
#[must_use]
pub fn charge_answer(job_number: &str, tier: Tier, why_link: &str) -> String {
    let size = match tier {
        Tier::Small => "small job",
        Tier::Medium => "medium job",
        Tier::Large => "large job",
    };
    format!(
        "Your latest charge: {} for job {job_number}, the introduction fee for a {size}. Why was I charged? {why_link} {}",
        whole(tier.cents()),
        charge_reply_line()
    )
}

/// BIZ-042, D-462, TD-BIZ-28: the job-card page route calls this each time `principal` opens a
/// real job card (never a sample, never during sign-up). The first time an owner or "Confirm and
/// billing" person does, it renders the "Send a sample pick" step page for `app` first, then
/// records the offer and returns the page, so a failed render records nothing; every other time,
/// and for a "Cards and quotes"-only member, `None`. It never blocks the card: the step offers
/// "Skip for now".
///
/// # Errors
/// [`PortalsError::Database`] when the offer cannot be recorded or the page fails to render.
pub async fn sample_pick_on_open(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    app: MailApp,
    (csrf, now): (&str, Timestamp),
) -> Result<Option<String>, PortalsError> {
    let by = Acting {
        business: principal.business,
        member: principal.member,
    };
    let page = sample_pick_page(app, false, csrf).map_err(|_| PortalsError::Database)?;
    let offered = offer_sample_pick(db, by, now)
        .await
        .map_err(|_| PortalsError::Database)?;
    Ok(offered.then_some(page))
}

/// Where the auto-confirm tick is posted; served by T306, not by `server` (D-2128).
pub const AUTO_CONFIRM_ROUTE: &str = "/b/quote/{job}/auto-confirm";

/// The quote form's "Auto-confirm and pay if picked" box (PICK-048).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoConfirmBox {
    /// The box's own authorization with the fee shown (`quotes::api::auto_confirm_authorization`).
    pub authorization: String,
    /// The quote's box is ticked now.
    pub ticked: bool,
}

/// PICK-048, FEE-014, D-401, D-491: the box for `principal`'s quote on `job` with the fee
/// `fee_cents` (the one fee the form shows; `None` for none or more than one), or `None` when it
/// is not shown: the person is not the owner or a "Confirm and billing" member, Stripe's written
/// answer is not filed (`quotes::api::stripe_answer_filed`, D-2783), the business
/// has no saved card, or the quote has no single fee. This route decides, never `quotes`.
///
/// # Errors
/// [`PortalsError::Database`].
pub async fn auto_confirm_box(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (job, fee_cents): (Uuid, Option<u64>),
) -> Result<Option<AutoConfirmBox>, PortalsError> {
    let Some(fee) = fee_cents.filter(|c| *c > 0) else {
        return Ok(None);
    };
    if box_card(db, principal).await?.is_none() {
        return Ok(None);
    }
    let ticked = auto_confirm_of(db, (job, principal.business))
        .await
        .map_err(|_| PortalsError::Database)?
        .is_some_and(|t| t.fee_cents == fee);
    Ok(Some(AutoConfirmBox {
        authorization: auto_confirm_authorization(fee),
        ticked,
    }))
}

/// The saved card the box would be ticked with, when the box may show for `principal`.
async fn box_card(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
) -> Result<Option<String>, PortalsError> {
    if !principal.may(principal.business, Need::ConfirmAndBilling) {
        return Ok(None);
    }
    let filed = stripe_answer_filed(db)
        .await
        .map_err(|_| PortalsError::Database)?;
    if !filed {
        return Ok(None);
    }
    saved_card_of(db, BusinessId(principal.business))
        .await
        .map_err(|_| PortalsError::Database)
}

/// Why a tick or untick was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AutoConfirmBoxError {
    /// Not the owner or a "Confirm and billing" member, no filed answer, no saved card, a $0 fee
    /// or no live quote: the box is not offered.
    #[error("the auto-confirm box is not offered")]
    NotOffered,
    /// No Verify code in the last 5 minutes (SEC-038); the refusal is logged.
    #[error("ticking the box needs a fresh code")]
    NeedsCode,
    /// A statement failed.
    #[error("the auto-confirm change could not be stored")]
    Database,
}

/// PICK-048, PICK-056, SEC-038, SEC-048, D-480: ticks (`tick`) or unticks the box on
/// `principal`'s quote for `job` with the fee shown, `fee_cents`. A tick needs the box offered
/// ([`auto_confirm_box`]'s conditions) and then `require_step_up` (a Verify code in the last 5
/// minutes, else refused and logged), and stores the member, the fee, the authorization version
/// and the saved card; an untick needs only the role and may be made any time before the pick.
///
/// # Errors
/// [`AutoConfirmBoxError`].
pub async fn set_auto_confirm(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (job, fee_cents, tick): (Uuid, u64, bool),
    now: Timestamp,
) -> Result<(), AutoConfirmBoxError> {
    if !principal.may(principal.business, Need::ConfirmAndBilling) {
        return Err(AutoConfirmBoxError::NotOffered);
    }
    let quote = (job, principal.business);
    if !tick {
        return untick_auto_confirm(db, quote, Untick::Business, now)
            .await
            .map(|_| ())
            .map_err(|_| AutoConfirmBoxError::Database);
    }
    let card = box_card(db, principal)
        .await
        .map_err(|_| AutoConfirmBoxError::Database)?
        .filter(|_| fee_cents > 0)
        .ok_or(AutoConfirmBoxError::NotOffered)?;
    let change = (SensitiveChange::AutoConfirm, AUTO_CONFIRM_ROUTE);
    require_step_up(db, principal, change, now)
        .await
        .map_err(|e| match e {
            StepUpError::NeedsCode => AutoConfirmBoxError::NeedsCode,
            StepUpError::Database => AutoConfirmBoxError::Database,
        })?;
    let stored = AutoConfirmTick {
        member: principal.member,
        fee_cents,
        authorization_version: AUTO_CONFIRM_VERSION,
        payment_method: card,
    };
    tick_auto_confirm(db, quote, &stored, now)
        .await
        .map_err(|e| match e {
            QuotesAutoError::NoQuote | QuotesAutoError::BadField => AutoConfirmBoxError::NotOffered,
            QuotesAutoError::Database => AutoConfirmBoxError::Database,
        })
}
