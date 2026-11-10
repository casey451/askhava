//! The business's "Why was I charged?" page for one charge (FEE-031, FEE-053, D-225, D-491).
//!
//! [`why_charged_page`] renders `templates/why_charged.html` from [`WhyCharged`]: the charge as
//! `payments::api::receipt_for` read it (job, fee, tier), the charge description "Ask Hava
//! introduction service, job <n>", the quote the business sent, how the fee basis was set (the
//! [`Basis`] `payments` fixed at the pick, FEE-005, checked against the tier charged, D-1561) and
//! the refund rules. Every receipt email links it at [`WHY_CHARGED_ROUTE`]. The route that reads
//! the facts behind the business's sign-in is wired with the business portal's sign-in (D-1560).

use askama::Template;
use askhava_payments::api::{ChargeReceipt, IntentText, Tier};

use crate::confirm_pay::{REFUND_LINE, money};

/// The page's route; the receipt email's link is this path on askhava.com (FEE-031).
pub const WHY_CHARGED_ROUTE: &str = "/b/charges/{pick}";

/// What the page shows for one charge (FEE-031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyCharged {
    /// The charge, read through `payments::api::receipt_for` for the signed-in business.
    pub receipt: ChargeReceipt,
    /// The job's request number.
    pub job_number: i64,
    /// The chosen quote as the business sent it, for example "$480.00, fixed price".
    pub quote: String,
    /// How the fee basis was set at the pick (FEE-003, FEE-004, FEE-044).
    pub basis: Basis,
}

/// How `payments` set the fee basis at the pick, as the route passes it (FEE-003, FEE-004,
/// FEE-044, D-1561).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The price is set on site, so the category's usual visit fee was used (FEE-004).
    OnSite,
    /// A one-time quote's basis in cents: its price, or a range's midpoint (FEE-003).
    Quote(u64),
    /// A repeating job's basis in cents: 4 months of the regular price (FEE-044, REC-002).
    FourMonths(u64),
}

#[derive(Template)]
#[template(path = "why_charged.html")]
struct WhyView<'a> {
    job_number: i64,
    line: String,
    fee: String,
    quote: &'a str,
    tier_line: String,
    basis: String,
    refund: &'static str,
}

/// The tier's name and the basis range it covers (FEE-001).
const fn tier_words(tier: Tier) -> (&'static str, &'static str) {
    match tier {
        Tier::Small => ("small job", "under $500.00"),
        Tier::Medium => ("medium job", "from $500.00 to $1,500.00"),
        Tier::Large => ("large job", "over $1,500.00"),
    }
}

/// FEE-031, FEE-039, D-1561, WORD-001: how the basis was set, and its tier range checked against the tier
/// charged: when they differ (a recurring cap, FEE-039, or a flat fee, FEE-007) the page says the
/// fee for this kind of job is set at the charged tier's fee instead of claiming the basis set it.
fn basis_line(charged: Option<Tier>, basis: Basis) -> String {
    let cents = match basis {
        Basis::OnSite => {
            let lead = format!(
                "You set your price after seeing the job, so {} used the usual visit fee for this kind of work.",
                askhava_core::api::brand().display_name()
            );
            return charged.map_or_else(
                || lead.clone(),
                |tier| format!("{lead} That makes it a {}.", tier_words(tier).0),
            );
        }
        Basis::Quote(cents) | Basis::FourMonths(cents) => cents,
    };
    let lead = match basis {
        Basis::FourMonths(_) => format!(
            "This is a repeat job, so the fee basis is 4 months at your regular price: {}.",
            money(cents)
        ),
        _ => format!("The fee basis from your quote is {}.", money(cents)),
    };
    let (name, range) = tier_words(Tier::for_basis(cents));
    match charged {
        Some(tier) if tier == Tier::for_basis(cents) => {
            format!("{lead} That is {range}, so this is a {name}.")
        }
        Some(tier) => format!(
            "{lead} That is {range}, which would usually be a {name}, but this kind of job pays the {} fee.",
            tier_words(tier).0
        ),
        None => format!("{lead} That is {range}."),
    }
}

/// FEE-031, FEE-053, D-225: the page for `facts`; see the module docs.
///
/// # Errors
/// The template's render error, or a job number too long for the charge description.
pub fn why_charged_page(facts: &WhyCharged) -> Result<String, askama::Error> {
    let line = IntentText::for_job(facts.job_number)
        .map_err(|e| askama::Error::Custom(Box::new(e)))?
        .description();
    let fee = money(facts.receipt.fee_cents);
    let tier_line = facts.receipt.tier.map_or_else(
        || format!("Introduction fee by job size: {fee}."),
        |tier| {
            format!(
                "Introduction fee by job size: {fee} ({}).",
                tier_words(tier).0
            )
        },
    );
    WhyView {
        job_number: facts.job_number,
        line,
        fee,
        quote: &facts.quote,
        tier_line,
        basis: basis_line(facts.receipt.tier, facts.basis),
        refund: REFUND_LINE,
    }
    .render()
}
