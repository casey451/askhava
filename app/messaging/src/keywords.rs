//! The HELP, INFO and AYUDA reply (CON-018, D-131, D-066, D-289, D-290, D-349, D-371).
//!
//! [`keyword`] says whether a text is only HELP, INFO or AYUDA ("I need help with my sink" is a
//! request, not a keyword). [`reply_to_help`] queues the one reply for such a text at once, under
//! the `help_reply` template ID that `rules::may_send_to` lets through to an opted-out number and
//! that no quiet-hours check holds back, so it goes within 5 minutes at any hour. The body is
//! one of four registry texts: normal or owner-away (OPS-026), each with or without the roaming
//! line for a Canadian number. The reply times come from [`HelpSettings`], never from the text.
//! START needs no reply code here: the provider's `OptOutType=START` lifts the listing and sends
//! its own reply (`optout::on_inbound_text`, CON-015, CON-030, D-870).

use askhava_privacy::api::normalize_phone;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::models::OutboxError;
use crate::optout::InboundText;
use crate::rules::HELP_TEMPLATE;
use crate::service::{self, NewText};
use crate::templates::{Language, Registry};
use secrecy::SecretString;

/// Canadian area codes: a reply to one of these numbers adds the roaming line (CON-018).
const CANADIAN_AREA_CODES: [&str; 54] = [
    "204", "226", "236", "249", "250", "263", "289", "306", "343", "354", "365", "367", "368",
    "382", "403", "416", "418", "428", "431", "437", "438", "450", "468", "474", "506", "514",
    "519", "548", "579", "581", "584", "587", "604", "613", "639", "647", "672", "683", "705",
    "709", "742", "753", "778", "780", "782", "807", "819", "825", "867", "873", "879", "902",
    "905", "942",
];

/// A text that is only a help keyword, with the language of its reply (D-066, D-131).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    /// HELP or INFO (English) or AYUDA (Spanish).
    Help(Language),
}

/// Why [`HelpSettings::new`] refused a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HelpSettingsError {
    /// A reply time of zero days or zero hours promises nothing real.
    #[error("a reply time must be at least 1")]
    ZeroTime,
    /// The help line is the texting number: HELP replies say that number is text-only (CON-018).
    #[error("the help phone can never be the texting number")]
    HelpPhoneIsTextingNumber,
    /// A phone setting is not a US or Canadian number.
    #[error("the phone setting is not a valid number")]
    BadPhone,
}

/// The settings a HELP reply is built from (CON-018, OPS-026). Fields are private so every value
/// has passed [`HelpSettings::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSettings {
    days: u32,
    urgent_hours: u32,
    away_until: Option<String>,
}

impl HelpSettings {
    /// Checks the reply-time settings (`days` for an ordinary help request, `urgent_hours` for an
    /// urgent one), the owner's away date (`away_until`, already written as the caller wants it
    /// shown, for example "Nov 12"; `None` while the away setting is off) and the help phone
    /// (`help_phone`, `None` while no separate help line is set). No voice number is ever put in
    /// the reply while no help line exists; the help phone may never be `texting_number`.
    ///
    /// # Errors
    ///
    /// [`HelpSettingsError::ZeroTime`], [`HelpSettingsError::BadPhone`] or
    /// [`HelpSettingsError::HelpPhoneIsTextingNumber`].
    pub fn new(
        (days, urgent_hours): (u32, u32),
        away_until: Option<&str>,
        (help_phone, texting_number): (Option<&str>, &str),
    ) -> Result<Self, HelpSettingsError> {
        if days == 0 || urgent_hours == 0 {
            return Err(HelpSettingsError::ZeroTime);
        }
        let texting = normalize_phone(texting_number).map_err(|_| HelpSettingsError::BadPhone)?;
        if let Some(phone) = help_phone {
            let phone = normalize_phone(phone).map_err(|_| HelpSettingsError::BadPhone)?;
            if phone == texting {
                return Err(HelpSettingsError::HelpPhoneIsTextingNumber);
            }
        }
        Ok(Self {
            days,
            urgent_hours,
            away_until: away_until.map(str::to_owned),
        })
    }

    /// The `{reply_time}` phrase: "2 days, or within 24 hours if your job is urgent" (Spanish:
    /// "2 días, o en 24 horas si su trabajo es urgente").
    fn reply_time(&self, language: Language) -> String {
        let (d, h) = (self.days, self.urgent_hours);
        match language {
            Language::En => {
                let day = if d == 1 { "day" } else { "days" };
                let hour = if h == 1 { "hour" } else { "hours" };
                format!("{d} {day}, or within {h} {hour} if your job is urgent")
            }
            Language::Es => {
                let day = if d == 1 { "día" } else { "días" };
                let hour = if h == 1 { "hora" } else { "horas" };
                format!("{d} {day}, o en {h} {hour} si su trabajo es urgente")
            }
        }
    }
}

/// Whether `text` is only HELP, INFO or AYUDA (CON-018, D-131): case, surrounding spaces and
/// punctuation do not matter; any other word makes it a request ("I need help with my sink").
#[must_use]
pub fn keyword(text: &str) -> Option<Keyword> {
    let word = text
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    match word.as_str() {
        "help" | "info" => Some(Keyword::Help(Language::En)),
        "ayuda" => Some(Keyword::Help(Language::Es)),
        _ => None,
    }
}

/// Whether `phone` is a +1 number in a Canadian area code (roaming line, CON-018).
fn is_canadian(phone: &str) -> bool {
    normalize_phone(phone).is_ok_and(|p| {
        let digits: String = p.chars().filter(char::is_ascii_digit).collect();
        let ten = digits.strip_prefix('1').unwrap_or(&digits);
        CANADIAN_AREA_CODES.contains(&ten.get(..3).unwrap_or(""))
    })
}

/// The HELP reply for a number `phone` in `language` (CON-018): the registry text for the away
/// setting and the roaming line, filled from `settings`. The Spanish text stays on while the
/// Spanish setting is off (TXT-039).
///
/// # Errors
///
/// [`OutboxError::Template`] if the compiled-in texts are broken.
pub fn help_text(
    settings: &HelpSettings,
    language: Language,
    phone: &str,
) -> Result<String, OutboxError> {
    let canadian = is_canadian(phone);
    let id = match (settings.away_until.is_some(), canadian) {
        (false, false) => "help_reply",
        (false, true) => "help_reply_ca",
        (true, false) => "help_reply_away",
        (true, true) => "help_reply_away_ca",
    };
    let reply_time = settings.reply_time(language);
    let away = settings.away_until.as_deref().unwrap_or_default();
    let values = [("reply_time", reply_time.as_str()), ("time", away)];
    Registry::embedded()
        .and_then(|r| r.render(id, language, false, &values))
        .map(|text| text.body)
        .map_err(|_| OutboxError::Template)
}

/// Queues the HELP reply for `msg` if its text is only HELP, INFO or AYUDA, whether or not the
/// number is on the suppression list (CON-013, CON-018, D-131); returns its outbox row, or `None`
/// for any other text. One reply per inbound message: a repeat of the same message queues no
/// second one (TXT-011). The row goes out through `optout::send_checked` at any hour; the
/// caller sends it at once so it is delivered within 5 minutes.
///
/// # Errors
///
/// [`OutboxError::Template`], [`OutboxError::PastCustomer`] or [`OutboxError::Database`].
pub async fn reply_to_help(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &InboundText<'_>,
    settings: &HelpSettings,
) -> Result<Option<Uuid>, OutboxError> {
    let Some(Keyword::Help(language)) = keyword(msg.text) else {
        return Ok(None);
    };
    let body = help_text(settings, language, msg.phone)?;
    let text = NewText {
        template_id: HELP_TEMPLATE,
        subject: &[msg.inbox_id],
        recipient: msg.contact,
        relation: msg.relation,
        body: &body,
    };
    let (id, _) = service::enqueue(db, secret, &text).await?;
    Ok(Some(id))
}
