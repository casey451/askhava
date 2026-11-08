//! The "Emergency emails at any hour" switch page with its test emergency email (BIZ-007,
//! BIZ-037, BIZ-042; T150, D-346, D-362, D-492).
//!
//! [`emergency_switch_page`] renders `templates/emergency_test_email.html` for one signed-in
//! person: the switch, and once it is on [`DND_LINE`], the "Send a test emergency email" button and,
//! after the test, [`ALERT_QUESTION`] with Yes and No; a No shows the iPhone (Settings, Focus) and
//! Android (Do Not Disturb exceptions) steps from `docs/copy/mail-app-steps.md` with their help
//! pages. Turning off the last switch that keeps the after-hours tick shows [`OFF_WARNING`] first.
//! [`post_step`] is the page's POST (switch on or off, send the test, the Yes/No answer), for the
//! caller's own person only. The route [`EMERGENCY_SWITCH_ROUTE`] is not served yet.

use askama::Template;
use askhava_businesses::api::{
    Acting, EMERGENCY_YES_LIFE, SwitchError, SwitchState, record_alert_answer, send_emergency_test,
    set_emergency_emails, switch_off_warns, switch_state,
};
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
    let help = if show_steps { dnd_steps() } else { Vec::new() };
    SwitchView {
        state,
        warns,
        help,
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
            _ => None,
        }
    }
}

/// BIZ-007, BIZ-037, BIZ-042: applies `step` for `by` (the signed-in person, never one from the
/// browser) and renders the page again; a No shows the steps.
///
/// # Errors
/// [`SwitchError`] from `businesses`, or `Database` when the page fails to render.
pub async fn post_step(
    conn: &mut PgConnection,
    by: Acting,
    (step, csrf): (SwitchStep, &str),
    now: Timestamp,
) -> Result<String, SwitchError> {
    match step {
        SwitchStep::Switch(on) => set_emergency_emails(conn, by, on, now).await?,
        SwitchStep::SendTest => send_emergency_test(conn, by, now).await?,
        SwitchStep::Answer(yes) => record_alert_answer(conn, by, yes, now).await?,
    }
    let state = switch_state(conn, by, (now, EMERGENCY_YES_LIFE)).await?;
    let warns = state.on && switch_off_warns(conn, by).await?;
    let show_steps = step == SwitchStep::Answer(false);
    emergency_switch_page(state, (warns, show_steps), csrf).map_err(|_| SwitchError::Database)
}
