//! The sign-up inbox check and the sample pick step screens (BIZ-042, BIZ-043; T149, D-328,
//! D-352, D-380, D-462).
//!
//! [`email_check_page`] is the screen after the sign-up email is verified (and each team member's
//! first sign-in): "No email? Check your Spam or Junk folder.", the sample card's resend, "Did it
//! arrive in your inbox?" with its four answers, the chosen mail app's steps for every answer but
//! Inbox, and the last step ("Reply OK to this email ...") with its per-app picture guide; an OK
//! reply shows as "Reply received". [`sample_pick_page`] is the "Send a sample pick" step that
//! `business_actions::sample_pick_on_open` shows the first time an owner or "Confirm and billing"
//! person opens a real job card. Every mail-app step's words, its maker's help-page address and the
//! date it was checked come from `docs/copy/mail-app-steps.md` ([`steps`]), never from this file.
//! Nothing here blocks activation: every screen offers "Skip for now".

use askama::Template;
use askhava_businesses::api::{EmailCheck, InboxAnswer};

/// The mail-app steps, their help pages and check dates (BIZ-042).
const MAIL_APP_STEPS: &str = include_str!("../../../docs/copy/mail-app-steps.md");

/// BIZ-042: the verification screen's line.
pub const NO_EMAIL_LINE: &str = "No email? Check your Spam or Junk folder.";
/// BIZ-042: the inbox question after the sample card.
pub const ARRIVED_QUESTION: &str = "Did it arrive in your inbox?";
/// BIZ-042, D-352: the last onboarding step.
pub const LAST_STEP_LINE: &str = "Reply OK to this email, star it and add us to your contacts, so our emails reach your inbox and your phone alerts you.";
/// BIZ-042, D-352: what an OK reply to the sample shows.
pub const REPLY_RECEIVED: &str = "Reply received";
/// BIZ-042, D-462: the sample pick step's title.
pub const SAMPLE_PICK_TITLE: &str = "Send a sample pick";
/// BIZ-042, D-380: the sample pick step's Do Not Disturb or Sleep line.
pub const PICK_DND_LINE: &str =
    "Turn on Do Not Disturb or Sleep the way your phone usually is, then send the test.";
/// BIZ-042, D-380: the line under the sample pick step.
pub const PICK_SKIP_LINE: &str = "Skip this and you may miss picks. A pick you don't confirm in 4 business hours may go to another business.";
/// BIZ-042: the question after the sample pick.
pub const PICK_ALERT_QUESTION: &str = "Did your phone alert you?";

/// The mail app a person picks on the screen (BIZ-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailApp {
    /// Gmail, on the web or the Gmail app.
    Gmail,
    /// Outlook.
    Outlook,
    /// Yahoo Mail.
    Yahoo,
    /// Mail on iPhone.
    IPhone,
    /// Any other mail app.
    Other,
}

impl MailApp {
    /// Every app, in the order the screen lists them.
    pub const ALL: [Self; 5] = [
        Self::Gmail,
        Self::Outlook,
        Self::Yahoo,
        Self::IPhone,
        Self::Other,
    ];

    /// The code in the steps file and the form.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Outlook => "outlook",
            Self::Yahoo => "yahoo",
            Self::IPhone => "iphone",
            Self::Other => "other",
        }
    }

    /// The name the screen shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail",
            Self::Outlook => "Outlook",
            Self::Yahoo => "Yahoo Mail",
            Self::IPhone => "iPhone Mail",
            Self::Other => "Another mail app",
        }
    }

    /// The app for a posted code; anything unknown is [`MailApp::Other`].
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|a| a.code() == code)
            .unwrap_or(Self::Other)
    }
}

/// Which list of steps (the `##` heading in the steps file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepList {
    /// What to do when the sample landed in Spam, another tab or not at all.
    Inbox,
    /// The last step's picture guide: star, contacts, VIP, Primary.
    LastStep,
    /// Giving Ask Hava email its own phone alert, for the sample pick.
    PickAlert,
}

impl StepList {
    const fn heading(self) -> &'static str {
        match self {
            Self::Inbox => "## inbox",
            Self::LastStep => "## last-step",
            Self::PickAlert => "## pick-alert",
        }
    }
}

/// One mail-app step with its maker's help page and the date the page was checked (BIZ-042).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The words shown.
    pub text: String,
    /// The maker's help page the step was checked against; empty for "another mail app".
    pub help_page: String,
    /// The date the help page was checked, `YYYY-MM-DD`.
    pub checked: String,
}

/// BIZ-042: the steps of `list` for `app`, in file order, read from `docs/copy/mail-app-steps.md`.
#[must_use]
pub fn steps(list: StepList, app: MailApp) -> Vec<Step> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in MAIL_APP_STEPS.lines() {
        if line.starts_with("## ") {
            inside = line.trim() == list.heading();
            continue;
        }
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if inside && cells.len() == 4 && cells[0] == app.code() {
            let page = if cells[2] == "none" { "" } else { cells[2] };
            out.push(Step {
                text: cells[1].to_owned(),
                help_page: page.to_owned(),
                checked: cells[3].to_owned(),
            });
        }
    }
    out
}

/// One answer button.
struct Answer {
    code: &'static str,
    label: &'static str,
}

const fn answer_label(a: InboxAnswer) -> &'static str {
    match a {
        InboxAnswer::Inbox => "Inbox",
        InboxAnswer::SpamOrJunk => "Spam or Junk",
        InboxAnswer::OtherTab => "Promotions or another tab",
        InboxAnswer::NotYet => "Not yet",
    }
}

#[derive(Template)]
#[template(path = "email_check.html")]
struct EmailCheckView<'a> {
    pick: bool,
    app: MailApp,
    apps: [MailApp; 5],
    sent: bool,
    answers: Vec<Answer>,
    help: Vec<Step>,
    last: Vec<Step>,
    reply_received: bool,
    csrf: &'a str,
}

impl EmailCheckView<'_> {
    fn line(&self, name: &str) -> &'static str {
        match name {
            "no_email" => NO_EMAIL_LINE,
            "arrived" => ARRIVED_QUESTION,
            "last" => LAST_STEP_LINE,
            "received" => REPLY_RECEIVED,
            "pick_title" => SAMPLE_PICK_TITLE,
            "pick_dnd" => PICK_DND_LINE,
            "pick_skip" => PICK_SKIP_LINE,
            _ => PICK_ALERT_QUESTION,
        }
    }
}

/// BIZ-042, TD-BIZ-18: the inbox check screen for one person's `check` and chosen `app`.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn email_check_page(
    check: EmailCheck,
    app: MailApp,
    csrf: &str,
) -> Result<String, askama::Error> {
    let shows = check.answer.is_some_and(InboxAnswer::shows_steps);
    let view = EmailCheckView {
        pick: false,
        app,
        apps: MailApp::ALL,
        sent: check.sample_sent,
        answers: InboxAnswer::ALL
            .into_iter()
            .map(|a| Answer {
                code: a.code(),
                label: answer_label(a),
            })
            .collect(),
        help: if shows {
            steps(StepList::Inbox, app)
        } else {
            Vec::new()
        },
        last: steps(StepList::LastStep, app),
        reply_received: check.reply_received,
        csrf,
    };
    view.render()
}

/// BIZ-042, TD-BIZ-28: the "Send a sample pick" step; `show_steps` after a No shows the alert
/// steps for `app` again.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn sample_pick_page(
    app: MailApp,
    show_steps: bool,
    csrf: &str,
) -> Result<String, askama::Error> {
    let view = EmailCheckView {
        pick: true,
        app,
        apps: MailApp::ALL,
        sent: false,
        answers: Vec::new(),
        help: if show_steps {
            steps(StepList::PickAlert, app)
        } else {
            Vec::new()
        },
        last: Vec::new(),
        reply_received: false,
        csrf,
    };
    view.render()
}
