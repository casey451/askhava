//! The owner's AI page: what the "AI on scrubbed text" switch still needs, the two account
//! checklist forms and the switch itself (AI-008, AI-018, SEC-006, SEC-043, A11Y-030; T737,
//! D-412, D-418, D-492, D-4727, D-5534).
//!
//! [`owner_ai_page`] renders `templates/owner_ai_switch.html` from `ai::api::missing_records`:
//! each missing record in plain words, the provider-terms form, the policy-version form and the
//! on and off buttons. [`owner_ai_change`] runs one [`AiChange`] after `OwnerAction::AiSwitch`'s
//! fresh factor (a stale one is [`OwnerToolError::NeedsFreshFactor`] and records nothing): a form
//! is parsed here once (R1) and recorded through `ai::api::record_provider_terms` or
//! `record_policy_version`; the switch goes through `ai::api::set_ai_switch`, whose refusal names
//! the first missing record. A bad field, a repeated version name or a refused on comes back as
//! [`AiShown::errors`], shown A11Y-030's way with every typed value kept. No AI call is made here.
//! `server`'s `owner_ai_routes` serves the routes behind the owner session.

use askama::Template;
use askhava_ai::api::{
    ChecklistError, Missing, PolicyVersion, ProviderTerms, SwitchError, missing_records,
    record_policy_version, record_provider_terms, set_ai_switch,
};
use jiff::Timestamp;
use jiff::civil::Date;
use serde::Deserialize;
use sqlx::PgConnection;

use crate::owner_auth::{OwnerAction, OwnerAuthError, OwnerPrincipal, require_owner_step_up};
use crate::owner_portal::OwnerToolError;
use crate::owner_step_up_page::OWNER_STEP_UP_ROUTE;

/// The AI page (`GET`).
pub const OWNER_AI_ROUTE: &str = "/owner/ai";
/// The provider-terms record (`POST`, a [`TermsForm`]).
pub const OWNER_AI_TERMS_ROUTE: &str = "/owner/ai/terms";
/// The policy-version record (`POST`, a [`PolicyForm`]).
pub const OWNER_AI_POLICY_ROUTE: &str = "/owner/ai/policy";
/// The switch (`POST`, `to` = `on` or `off`).
pub const OWNER_AI_SWITCH_ROUTE: &str = "/owner/ai/switch";

/// The provider-terms form as typed; `training` is `yes` when ticked. Money is in dollars.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TermsForm {
    /// The terms page's link.
    pub url: String,
    /// The day they were read, `YYYY-MM-DD`.
    pub checked_on: String,
    /// `yes` when the terms say API inputs are not used for training.
    pub training: String,
    /// Days the provider keeps API inputs.
    pub retention_days: String,
    /// Days it may keep flagged text.
    pub flagged_days: String,
    /// The Console's monthly spend limit, in dollars.
    pub spend_limit: String,
    /// The expected monthly AI cost, in dollars.
    pub expected_cost: String,
    /// The zero-retention signing day, empty when not signed.
    pub zero_retention_on: String,
}

/// The policy-version form as typed; `ai018` is `yes` when ticked.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyForm {
    /// The version name.
    pub version: String,
    /// The day it takes effect, `YYYY-MM-DD`.
    pub effective_on: String,
    /// `yes` when it carries the AI-018 wording.
    pub ai018: String,
}

/// One change the owner asks for on the AI page (AI-008, SEC-043).
#[derive(Debug, Clone)]
pub enum AiChange {
    /// Record the provider's terms.
    Terms(TermsForm),
    /// Record a privacy-policy version.
    Policy(PolicyForm),
    /// Turn the switch on (`true`) or off.
    Switch(bool),
}

/// What the AI page shows after a change: its done line, or its errors (each a field ID and a
/// message) with the typed forms kept (A11Y-030).
#[derive(Debug, Clone, Default)]
pub struct AiShown {
    done: Option<&'static str>,
    errors: Vec<(&'static str, String)>,
    terms: TermsForm,
    policy: PolicyForm,
}

impl AiShown {
    /// Whether the change was refused (answered 400 with the error summary).
    #[must_use]
    pub fn refused(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// The plain words for a missing record and the field it links to (AI-008, D-492).
fn missing_words(missing: Missing) -> (&'static str, &'static str) {
    match missing {
        Missing::Terms => (
            "url",
            "The AI provider's terms aren't recorded yet. Record them below.",
        ),
        Missing::TrainingFree => (
            "training",
            "The newest terms don't say API inputs are kept out of training.",
        ),
        Missing::FlaggedRetention => (
            "flagged_days",
            "The newest terms don't say how long flagged text may be kept.",
        ),
        Missing::SpendLimit => (
            "spend_limit",
            "No spend limit is recorded, or it's more than 3 times what you expect to spend.",
        ),
        Missing::PolicyWording => (
            "version",
            "Today's privacy policy doesn't have the AI wording yet.",
        ),
    }
}

const DATE_ERROR: &str = "Enter the date as year-month-day, like 2026-11-23.";
const DAYS_ERROR: &str = "Enter a whole number of days from 0 to 3650.";
const MONEY_ERROR: &str = "Enter dollars above 0, like 60 or 60.50.";

fn date(typed: &str) -> Option<Date> {
    typed.trim().parse().ok()
}

fn days(typed: &str) -> Option<i32> {
    typed.trim().parse().ok().filter(|d| (0..=3650).contains(d))
}

/// Dollars as typed (`60`, `$60.5`, `60.50`) in cents, above 0 and below $10 million.
fn cents(typed: &str) -> Option<i64> {
    let typed = typed.trim().trim_start_matches('$');
    let (whole, part) = typed.split_once('.').unwrap_or((typed, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || whole.len() > 7 || part.len() > 2 || !digits(whole) || !digits(part) {
        return None;
    }
    let part = format!("{part:0<2}").parse::<i64>().ok()?;
    Some(whole.parse::<i64>().ok()? * 100 + part).filter(|c| *c > 0)
}

fn url_ok(typed: &str) -> bool {
    let rest = typed.strip_prefix("https://").unwrap_or_default();
    !rest.is_empty() && typed.len() <= 300 && !typed.chars().any(char::is_whitespace)
}

fn version_ok(typed: &str) -> bool {
    let ok = |b: u8| b.is_ascii_digit() || b.is_ascii_lowercase();
    let mut bytes = typed.bytes();
    typed.len() <= 32
        && bytes.next().is_some_and(ok)
        && bytes.all(|b| ok(b) || b == b'.' || b == b'-')
}

/// Records `value` or an error on field `id`.
fn need<T>(
    errors: &mut Vec<(&'static str, String)>,
    id: &'static str,
    value: Option<T>,
    why: &str,
) -> Option<T> {
    if value.is_none() {
        errors.push((id, why.to_owned()));
    }
    value
}

/// The typed terms, checked against `ai.provider_terms`' constraints, or each field's error (R1).
fn parse_terms(form: &TermsForm) -> Result<ProviderTerms, Vec<(&'static str, String)>> {
    let mut e = Vec::new();
    let url = form.url.trim();
    let url_why = "Enter the terms page link, starting https://";
    let url = need(&mut e, "url", url_ok(url).then(|| url.to_owned()), url_why);
    let checked_on = need(&mut e, "checked_on", date(&form.checked_on), DATE_ERROR);
    let retention = need(
        &mut e,
        "retention_days",
        days(&form.retention_days),
        DAYS_ERROR,
    );
    let flagged = need(&mut e, "flagged_days", days(&form.flagged_days), DAYS_ERROR);
    let spend = need(&mut e, "spend_limit", cents(&form.spend_limit), MONEY_ERROR);
    let expected = need(
        &mut e,
        "expected_cost",
        cents(&form.expected_cost),
        MONEY_ERROR,
    );
    let signed = form.zero_retention_on.trim();
    let zero = (!signed.is_empty()).then(|| date(signed));
    if zero == Some(None) {
        e.push(("zero_retention_on", DATE_ERROR.to_owned()));
    }
    match (url, checked_on, retention, flagged, spend, expected) {
        (Some(url), Some(checked_on), Some(r), Some(f), Some(s), Some(x)) if e.is_empty() => {
            Ok(ProviderTerms {
                url,
                checked_on,
                training_free: form.training == "yes",
                retention_days: r,
                flagged_retention_days: f,
                spend_limit_cents: s,
                expected_monthly_cents: x,
                zero_retention_signed_on: zero.flatten(),
            })
        }
        _ => Err(e),
    }
}

/// The typed policy version, or each field's error (R1).
fn parse_policy(form: &PolicyForm) -> Result<PolicyVersion, Vec<(&'static str, String)>> {
    let mut e = Vec::new();
    let version = form.version.trim();
    let why = "Use up to 32 lowercase letters, digits, dots or dashes, like 2026-11-23.";
    let version = need(
        &mut e,
        "version",
        version_ok(version).then(|| version.to_owned()),
        why,
    );
    let effective_on = need(&mut e, "effective_on", date(&form.effective_on), DATE_ERROR);
    match (version, effective_on) {
        (Some(version), Some(effective_on)) => Ok(PolicyVersion {
            version,
            effective_on,
            has_ai018_sentence: form.ai018 == "yes",
        }),
        _ => Err(e),
    }
}

/// AI-008, AI-018, SEC-006, SEC-043, A11Y-030, D-4727: `change` for `principal` at `now`, after
/// `OwnerAction::AiSwitch`'s fresh factor; see the module docs. Each record or switch change
/// writes its own IDs-only audit row in `ai`'s transaction.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`] (nothing recorded) or [`OwnerToolError::Database`].
pub async fn owner_ai_change(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    change: AiChange,
    now: Timestamp,
) -> Result<AiShown, OwnerToolError> {
    let route = match &change {
        AiChange::Terms(_) => OWNER_AI_TERMS_ROUTE,
        AiChange::Policy(_) => OWNER_AI_POLICY_ROUTE,
        AiChange::Switch(_) => OWNER_AI_SWITCH_ROUTE,
    };
    let step = (OwnerAction::AiSwitch, route);
    match require_owner_step_up(conn, principal, step, now).await {
        Err(OwnerAuthError::NeedsFreshFactor) => return Err(OwnerToolError::NeedsFreshFactor),
        other => other.map_err(|_| OwnerToolError::Database)?,
    }
    let owner = principal.owner();
    let mut shown = AiShown::default();
    let made = match change {
        AiChange::Terms(form) => terms(conn, &form, owner)
            .await
            .map(|e| (e, Some(form), None)),
        AiChange::Policy(form) => policy(conn, &form, owner)
            .await
            .map(|e| (e, None, Some(form))),
        AiChange::Switch(on) => switch(conn, on, owner).await.map(|e| (e, None, None)),
    };
    match made? {
        (Ok(done), _, _) => shown.done = Some(done),
        (Err(errors), terms, policy) => {
            shown.errors = errors;
            shown.terms = terms.unwrap_or_default();
            shown.policy = policy.unwrap_or_default();
        }
    }
    Ok(shown)
}

type Made = Result<Result<&'static str, Vec<(&'static str, String)>>, OwnerToolError>;

async fn terms(conn: &mut PgConnection, form: &TermsForm, owner: sqlx::types::Uuid) -> Made {
    let terms = match parse_terms(form) {
        Ok(terms) => terms,
        Err(errors) => return Ok(Err(errors)),
    };
    record_provider_terms(conn, &terms, owner)
        .await
        .map_err(|_| OwnerToolError::Database)?;
    Ok(Ok("The provider's terms are recorded."))
}

async fn policy(conn: &mut PgConnection, form: &PolicyForm, owner: sqlx::types::Uuid) -> Made {
    let policy = match parse_policy(form) {
        Ok(policy) => policy,
        Err(errors) => return Ok(Err(errors)),
    };
    match record_policy_version(conn, &policy, owner).await {
        Ok(()) => Ok(Ok("The privacy policy version is recorded.")),
        Err(ChecklistError::Duplicate) => {
            let why = "That version name is already recorded. Use a new one.";
            Ok(Err(vec![("version", why.to_owned())]))
        }
        Err(ChecklistError::Database) => Err(OwnerToolError::Database),
    }
}

async fn switch(conn: &mut PgConnection, on: bool, owner: sqlx::types::Uuid) -> Made {
    match set_ai_switch(conn, on, owner).await {
        Ok(()) if on => Ok(Ok("AI on scrubbed text is now on.")),
        Ok(()) => Ok(Ok("AI on scrubbed text is now off.")),
        Err(SwitchError::Refused(missing)) => {
            let (field, words) = missing_words(missing);
            let why = format!("AI can't be turned on yet. {words}");
            Ok(Err(vec![(field, why)]))
        }
        Err(SwitchError::Database) => Err(OwnerToolError::Database),
    }
}

#[derive(Template)]
#[template(path = "owner_ai_switch.html")]
struct AiView<'a> {
    csrf: &'a str,
    routes: [&'static str; 4],
    missing: Vec<(&'static str, &'static str)>,
    on: bool,
    shown: &'a AiShown,
}

impl AiView<'_> {
    /// Field `id`'s error, if any.
    fn err(&self, id: &str) -> Option<&str> {
        let found = self.shown.errors.iter().find(|(f, _)| *f == id);
        found.map(|(_, why)| why.as_str())
    }
}

/// AI-008, AI-018, A11Y-030: the AI page for `csrf` showing `shown`: every missing record in plain
/// words, the switch's state, the two forms and the buttons. Reads only.
///
/// # Errors
///
/// [`OwnerToolError::Database`] when the records cannot be read or the page cannot render.
pub async fn owner_ai_page(
    conn: &mut PgConnection,
    csrf: &str,
    shown: &AiShown,
) -> Result<String, OwnerToolError> {
    let (missing, on) = missing_records(conn)
        .await
        .map_err(|_| OwnerToolError::Database)?;
    AiView {
        csrf,
        routes: [
            OWNER_AI_TERMS_ROUTE,
            OWNER_AI_POLICY_ROUTE,
            OWNER_AI_SWITCH_ROUTE,
            OWNER_STEP_UP_ROUTE,
        ],
        missing: missing.into_iter().map(missing_words).collect(),
        on,
        shown,
    }
    .render()
    .map_err(|_| OwnerToolError::Database)
}

#[cfg(test)]
mod tests {
    //! The edge parsing of the two forms (AI-008, R1). Fake values only.

    use super::{PolicyForm, TermsForm, cents, parse_policy, parse_terms, version_ok};

    #[test]
    fn dollars_read_as_cents() {
        assert_eq!(cents("60"), Some(6000));
        assert_eq!(cents(" $60.5 "), Some(6050));
        assert_eq!(cents("0.07"), Some(7));
        for bad in ["", "0", "abc", "1.234", "-5", ".5", "99999999", "6 0"] {
            assert_eq!(cents(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_bad_form_names_each_bad_field() {
        let form = TermsForm {
            url: "http://example.com/terms".to_owned(),
            zero_retention_on: "soon".to_owned(),
            ..TermsForm::default()
        };
        let fields: Vec<_> = parse_terms(&form)
            .expect_err("bad")
            .into_iter()
            .map(|e| e.0)
            .collect();
        let want = [
            "url",
            "checked_on",
            "retention_days",
            "flagged_days",
            "spend_limit",
        ];
        assert!(want.iter().all(|f| fields.contains(f)), "{fields:?}");
        assert!(fields.contains(&"zero_retention_on"));
        assert!(version_ok("2026-11-23") && !version_ok("V1") && !version_ok("-1"));
        let policy = PolicyForm::default();
        assert_eq!(parse_policy(&policy).expect_err("bad").len(), 2);
    }
}
