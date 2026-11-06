//! The business's "Why was I charged?" page for one charge (FEE-031, FEE-053, D-225, D-491).
//!
//! [`why_charged_page`] renders `templates/why_charged.html` from [`WhyCharged`]: the charge as
//! `payments::api::receipt_for` read it (job, fee, tier), the charge description "Ask Hava
//! introduction service, job <n>", the quote the business sent, how the fee basis was set (the
//! basis `payments` fixed at the pick, FEE-005) and the refund rules. Every receipt email links
//! it at [`WHY_CHARGED_ROUTE`]. The route that reads the facts behind the business's sign-in is
//! wired with the business portal's sign-in (D-1560).

use askama::Template;
use askhava_payments::api::{ChargeReceipt, IntentText, Tier};

use crate::confirm_pay::REFUND_LINE;

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
    /// The fee basis fixed at the pick, or `None` when the price is set on site (FEE-004).
    pub basis_cents: Option<u64>,
}

#[derive(Template)]
#[template(path = "why_charged.html")]
struct WhyView<'a> {
    job_number: i64,
    line: String,
    fee: String,
    quote: &'a str,
    tier: &'static str,
    basis: String,
    refund: &'static str,
}

/// `$1,234.56` for `cents`.
fn money(cents: u64) -> String {
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

/// The tier's name and the basis range it covers (FEE-001).
const fn tier_words(tier: Tier) -> (&'static str, &'static str) {
    match tier {
        Tier::Small => ("small job", "under $500.00"),
        Tier::Medium => ("medium job", "from $500.00 to $1,500.00"),
        Tier::Large => ("large job", "over $1,500.00"),
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
    let (tier, range) = tier_words(facts.receipt.tier);
    let basis = facts.basis_cents.map_or_else(
        || format!("The price gets set on site, so the fee went by this trade's usual visit fee: the {tier} fee."),
        |cents| format!("Your {} quote is {range}, so this is a {tier}.", money(cents)),
    );
    WhyView {
        job_number: facts.job_number,
        line,
        fee: money(facts.receipt.fee_cents),
        quote: &facts.quote,
        tier,
        basis,
        refund: REFUND_LINE,
    }
    .render()
}
