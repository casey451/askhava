//! The "Emergency emails at any hour" switch page with its test emergency email (BIZ-007,
//! BIZ-037, BIZ-042; T150, D-346, D-362, D-492).
//!
//! [`emergency_switch_page`] renders `templates/emergency_test_email.html` for one signed-in
//! person: the switch, and once it is on [`DND_LINE`], the "Send a test emergency email" button and,
//! after the test, [`ALERT_QUESTION`] with Yes and No; a No shows the iPhone (Settings, Focus) and
//! Android (Do Not Disturb exceptions) steps from `docs/copy/mail-app-steps.md` with their help
//! pages. Turning off the last switch that keeps the after-hours tick shows [`OFF_WARNING`] first.
//! [`post_step`] is the page's POST (switch on or off, send the test, the Yes/No answer), for the
//! caller's own person only. The route [`EMERGENCY_SWITCH_ROUTE`] is not served yet (T306).
//!
//! T369 (BIZ-007, D-2702, D-2010 (c)): for the owner or a "Confirm and billing" member the page
//! also shows the after-hours box ([`AfterHoursBox`]) with [`AFTER_HOURS_LINE`];
//! [`SwitchStep::AfterHours`] calls `set_after_hours`, and a `NoCover` refusal shows the person's
//! own steps still missing. [`held_back_link`] is the link a held-back sign-up answer shows.

use askama::Template;
use askhava_businesses::api::{
    Acting, EMERGENCY_YES_LIFE, SwitchError, SwitchState, after_hours_box, record_alert_answer,
    send_emergency_test, set_after_hours, set_emergency_emails, switch_off_warns, switch_state,
};
use askhava_core::api::brand;
use jiff::Timestamp;
use sqlx::PgConnection;

use crate::email_check::{MailApp, Step, StepList, steps};

/// The page's route (not served yet).
pub const EMERGENCY_SWITCH_ROUTE: &str = "/b/emergency-emails";
/// BIZ-037: the switch's name.
pub const SWITCH_LABEL: &str = "Emergency emails at any hour";
/// BIZ-042, D-362: the line above the test button.
pub const DND_LINE: &str =
    "Turn on Do Not Disturb or Sleep the way your phone is at night, then send the test.";
/// BIZ-042: the test button.
pub const TEST_BUTTON: &str = "Send a test emergency email";
/// BIZ-042, D-362: the question after the test.
pub const ALERT_QUESTION: &str = "Did your phone alert you with Do Not Disturb or Sleep on?";
/// BIZ-007: the warning before the last switch keeping the after-hours tick goes off.
pub const OFF_WARNING: &str = "You're the last person here getting emergency emails at night. \
     Turn this off and your business stops getting emergency jobs at night. We'll email the owner \
     to let them know.";
/// BIZ-007: the after-hours box's line; the brand's name follows "for".
#[must_use]
pub fn after_hours_line() -> String {
    format!(
        "We email you about emergency jobs at any hour, marked EMERGENCY. Turn on phone alerts for \
         {} emails so you can quote and confirm fast.",
        brand().display_name()
    )
}

/// BIZ-007, D-2702: where a held-back sign-up after-hours answer links, with its link words.
#[must_use]
pub fn held_back_link(after_hours_held: bool) -> Option<(&'static str, &'static str)> {
    after_hours_held.then_some((EMERGENCY_SWITCH_ROUTE, SWITCH_LABEL))
}

/// BIZ-007: the after-hours box as the page shows it to the owner or a "Confirm and billing"
/// member (T369).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AfterHoursBox {
    /// The business is ticked for after-hours emergencies.
    pub ticked: bool,
    /// A tick was just refused (`NoCover`): the page lists the person's steps still missing.
    pub refused: bool,
}

/// BIZ-042: the missing-step line for the Yes to [`ALERT_QUESTION`].
const ANSWER_YES_STEP: &str =
    "Answer Yes to \"Did your phone alert you with Do Not Disturb or Sleep on?\"";

/// BIZ-007, BIZ-037, BIZ-042: `state`'s steps still missing before this person covers.
#[must_use]
pub fn missing_steps(state: SwitchState) -> Vec<&'static str> {
    [
        (!state.on, "Turn on emergency emails at any hour."),
        (!state.test_sent, "Send a test emergency email."),
        (!state.yes_current, ANSWER_YES_STEP),
    ]
    .into_iter()
    .filter_map(|(missing, line)| missing.then_some(line))
    .collect()
}

/// BIZ-042: the No answer's per-phone steps, each with its phone's name: iPhone (Settings,
/// Focus) and Android (Do Not Disturb exceptions, the Android help page's row).
#[must_use]
pub fn dnd_steps() -> Vec<(&'static str, Step)> {
    [("iPhone", MailApp::IPhone), ("Android", MailApp::Gmail)]
        .into_iter()
        .flat_map(|(phone, app)| {
            steps(StepList::PickAlert, app)
                .into_iter()
                .map(move |s| (phone, s))
        })
        .filter(|(_, s)| s.text.contains("Do Not Disturb"))
        .collect()
}

#[derive(Template)]
#[template(path = "emergency_test_email.html")]
struct SwitchView<'a> {
    state: SwitchState,
    warns: bool,
    help: Vec<(&'static str, Step)>,
    after_hours: Option<AfterHoursBox>,
    missing: Vec<&'static str>,
    csrf: &'a str,
}

impl SwitchView<'_> {
    fn line(&self, name: &str) -> &'static str {
        match name {
            "switch" => SWITCH_LABEL,
            "dnd" => DND_LINE,
            "test" => TEST_BUTTON,
            "warn" => OFF_WARNING,
            _ => ALERT_QUESTION,
        }
    }

    fn after_hours_line(&self) -> String {
        after_hours_line()
    }
}

/// BIZ-042, TD-BIZ-24: the page for `state`; `warns` shows [`OFF_WARNING`], `show_steps` the
/// Do Not Disturb steps after a No.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn emergency_switch_page(
    state: SwitchState,
    (warns, show_steps): (bool, bool),
    csrf: &str,
) -> Result<String, askama::Error> {
    emergency_switch_page_with_box(state, (warns, show_steps), None, csrf)
}

/// BIZ-007, BIZ-042, TD-BIZ-47: [`emergency_switch_page`] with the after-hours box when `after_hours`
/// is some (the owner or a "Confirm and billing" member); a refused box lists [`missing_steps`].
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn emergency_switch_page_with_box(
    state: SwitchState,
    (warns, show_steps): (bool, bool),
    after_hours: Option<AfterHoursBox>,
    csrf: &str,
) -> Result<String, askama::Error> {
    let help = if show_steps { dnd_steps() } else { Vec::new() };
    let refused = after_hours.is_some_and(|b| b.refused);
    let missing = if refused {
        missing_steps(state)
    } else {
        Vec::new()
    };
    SwitchView {
        state,
        warns,
        help,
        after_hours,
        missing,
        csrf,
    }
    .render()
}

/// What the page's POST asked for (parsed once at the edge, R1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchStep {
    /// Turn the switch on or off.
    Switch(bool),
    /// Send the test emergency email.
    SendTest,
    /// The answer to [`ALERT_QUESTION`]: Yes or No.
    Answer(bool),
    /// BIZ-007: tick or untick after-hours emergencies (owner or "Confirm and billing" only).
    AfterHours(bool),
}

impl SwitchStep {
    /// The step for the posted `step` value; `None` for anything else.
    #[must_use]
    pub fn from_form(step: &str) -> Option<Self> {
        match step {
            "on" => Some(Self::Switch(true)),
            "off" => Some(Self::Switch(false)),
            "send_test" => Some(Self::SendTest),
            "yes" => Some(Self::Answer(true)),
            "no" => Some(Self::Answer(false)),
            "after_hours_on" => Some(Self::AfterHours(true)),
            "after_hours_off" => Some(Self::AfterHours(false)),
            _ => None,
        }
    }
}

/// BIZ-007, BIZ-037, BIZ-042: applies `step` for `by` (the signed-in person, never one from the
/// browser) and renders the page again; a No shows the steps; an after-hours tick refused for
/// `NoCover` renders the box with the missing steps.
///
/// # Errors
/// [`SwitchError`] from `businesses` (`NotAllowed` for an after-hours step by anyone but the owner
/// or a "Confirm and billing" member), or `Database` when the page fails to render.
pub async fn post_step(
    conn: &mut PgConnection,
    by: Acting,
    (step, csrf): (SwitchStep, &str),
    now: Timestamp,
) -> Result<String, SwitchError> {
    let mut refused = false;
    match step {
        SwitchStep::Switch(on) => set_emergency_emails(conn, by, on, now).await?,
        SwitchStep::SendTest => send_emergency_test(conn, by, now).await?,
        SwitchStep::Answer(yes) => record_alert_answer(conn, by, yes, now).await?,
        SwitchStep::AfterHours(on) => refused = tick(conn, by, on, now).await?,
    }
    let state = switch_state(conn, by, (now, EMERGENCY_YES_LIFE)).await?;
    let warns = state.on && switch_off_warns(conn, by).await?;
    let show_steps = step == SwitchStep::Answer(false);
    let after_hours = after_hours_box(conn, by)
        .await?
        .map(|ticked| AfterHoursBox { ticked, refused });
    emergency_switch_page_with_box(state, (warns, show_steps), after_hours, csrf)
        .map_err(|_| SwitchError::Database)
}

/// BIZ-007, D-2702: the after-hours step for the owner or a "Confirm and billing" member; true
/// when the tick was refused because nobody covers.
async fn tick(
    conn: &mut PgConnection,
    by: Acting,
    on: bool,
    now: Timestamp,
) -> Result<bool, SwitchError> {
    if after_hours_box(conn, by).await?.is_none() {
        return Err(SwitchError::NotAllowed);
    }
    match set_after_hours(conn, by.business, on, (now, EMERGENCY_YES_LIFE)).await {
        Ok(()) => Ok(false),
        Err(SwitchError::NoCover) => Ok(true),
        Err(e) => Err(e),
    }
}
