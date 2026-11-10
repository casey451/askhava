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
//! T155 added the capability step (BIZ-049, BIZ-004, CARD-001; D-393, D-492): [`capability_page`]
//! over `templates/capability_setup.html` shows the checklist `businesses::api::checklist_offer`
//! builds from the license map (pre-ticked, untick any, no-license extras only), then towns,
//! largest job, recurring work and after-hours, as checkboxes on one page ([`CapabilityStep::All`])
//! or one question per guided screen; [`CapabilityValues`] reads either post the same way and
//! [`capability_summary`] reads the answers back on the check screen. Saving is
//! `businesses::api::save_capability`; the route waits with the rest of sign-up (D-1709).

use std::collections::BTreeSet;

use askama::Template;
use askhava_businesses::api::{
    Capability, CapabilityAnswers, JobTypeChoice, LargestJob, TradeOffer, kept_ticks, pre_ticked,
};
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
    notice: String,
    notice_lang: &'static str,
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
        Screen::Form(categories) => {
            v.title = "Sign up your business";
            v.categories = categories
                .iter()
                .map(|(c, l)| (c.clone(), category_name(c), *l))
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
        out
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

    /// The problem message for `field`, if any.
    fn error_for(&self, field: &str) -> Option<&'static str> {
        let found = self.page.problems.iter().find(|p| p.field() == field);
        found.map(|p| p.message())
    }
}

/// BIZ-049, BIZ-004, BIZ-007, BIZ-010, BIZ-030, A11Y-012, A11Y-030: the capability step's page
/// over `templates/capability_setup.html`: the checklist pre-ticked from the license, the
/// no-license extras, towns, largest job, recurring work and after-hours, all on one page or one
/// per guided screen; the check screen reads the answers back. The form posts to
/// [`CAPABILITY_ROUTE`] with `step` set to the next screen.
///
/// # Errors
/// The template's render error.
pub fn capability_page(page: &CapabilityPage<'_>) -> Result<String, askama::Error> {
    use CapabilityStep as S;
    let step = page.step;
    let all = step == S::All;
    let show = [
        S::JobTypes,
        S::Towns,
        S::Largest,
        S::Recurring,
        S::AfterHours,
    ]
    .map(|s| all || s == step);
    let summary = match (step, page.values.answers()) {
        (S::Check, Ok(a)) => capability_summary(page.offer, page.towns, &a),
        _ => Vec::new(),
    };
    let (title, button) = match step {
        S::All => ("Pick the jobs we send you", "Check your answers"),
        S::Check => ("Check your answers", "Save and continue"),
        _ => ("Pick the jobs we send you", "Next"),
    };
    let carried = if all {
        Vec::new()
    } else {
        page.values.carried(step)
    };
    let next = step.next().unwrap_or(S::Check).slug().to_owned();
    CapabilityView {
        page,
        title,
        show,
        carried,
        summary,
        next,
        button,
    }
    .render()
}
