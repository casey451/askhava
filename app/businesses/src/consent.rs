//! A business's notice consent and the messages around it (CON-019, CON-020, BIZ-017, BIZ-032,
//! BIZ-048; D-191, D-312, D-344, D-374, D-491, D-492).
//!
//! [`consent_box`] is the unticked E-SIGN box: its wording names each channel it covers and says
//! Ask Hava never places recorded or automated calls; [`record_consent`] stores one row per
//! channel with the wording version and time (a text row also names the phone), and refuses an
//! unticked box. A phone gets texts only after a current member ticked the text box for that
//! phone ([`may_text`]); no call consent exists. On
//! activation [`request_welcome`] queues the welcome email once; [`welcome_sent`] marks it gone and
//! [`may_get_cards`] stays false until then. [`phone_stopped`] handles an opt-out from a business
//! phone: the account keeps its status and the owner is emailed [`stopped_email_line`].
//! [`auto_reply_due`] says when a business text gets [`AUTO_REPLY_TEMPLATE`]. Recruiting is by
//! email only: [`recruiting_email`] opens with [`advertisement_line`], and [`stop_recruiting`] is
//! honored at once by [`may_recruit`]. No contact value is stored here, only opaque IDs and keyed
//! digests (PRIV-001).

use askhava_core::api::{NewTask, brand, enqueue};
use askhava_messaging::api::{FROM, MailboxKind, PostalAddress, detect_opt_out, keyword};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

/// The wording version stored with each consent row (CON-019); bump it when the wording changes.
pub const CONSENT_VERSION: &str = "con019-v1";
/// CON-019, D-344, WORD-001: the line every consent box carries, naming the brand's display name
/// (D-1932).
#[must_use]
pub fn no_calls_line() -> String {
    format!(
        "{} never places recorded or automated calls to you.",
        brand().display_name()
    )
}
/// The `core.task` kind that sends the CON-020 welcome email.
pub const WELCOME_KIND: &str = "businesses.welcome";
/// The `core.task` kind that emails the owner after a STOP from a business phone (BIZ-017).
pub const PHONE_STOPPED_KIND: &str = "businesses.phone_stopped";
/// BIZ-032: the reply template for a business text the app does not expect.
pub const AUTO_REPLY_TEMPLATE: &str = "business_auto_reply";
/// BIZ-048, WORD-001: the first line of every recruiting email, naming the brand's display name
/// (D-1932).
#[must_use]
pub fn advertisement_line() -> String {
    format!(
        "This email is an advertisement for {}.",
        brand().display_name()
    )
}
const BUSINESS: &str = "businesses.business";
const MEMBER: &str = "businesses.team_member";
const INBOX: &str = "messaging.inbox";

/// The site's root on the brand's domain, `https://<domain>/` (D-1932).
fn site() -> String {
    format!("https://{}/", brand().domain())
}

/// A channel a consent box covers. There is no call channel (BIZ-039, D-344).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentChannel {
    /// Job and account notices by email.
    Email,
    /// Job-card texts to the one phone the box was ticked for (BIZ-036, D-191).
    Text,
}

impl ConsentChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Text => "text",
        }
    }
}

/// The consent box as a form shows it: always unticked at first (CON-019, legal brain L3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentBox {
    /// The wording beside the box.
    pub wording: String,
    /// The wording version stored when it is ticked.
    pub version: &'static str,
    /// Whether the box is ticked; [`consent_box`] gives `false`.
    pub ticked: bool,
}

/// Why a consent, welcome, stop or recruiting call failed. No variant carries a value (R5).
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum ConsentError {
    /// The box was not ticked, or names no channel.
    #[error("the consent box was not ticked")]
    NotTicked,
    /// The member is not a current member of that business.
    #[error("unknown team member")]
    UnknownMember,
    /// A text box names no phone, or a phone not linked to that business.
    #[error("unknown phone")]
    UnknownPhone,
    /// A recruiting email's subject is empty or looks like a reply or a notice.
    #[error("the recruiting subject is not honest")]
    Subject,
    /// The stop link is not an askhava.com link, or the postal address is not a mailbox.
    #[error("the recruiting footer is not valid")]
    Footer,
    /// The digest is not 64 lowercase hex characters.
    #[error("bad address key")]
    BadKey,
    /// A database call failed.
    #[error("database error")]
    Database,
    /// The audit row could not be written.
    #[error("audit error")]
    Audit,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, ConsentError> {
    r.map_err(|_| ConsentError::Database)
}

/// CON-019, D-312, D-344: the box's wording for `channels`, naming each one and saying no calls.
#[must_use]
pub fn consent_wording(channels: &[ConsentChannel]) -> String {
    let name = brand().display_name();
    let mut lines = Vec::new();
    if channels.contains(&ConsentChannel::Email) {
        lines.push(format!("I agree to get job and account notices from {name} by email at the address above, not on paper."));
    }
    if channels.contains(&ConsentChannel::Text) {
        lines.push(format!("I agree to get job-card texts from {name} at the phone number above. Message frequency varies. Message and data rates may apply. Text STOP to stop or HELP for help."));
    }
    lines.push(no_calls_line());
    lines.push("You can change this any time in your portal.".to_owned());
    lines.join(" ")
}

/// CON-019: the unticked box for `channels` (sign-up: email, plus text when chosen; a member's
/// phone or a later text choice: text alone).
#[must_use]
pub fn consent_box(channels: &[ConsentChannel]) -> ConsentBox {
    ConsentBox {
        wording: consent_wording(channels),
        version: CONSENT_VERSION,
        ticked: false,
    }
}

/// CON-019, D-191: stores the ticked box of `member` of `business`, one row per channel with
/// [`CONSENT_VERSION`] and `now` (the text row naming `phone`, a linked `businesses.staff_phone` of
/// that business), and one audit row, in one transaction. A box already stored for the same
/// member, channel, phone and version adds no row.
///
/// # Errors
/// [`ConsentError::NotTicked`] (also for no channel), `UnknownMember`, `UnknownPhone` (a text box
/// with no phone or another business's phone), `Database` or `Audit`.
pub async fn record_consent(
    conn: &mut PgConnection,
    (business, member, phone): (Uuid, Uuid, Option<Uuid>),
    channels: &[ConsentChannel],
    ticked: bool,
    now: Timestamp,
) -> Result<(), ConsentError> {
    if !ticked || channels.is_empty() {
        return Err(ConsentError::NotTicked);
    }
    let mut tx = db(conn.begin().await)?;
    let current = sqlx::query_scalar!(
        "SELECT 1 AS \"one!\" FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL",
        member,
        business
    );
    db(current.fetch_optional(&mut *tx).await)?.ok_or(ConsentError::UnknownMember)?;
    let text_phone = if channels.contains(&ConsentChannel::Text) {
        Some(linked_phone(&mut tx, business, phone).await?)
    } else {
        None
    };
    let mut added = 0;
    for c in [ConsentChannel::Email, ConsentChannel::Text] {
        if channels.contains(&c) {
            let row_phone = text_phone.filter(|_| c == ConsentChannel::Text);
            added += insert_consent(&mut tx, (business, member, row_phone), c, now).await?;
        }
    }
    if added > 0 {
        log(
            &mut tx,
            (business, MEMBER),
            "businesses.consent_recorded",
            member,
        )
        .await?;
    }
    db(tx.commit().await)
}

async fn linked_phone(
    tx: &mut PgConnection,
    business: Uuid,
    phone: Option<Uuid>,
) -> Result<Uuid, ConsentError> {
    let phone = phone.ok_or(ConsentError::UnknownPhone)?;
    let q = sqlx::query_scalar!(
        "SELECT 1 AS \"one!\" FROM businesses.staff_phone WHERE id = $1 AND business_id = $2",
        phone,
        business
    );
    db(q.fetch_optional(tx).await)?.ok_or(ConsentError::UnknownPhone)?;
    Ok(phone)
}

async fn insert_consent(
    tx: &mut PgConnection,
    (business, member, phone): (Uuid, Uuid, Option<Uuid>),
    channel: ConsentChannel,
    now: Timestamp,
) -> Result<u64, ConsentError> {
    let q = sqlx::query!(
        "INSERT INTO businesses.esign_consent \
         (business_id, member_id, channel, line_id, wording_version, consented_at) \
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
        business,
        member,
        channel.as_str(),
        phone,
        CONSENT_VERSION,
        now.to_sqlx() as _
    );
    Ok(db(q.execute(tx).await)?.rows_affected())
}

/// CON-019, BIZ-048, D-191: whether `phone`, a linked phone of `business`, has a text box ticked
/// for it by a current member; no text of any kind goes to a business phone without it, and a
/// relinked phone (a new ID) needs a new box. The suppression list still applies after a STOP
/// (CON-013).
///
/// # Errors
/// [`ConsentError::Database`].
pub async fn may_text(
    conn: &mut PgConnection,
    business: Uuid,
    phone: Uuid,
) -> Result<bool, ConsentError> {
    let q = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.esign_consent c \
         JOIN businesses.team_member m ON m.id = c.member_id AND m.business_id = c.business_id \
         JOIN businesses.staff_phone p ON p.id = c.line_id AND p.business_id = c.business_id \
         WHERE c.business_id = $1 AND c.line_id = $2 AND c.channel = 'text' \
         AND m.removed_at IS NULL) AS \"yes!\"",
        business,
        phone
    );
    db(q.fetch_one(conn).await)
}

async fn log(
    tx: &mut PgConnection,
    (business, subject_kind): (Uuid, &'static str),
    action: &'static str,
    subject: Uuid,
) -> Result<(), ConsentError> {
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action,
        subject_kind,
        subject_id: subject,
        change: Change::Action,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| ConsentError::Audit)
}

async fn queue(
    tx: &mut PgConnection,
    kind: &'static str,
    (payload, key): (serde_json::Value, String),
    now: Timestamp,
) -> Result<(), ConsentError> {
    let mut task = NewTask::new(kind, payload, now);
    task.dedupe_key = Some(key);
    enqueue(tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| ConsentError::Database)
}

/// CON-020, D-312: on activation, records the welcome request and queues one [`WELCOME_KIND`]
/// task, with its audit row; `true` only the first time and only for an active business.
///
/// # Errors
/// [`ConsentError::Database`] or `Audit`.
pub async fn request_welcome(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<bool, ConsentError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query!(
        "INSERT INTO businesses.welcome (business_id, requested_at) \
         SELECT id, $2 FROM businesses.business WHERE id = $1 AND status = 'active' \
         ON CONFLICT (business_id) DO NOTHING",
        business,
        now.to_sqlx() as _
    );
    if db(q.execute(&mut *tx).await)?.rows_affected() == 0 {
        return Ok(false);
    }
    let payload = json!({"business": business});
    let key = format!("{WELCOME_KIND}.{business}");
    queue(&mut tx, WELCOME_KIND, (payload, key), now).await?;
    log(
        &mut tx,
        (business, BUSINESS),
        "businesses.welcome_requested",
        business,
    )
    .await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// CON-020: the sender marks the welcome email gone; `true` only the first time.
///
/// # Errors
/// [`ConsentError::Database`].
pub async fn welcome_sent(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<bool, ConsentError> {
    let q = sqlx::query!(
        "UPDATE businesses.welcome SET sent_at = $2 WHERE business_id = $1 AND sent_at IS NULL",
        business,
        now.to_sqlx() as _
    );
    Ok(db(q.execute(conn).await)?.rows_affected() == 1)
}

/// CON-020, D-312: whether the welcome email has gone out, so a job card may go to `business`.
///
/// # Errors
/// [`ConsentError::Database`].
pub async fn may_get_cards(conn: &mut PgConnection, business: Uuid) -> Result<bool, ConsentError> {
    let q = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.welcome \
         WHERE business_id = $1 AND sent_at IS NOT NULL) AS \"yes!\"",
        business
    );
    db(q.fetch_one(conn).await)
}

/// CON-020, SEC-025, BIZ-042, BIZ-046, D-344: the welcome email's lines, in order. They hold for
/// every channel choice, so they never say cards come only by email (BIZ-036).
#[must_use]
pub fn welcome_lines() -> Vec<String> {
    let (name, domain) = (brand().display_name(), brand().domain());
    vec![
        format!("Welcome to {name}. Your business is active and can now get job cards."),
        format!("Our emails come from {FROM}. Add it to your contacts so job cards reach your inbox."),
        "If we land in spam or junk: in Gmail, tap Not spam, add us to contacts, make a filter for our address with \"Never send it to Spam\" and move us to Primary. In Outlook, tap Not junk, then add us to Safe senders. In Yahoo, tap Not spam. In iPhone Mail, add us to VIP. In any other email app, add us to your contacts.".to_owned(),
        format!("Save the attached contact card, \"{name} homeowner calls\". Calls from these numbers are homeowners who chose your quote, calling on an {name} masked number. {name} never calls you."),
        format!("To change how you get notices, sign in at {}b.", site()),
        "If you choose texts, text STOP from a phone to stop texts to that phone.".to_owned(),
        format!("{name} emails come only from addresses ending in {domain}, never ask for your card or a code, and every button goes to {domain}."),
    ]
}

/// BIZ-017, D-311, D-344: after `messaging` listed a phone of `business` that texted an opt-out
/// (inbox row `inbox`), queues one [`PHONE_STOPPED_KIND`] email to the owner, with its audit row.
/// The account keeps its status and stays active on email; the phone's texts stop through the
/// suppression list alone. The channel move for a text-only business is T147's (D-491).
///
/// # Errors
/// [`ConsentError::Database`] or `Audit`.
pub async fn phone_stopped(
    conn: &mut PgConnection,
    business: Uuid,
    inbox: Uuid,
    now: Timestamp,
) -> Result<(), ConsentError> {
    let mut tx = db(conn.begin().await)?;
    let payload = json!({"business": business, "inbox": inbox});
    let key = format!("{PHONE_STOPPED_KIND}.{inbox}");
    queue(&mut tx, PHONE_STOPPED_KIND, (payload, key), now).await?;
    log(
        &mut tx,
        (business, INBOX),
        "businesses.phone_stopped",
        inbox,
    )
    .await?;
    db(tx.commit().await)
}

/// BIZ-017: the owner's email after a STOP from the phone ending `last4`.
#[must_use]
pub fn stopped_email_line(last4: &str) -> String {
    format!(
        "{}: Texts to the phone ending {last4} are off because it texted STOP. Text START from it to turn them back on.",
        brand().short_name()
    )
}

/// Business words the app reads elsewhere (PICK-033, PICK-043, BIZ-028, BIZ-034, BIZ-038, CON-015,
/// CALL-011), each alone or with one number, as `picks`' parsers accept them.
const COMMAND_WORDS: [&str; 18] = [
    "confirm",
    "confirmar",
    "yes",
    "si",
    "sí",
    "hold",
    "espera",
    "away",
    "ausente",
    "pause",
    "pausa",
    "resume",
    "reanudar",
    "busy",
    "ocupado",
    "start",
    "unstop",
    "link",
];

/// Whether the whole of `words` is a command: a command word alone or followed by one whole
/// number ("YES 1042.", "BUSY 5"), trailing periods and exclamation marks dropped, any case.
fn is_command(words: &str) -> bool {
    let mut parts = words
        .trim_end()
        .trim_end_matches(['.', '!'])
        .split_whitespace();
    let Some(word) = parts.next() else {
        return false;
    };
    let number_ok = parts.next().is_none_or(|n| n.parse::<u32>().is_ok());
    number_ok && parts.next().is_none() && COMMAND_WORDS.contains(&word.to_lowercase().as_str())
}

/// BIZ-032, D-228, D-491: whether a business text gets [`AUTO_REPLY_TEMPLATE`]: not when the
/// caller `expected` it (a quote or a reply the app asked for), and never for HELP, an opt-out or
/// a text that is wholly a command; anything else gets it, a question or a charge word included,
/// even when it starts with a command word ("Yes, why was I charged?"). An empty text (a photo
/// alone) is not expected either, so it gets the reply pointing to the portal (D-1717).
#[must_use]
pub fn auto_reply_due(words: &str, expected: bool) -> bool {
    if expected || keyword(words).is_some() || detect_opt_out(words).is_some() {
        return false;
    }
    !is_command(words)
}

/// A recruiting email's subject and lines (BIZ-048, SEC-028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecruitingEmail {
    /// The subject as given, checked honest.
    pub subject: String,
    /// [`advertisement_line`], the body, the stop link and the postal address.
    pub lines: Vec<String>,
}

/// BIZ-048, D-374, D-492: builds a recruiting email: an honest subject (not empty, not a reply,
/// forward or job notice), the advertisement line first, the stop link and Ask Hava LLC's postal
/// mailbox last.
///
/// # Errors
/// [`ConsentError::Subject`] or [`ConsentError::Footer`].
pub fn recruiting_email(
    subject: &str,
    body: &[&str],
    (postal, kind): (&str, MailboxKind),
    stop_url: &str,
) -> Result<RecruitingEmail, ConsentError> {
    let s = subject.trim().to_lowercase();
    let opening = format!("{}:", brand().short_name().to_lowercase());
    let notice = ["re:", "fwd:", "fw:", opening.as_str()]
        .iter()
        .any(|p| s.starts_with(p));
    if s.is_empty() || notice || s.contains("job card") || s.contains("your account") {
        return Err(ConsentError::Subject);
    }
    PostalAddress::new(postal, kind).map_err(|_| ConsentError::Footer)?;
    if !stop_url.starts_with(&site()) {
        return Err(ConsentError::Footer);
    }
    let mut lines = vec![advertisement_line()];
    lines.extend(body.iter().map(|l| (*l).to_owned()));
    lines.push(format!("To stop these emails: {stop_url}"));
    lines.push(format!("{}, {}", brand().legal_name(), postal.trim()));
    Ok(RecruitingEmail {
        subject: subject.trim().to_owned(),
        lines,
    })
}

fn check_key(key: &str) -> Result<(), ConsentError> {
    let hex = key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    hex.then_some(()).ok_or(ConsentError::BadKey)
}

/// BIZ-048: the address with keyed digest `key` gets no further recruiting email, from `now`.
///
/// # Errors
/// [`ConsentError::BadKey`] or [`ConsentError::Database`].
pub async fn stop_recruiting(
    conn: &mut PgConnection,
    key: &str,
    now: Timestamp,
) -> Result<(), ConsentError> {
    check_key(key)?;
    let q = sqlx::query!(
        "INSERT INTO businesses.recruit_stop (mailbox_key, stopped_at) VALUES ($1, $2) \
         ON CONFLICT (mailbox_key) DO NOTHING",
        key,
        now.to_sqlx() as _
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-048: whether a recruiting email may go to the address with keyed digest `key`.
///
/// # Errors
/// [`ConsentError::BadKey`] or [`ConsentError::Database`].
pub async fn may_recruit(conn: &mut PgConnection, key: &str) -> Result<bool, ConsentError> {
    check_key(key)?;
    let q = sqlx::query_scalar!(
        "SELECT NOT EXISTS (SELECT 1 FROM businesses.recruit_stop WHERE mailbox_key = $1) \
         AS \"yes!\"",
        key
    );
    db(q.fetch_one(conn).await)
}
