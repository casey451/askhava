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

use askama::Template;
use askhava_payments::api::{AUTHORIZATION_TEXT, ChargeOutcome, Fee, Tier};
use sqlx::types::Uuid;

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
}

/// The page's name for the tier (FEE-001).
const fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Small => "small job",
        Tier::Medium => "medium job",
        Tier::Large => "large job",
    }
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
        assert!(html.contains("I authorize Ask Hava LLC to save this card with Stripe"));
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
}
