//! Business sign-up (BIZ-001, BIZ-019, BIZ-020, BIZ-021, BIZ-024, BIZ-026, BIZ-027, BIZ-030,
//! BIZ-031, BIZ-053, FEE-011, TERMS-016, TERMS-033; D-145, D-175, D-176, D-457, D-492, D-493).
//!
//! Sign-up starts from the category list, each "Open now" or "Coming later" by `licensing`'s
//! `launch_open` map (LIC-046), before any license number; only coming-later choices put the
//! business on the waitlist. It needs an email (by its `privacy` mailbox ID), asks the optional
//! source question, stores the business and its sign-up row with an audit row in one transaction,
//! and then, outside it, creates the Stripe customer with no card (R11). The identity chain and
//! activation are in `identity`.
//!
//! T193 adds what the sign-up screens need from the records: the masked record contact and its
//! kind ([`MaskedContact`], BIZ-026), the "That number is old" wait that the next daily list with
//! a new record contact ends ([`number_is_old`], [`resume_on_new_record`]), the 4-day review
//! target ([`review_target`], BIZ-027), the plain-words fee summary ([`fee_summary`], BIZ-030),
//! the trade count of the done screen ([`trade_count`]) and the one launch notice's sent-once
//! claim ([`claim_launch_notice`], BIZ-031). The screens themselves are `portals`'.

use askhava_adapters::api::PaymentsAdapter;
use askhava_core::api::SettingsError;
use askhava_licensing::api::{ClassMap, Opening};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_payments::api::{
    BusinessId, EntityKey, EntityKind, ensure_customer, record_entity_keys,
};
use jiff::{SignedDuration, Timestamp, tz};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::about_us::AboutUsRefusal;
use crate::identity::{RecordContacts, Step};
use crate::models::{self, BusinessesError, db};
use crate::status::AccountStatus;

/// The waitlist line shown when only coming-later categories were chosen (BIZ-021, D-175, D-288).
pub const WAITLIST_LINE: &str = "The trades you picked aren't open on Ask Hava yet, so you're on the \
waitlist. Your join date is kept. No priority; everyone waiting is told at once.";

/// Why a sign-up step failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignupError {
    /// No sign-up email was given (BIZ-019).
    #[error("sign-up needs an email")]
    NoEmail,
    /// No category, or one that is not offered (BIZ-021).
    #[error("sign-up needs open or coming-later categories")]
    BadCategories,
    /// The `g` code is not a short lower-case code (BIZ-053).
    #[error("bad source code")]
    BadSourceCode,
    /// Twilio Verify refused or failed (BIZ-002).
    #[error("identity code could not be sent or checked")]
    Verify,
    /// The free-win entity keys could not be stored (FEE-011).
    #[error("entity keys not stored")]
    EntityKeys,
    /// The About us line was refused, with its reason (BIZ-022).
    #[error("about us line refused")]
    AboutUs(AboutUsRefusal),
    /// A records call failed.
    #[error(transparent)]
    Businesses(#[from] BusinessesError),
}

/// "Where did you hear about Ask Hava?" (BIZ-053): never shown, never routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeardFrom {
    /// A Facebook group.
    FacebookGroup,
    /// Another business.
    AnotherBusiness,
    /// A friend or neighbor.
    FriendOrNeighbor,
    /// Met the owner.
    MetTheOwner,
    /// Search.
    Search,
    /// Other.
    Other,
}

impl HeardFrom {
    /// The stored code in `businesses.signup.heard_from`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::FacebookGroup => "facebook_group",
            Self::AnotherBusiness => "another_business",
            Self::FriendOrNeighbor => "friend_or_neighbor",
            Self::MetTheOwner => "met_the_owner",
            Self::Search => "search",
            Self::Other => "other",
        }
    }
}

/// BIZ-021, LIC-046, D-176: the sign-up category list, in the order given, each with its "Open
/// now" or "Coming later" label; switched-off categories are left out. Pest control is a plain
/// item like any other (LIC-012 limits who may tick it later). A repeated code is listed once.
#[must_use]
pub fn category_list(map: &ClassMap, codes: &[&str]) -> Vec<(String, &'static str)> {
    let label = |o: Opening| match o {
        Opening::OpenNow => Some("Open now"),
        Opening::ComingLater => Some("Coming later"),
        Opening::Off => None,
    };
    let mut seen: Vec<&str> = Vec::new();
    let labelled = codes.iter().filter_map(|c| {
        let first = !seen.contains(c);
        seen.push(c);
        first.then(|| (*c, label(map.opening(c))))
    });
    labelled
        .filter_map(|(c, l)| Some((c.to_owned(), l?)))
        .collect()
}

/// What the person entered on the first sign-up screens (BIZ-019, BIZ-021, BIZ-053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignUpForm {
    /// The chosen category codes.
    pub categories: Vec<String>,
    /// The sign-up email's `privacy` mailbox ID; required (BIZ-019).
    pub mailbox_id: Option<Uuid>,
    /// The optional source answer.
    pub heard_from: Option<HeardFrom>,
    /// The `g` code of the askhava.com link the person arrived on, if any.
    pub g_code: Option<String>,
}

/// BIZ-019, BIZ-021, BIZ-053: checks the form; `true` when every choice is coming later, so the
/// business joins the waitlist.
///
/// # Errors
/// [`SignupError::NoEmail`], [`SignupError::BadCategories`] or [`SignupError::BadSourceCode`].
pub fn waitlisted(map: &ClassMap, form: &SignUpForm) -> Result<bool, SignupError> {
    if form.mailbox_id.is_none() {
        return Err(SignupError::NoEmail);
    }
    let g_ok = |g: &String| {
        (1..=32).contains(&g.len())
            && g.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && g.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    };
    if !form.g_code.as_ref().is_none_or(g_ok) {
        return Err(SignupError::BadSourceCode);
    }
    let mut codes: Vec<&str> = form.categories.iter().map(String::as_str).collect();
    codes.sort_unstable();
    codes.dedup();
    let listed = category_list(map, &codes);
    if listed.is_empty() || listed.len() != codes.len() {
        return Err(SignupError::BadCategories);
    }
    Ok(listed.iter().all(|(_, l)| *l == "Coming later"))
}

/// The new business and whether its Stripe customer exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedUp {
    /// The business record ID.
    pub business: Uuid,
    /// On the waitlist (show [`WAITLIST_LINE`]).
    pub waitlist: bool,
    /// The Stripe customer was made; when false the caller retries `ensure_customer` (BIZ-001).
    pub customer_ready: bool,
}

/// BIZ-001, BIZ-019, BIZ-021, BIZ-053: records the sign-up, then creates the Stripe customer with
/// no card (FEE-036), after the transaction commits (R11).
///
/// # Errors
/// The [`waitlisted`] refusals, or [`SignupError::Businesses`].
pub async fn sign_up(
    pool: &PgPool,
    (stripe, id_secret): (&impl PaymentsAdapter, &SecretString),
    map: &ClassMap,
    form: &SignUpForm,
    now: Timestamp,
) -> Result<SignedUp, SignupError> {
    let waitlist = waitlisted(map, form)?;
    let mailbox = form.mailbox_id.ok_or(SignupError::NoEmail)?;
    let status = if waitlist {
        AccountStatus::Waitlist
    } else {
        AccountStatus::Pending
    };
    let mut conn = db(pool.acquire().await)?;
    let mut tx = db(conn.begin().await)?;
    let business = models::insert(&mut tx, status, now).await?;
    let heard = form.heard_from.map(HeardFrom::code);
    let row = (mailbox, heard, form.g_code.as_deref(), now);
    models::insert_signup(&mut tx, business, row).await?;
    set_trades(&mut tx, business, &form.categories).await?;
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action: "businesses.signed_up",
        subject_kind: "businesses.business",
        subject_id: business,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| BusinessesError::Audit)?;
    db(tx.commit().await)?;
    drop(conn);
    let customer = ensure_customer(pool, stripe, id_secret, BusinessId(business)).await;
    Ok(SignedUp {
        business,
        waitlist,
        customer_ready: customer.is_ok(),
    })
}

/// The FEE-011 free-win entity values known at sign-up, handed to `payments` as keyed digests.
#[derive(Debug, Default)]
pub struct EntityValues<'a> {
    /// The ROC license number.
    pub roc: Option<&'a str>,
    /// The entity on the city license list.
    pub city_entity: Option<&'a str>,
    /// The identity-code contact from the record, in its stored form.
    pub identity_contact: Option<&'a SecretString>,
    /// The owner name on the city list.
    pub owner_name: Option<&'a SecretString>,
}

/// FEE-011, D-151: hands the sign-up's entity keys to `payments::api::record_entity_keys`.
///
/// # Errors
/// [`SignupError::EntityKeys`].
pub async fn hand_entity_keys(
    db: &mut PgConnection,
    secret: &SecretString,
    business: Uuid,
    v: &EntityValues<'_>,
) -> Result<(), SignupError> {
    use secrecy::ExposeSecret as _;
    let values = [
        (EntityKind::Roc, v.roc),
        (EntityKind::CityEntity, v.city_entity),
        (
            EntityKind::IdentityContact,
            v.identity_contact.map(|s| s.expose_secret()),
        ),
        (
            EntityKind::OwnerName,
            v.owner_name.map(|s| s.expose_secret()),
        ),
    ];
    let keys = values
        .into_iter()
        .filter_map(|(k, val)| val.map(|val| EntityKey::new(secret, k, val)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SignupError::EntityKeys)?;
    let stored = record_entity_keys(db, BusinessId(business), &keys).await;
    stored.map_err(|_| SignupError::EntityKeys)
}

/// The business terms as shown at sign-up (TERMS-033): document, version, the exact published
/// text, language (`en` or `es`) and the assent line beside the button.
#[derive(Debug, Clone, Copy)]
pub struct TermsShown<'a> {
    /// The document code, such as `business_terms`.
    pub document: &'a str,
    /// The version.
    pub version: &'a str,
    /// The exact published text, stored as its SHA256 digest.
    pub published_text: &'a str,
    /// `en` or `es`.
    pub language: &'a str,
    /// The assent line shown.
    pub assent_line: &'a str,
}

/// TERMS-016, TERMS-033, D-492: stores one business terms acceptance with channel "business
/// sign-up" and the business person's ID; append-only.
///
/// # Errors
/// [`BusinessesError::Database`] (also for a value the table's checks refuse).
pub async fn accept_terms(
    db: &mut PgConnection,
    (business, person): (Uuid, Uuid),
    t: TermsShown<'_>,
    now: Timestamp,
) -> Result<Uuid, BusinessesError> {
    let sha: String = Sha256::digest(t.published_text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let fields = [t.document, t.version, &sha, t.language, t.assent_line];
    models::insert_terms(db, (business, person, fields, now)).await
}

/// BIZ-020: needs-review sign-ups due their status email: never told, or told 7 days ago or more.
/// The caller sends the status (never a code) and then calls [`review_told`].
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn review_due(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Vec<Uuid>, BusinessesError> {
    models::review_due(db, now).await
}

/// BIZ-020: records that the waiting business was told its status at `now`.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn review_told(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    models::set_review_told(db, business, now).await
}

/// BIZ-021, BIZ-031: stores the sign-up's chosen category codes, sorted and once each, for the
/// waitlist done screen's count.
async fn set_trades(
    conn: &mut PgConnection,
    business: Uuid,
    categories: &[String],
) -> Result<(), BusinessesError> {
    let mut trades = categories.to_vec();
    trades.sort_unstable();
    trades.dedup();
    let q = sqlx::query!(
        "UPDATE businesses.signup SET trades = $2 WHERE business_id = $1",
        business,
        &trades
    );
    db(q.execute(conn).await).map(|_| ())
}

/// BIZ-024, D-179, D-288: the line every sign-up screen shows; the code still goes only to the
/// record contact.
pub const SOMEONE_ELSE_LINE: &str = "Someone else filling this in? Ask the owner to read you the \
6 digits, or forward the text.";

/// BIZ-026: the button that saves the sign-up until the record shows a new contact.
pub const NUMBER_OLD_BUTTON: &str = "That number is old";

/// BIZ-026, D-254: the button before a letter that saves the sign-up instead of mailing.
pub const STOP_FIX_BUTTON: &str = "Stop, I'll fix my record first";

/// BIZ-026, D-254: the `messaging` template sent when a daily list shows a new record contact.
pub const RECORD_UPDATED_TEMPLATE: &str = "signup_record_updated";

/// BIZ-030, WORD-002, D-356: the business statement of the plain-words box.
pub const BUSINESS_STATEMENT: &str = "Ask Hava is not a contractor. It passes along this request \
and the homeowner's choice. If the homeowner picks you, they hire you directly; Ask Hava doesn't \
direct, schedule, supervise or pay for the work.";

/// BIZ-030, FEE-013: days after a job closes that repeat picks from the same homeowner are free.
pub const REPEAT_FREE_DAYS: u32 = 180;

/// The kind of record contact a code would go to (BIZ-026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    /// A record phone that gets texts.
    Mobile,
    /// A record phone that is a landline: never called (BIZ-046, D-366).
    Landline,
    /// The record email.
    Email,
    /// The record mailing address.
    MailingAddress,
}

impl ContactKind {
    /// The words the screen uses for the kind.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Mobile => "mobile phone",
            Self::Landline => "landline",
            Self::Email => "email",
            Self::MailingAddress => "mailing address",
        }
    }
}

/// A record contact as sign-up shows it before any code (BIZ-026): its kind and a masked value.
/// It holds no full contact: a phone shows its last 4 digits, an email its first letter and
/// domain, an address its town and ZIP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedContact {
    kind: ContactKind,
    shown: String,
}

impl MaskedContact {
    /// BIZ-026: the record contact the chain's `step` sends to; `place` is the record address's
    /// town and ZIP. `None` when the record has no contact for the step.
    #[must_use]
    pub fn for_step(record: &RecordContacts, step: Step, place: (&str, &str)) -> Option<Self> {
        match step {
            Step::Text => record.texting_phone.as_ref().map(|p| Self::phone(p, false)),
            Step::Email => record.email.as_ref().map(Self::email),
            Step::Letter => record.has_address.then(|| Self {
                kind: ContactKind::MailingAddress,
                shown: format!("{} {}", place.0, place.1),
            }),
        }
    }

    /// BIZ-026, BIZ-046: a record phone, masked; `landline` marks one that is never texted or called.
    #[must_use]
    pub fn phone(value: &SecretString, landline: bool) -> Self {
        let digits: Vec<char> = value
            .expose_secret()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        let last: String = digits[digits.len().saturating_sub(4)..].iter().collect();
        let kind = if landline {
            ContactKind::Landline
        } else {
            ContactKind::Mobile
        };
        Self {
            kind,
            shown: format!("(•••) •••-{last}"),
        }
    }

    fn email(value: &SecretString) -> Self {
        let v = value.expose_secret();
        let shown = match (v.chars().next(), v.rsplit_once('@')) {
            (Some(first), Some((_, domain))) => format!("{first}•••@{domain}"),
            _ => "•••".to_owned(),
        };
        Self {
            kind: ContactKind::Email,
            shown,
        }
    }

    /// The contact's kind.
    #[must_use]
    pub const fn kind(&self) -> ContactKind {
        self.kind
    }

    /// The masked value.
    #[must_use]
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// BIZ-026, D-366: the line shown before a code; a landline is never called, so its line
    /// points to the email or letter step instead.
    #[must_use]
    pub fn line(&self) -> String {
        match self.kind {
            ContactKind::Landline => format!(
                "The phone on your record, {}, is a landline, so we can't text it, and we never \
call. Your code goes to the email on your record, or by letter if there's no email.",
                self.shown
            ),
            ContactKind::MailingAddress => format!(
                "We'll mail a letter with your 6-digit code to the address on your record in {}.",
                self.shown
            ),
            ContactKind::Mobile => format!(
                "We'll text your 6-digit code to the mobile phone on your ROC or city license \
record: {}.",
                self.shown
            ),
            ContactKind::Email => format!(
                "We'll email your 6-digit code to the address on your ROC or city license record: \
{}.",
                self.shown
            ),
        }
    }
}

/// BIZ-026, D-288: the "That number is old" and "Stop, I'll fix my record first" explanation,
/// with the typical delay (a setting) before the daily list shows a change.
#[must_use]
pub fn fix_record_lines(delay_days: u32) -> [String; 3] {
    [
        "To change it, update your contact details with the Arizona Registrar of Contractors \
(ROC). If you only have a city business license, update it with the city."
            .to_owned(),
        format!("Changes usually reach us about {delay_days} days after you make them."),
        "Once we see the change, we'll email you a link to finish. Your code will go to your new \
number or email."
            .to_owned(),
    ]
}

/// The keyed digest of the record contacts, so a change is seen with no contact stored (PRIV-001).
fn contacts_key(secret: &SecretString, business: Uuid, record: &RecordContacts) -> String {
    let phone = record
        .texting_phone
        .as_ref()
        .map(ExposeSecret::expose_secret);
    let email = record.email.as_ref().map(ExposeSecret::expose_secret);
    let address = if record.has_address { "1" } else { "0" };
    let id = business.to_string();
    let parts = [
        secret.expose_secret(),
        "businesses.record_contacts",
        &id,
        phone.unwrap_or_default(),
        email.unwrap_or_default(),
        address,
    ];
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// BIZ-026, D-178: "That number is old": saves the sign-up as waiting on its record, keyed to the
/// record contacts it said were old. Only a sign-up whose code is not yet done waits.
///
/// # Errors
/// [`BusinessesError::Refused`] for no such waiting sign-up; [`BusinessesError::Database`].
pub async fn number_is_old(
    conn: &mut PgConnection,
    secret: &SecretString,
    (business, record): (Uuid, &RecordContacts),
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let key = contacts_key(secret, business, record);
    let q = sqlx::query!(
        "UPDATE businesses.signup SET waiting_on_record_since = $2, old_contact_key = $3 \
         WHERE business_id = $1 AND NOT code_done",
        business,
        jiff_sqlx::ToSqlx::to_sqlx(now) as _,
        key
    );
    let done = db(q.execute(conn).await)?;
    if done.rows_affected() == 1 {
        Ok(())
    } else {
        Err(BusinessesError::Refused)
    }
}

/// BIZ-026, D-178, D-254: run for a waiting sign-up when a daily list is read. When the record
/// contacts differ from the ones it said were old, the wait ends and the identity chain starts
/// over, so the next `send_code` goes to the new record contact (never a typed one); the caller
/// then sends [`RECORD_UPDATED_TEMPLATE`]. Returns whether it resumed.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn resume_on_new_record(
    conn: &mut PgConnection,
    secret: &SecretString,
    (business, record): (Uuid, &RecordContacts),
) -> Result<bool, BusinessesError> {
    let key = contacts_key(secret, business, record);
    let q = sqlx::query!(
        "UPDATE businesses.signup SET waiting_on_record_since = NULL, old_contact_key = NULL, \
         identity_step = NULL WHERE business_id = $1 AND old_contact_key <> $2",
        business,
        key
    );
    Ok(db(q.execute(conn).await)?.rows_affected() == 1)
}

/// BIZ-026: whether the sign-up is waiting on its record.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn waiting_on_record(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<bool, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT waiting_on_record_since IS NOT NULL AS \"w!\" FROM businesses.signup \
         WHERE business_id = $1",
        business
    );
    Ok(db(q.fetch_optional(conn).await)?.unwrap_or(false))
}

/// The setting for [`SignupSettings::review_target_days`] (BIZ-027).
pub const REVIEW_TARGET_KEY: &str = "ASKHAVA_BIZ_REVIEW_TARGET_DAYS";
/// The setting for [`SignupSettings::list_delay_days`] (BIZ-026).
pub const LIST_DELAY_KEY: &str = "ASKHAVA_BIZ_LIST_DELAY_DAYS";

/// The sign-up screens' settings (BIZ-026, BIZ-027, D-223).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignupSettings {
    /// Calendar days for Casey's needs-review decision, a target that blocks nothing (default 4).
    pub review_target_days: u32,
    /// The typical days until the daily list shows a record change (default 3).
    pub list_delay_days: u32,
}

impl Default for SignupSettings {
    fn default() -> Self {
        Self {
            review_target_days: 4,
            list_delay_days: 3,
        }
    }
}

impl SignupSettings {
    /// BIZ-026, BIZ-027: reads [`REVIEW_TARGET_KEY`] and [`LIST_DELAY_KEY`]; absent or empty keeps
    /// the default. Every other key is ignored (`Settings::from_pairs` checks the names).
    ///
    /// # Errors
    /// [`SettingsError::Invalid`] naming the key when a value is not a whole number from 1 to 30.
    pub fn from_pairs<I>(pairs: I) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut s = Self::default();
        for (name, value) in pairs {
            let (key, slot) = match name.as_str() {
                REVIEW_TARGET_KEY => (REVIEW_TARGET_KEY, &mut s.review_target_days),
                LIST_DELAY_KEY => (LIST_DELAY_KEY, &mut s.list_delay_days),
                _ => continue,
            };
            if value.trim().is_empty() {
                continue;
            }
            *slot = value
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|d| (1..=30).contains(d))
                .ok_or(SettingsError::Invalid(key))?;
        }
        Ok(s)
    }
}

/// BIZ-027, D-204, D-223: the decision target of a sign-up in needs review since `since`, in
/// calendar days; it blocks nothing.
#[must_use]
pub fn review_target(since: Timestamp, days: u32) -> Timestamp {
    since + SignedDuration::from_hours(i64::from(days) * 24)
}

/// BIZ-027: the owner portal's service-target line, the date in Arizona time (UTC-7, no DST).
#[must_use]
pub fn review_target_line(since: Timestamp, days: u32) -> String {
    let due = review_target(since, days).to_zoned(tz::TimeZone::fixed(tz::offset(-7)));
    format!(
        "Decide by {}, {days} calendar days after this came in. Nothing is held up in the meantime.",
        due.strftime("%B %-d")
    )
}

/// The numbers of the plain-words fee box, read from the fee settings (BIZ-030, D-173).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeNumbers {
    /// The three introduction fees by job size, in cents (`payments::api::Tier::cents`).
    pub tier_cents: [u64; 3],
    /// Business hours to confirm a pick (the Confirm window setting, PICK-019).
    pub confirm_hours: u32,
    /// Minutes to confirm an emergency pick (PICK-044).
    pub emergency_minutes: i64,
}

/// Whole dollars, or dollars and cents.
fn dollars(cents: u64) -> String {
    if cents.is_multiple_of(100) {
        format!("${}", cents / 100)
    } else {
        format!("${}.{:02}", cents / 100, cents % 100)
    }
}

/// BIZ-030, D-173, D-205, D-248, D-349, D-357, D-380, D-406, D-1494: the plain-words summary of
/// fees, refunds and card use, every number from `n`, in the spec's order.
#[must_use]
pub fn fee_summary(n: &FeeNumbers) -> Vec<String> {
    let [s, m, l] = n.tier_cents.map(dollars);
    vec![
        format!("Free to join. Pay only when you confirm a pick: {s} / {m} / {l} by job size. Refund if you can't reach them."),
        format!("Picks must be confirmed within {} business hours (8 a.m. to 8 p.m., every day), or within {} minutes for an emergency job. If you check email only once or twice a day, add a backup address for someone who checks more often.", n.confirm_hours, n.emergency_minutes),
        "It's an introduction fee for a homeowner who chose you.".to_owned(),
        format!("You pay one fixed fee when you confirm a pick: {s}, {m} or {l} by job size. It's not a share of the job. If you call twice inside the homeowner's window and can't reach them, the refund rule gives the fee back without you asking."),
        "It's a fixed introduction fee, never a share of the job, and it doesn't depend on the work being done or paid for.".to_owned(),
        BUSINESS_STATEMENT.to_owned(),
        "The homeowner sees every quote and picks one business: you. It isn't shared with anyone else.".to_owned(),
        "A new fee never applies until you accept it.".to_owned(),
        format!("Repeat picks from the same homeowner are free for {REPEAT_FREE_DAYS} days after the job closes."),
        "Fees follow the published schedule, and a change needs your OK first.".to_owned(),
        "Statements download as CSV.".to_owned(),
        "If many picks in your trade end without a hire, the Ask Hava owner reviews it and may add a credit.".to_owned(),
    ]
}

/// BIZ-031, CARD-019, D-187: the done screen's <n> for `category`: the active businesses with a
/// job type in it (the businesses a card in that trade goes to), or with `waitlist` the waitlisted
/// businesses that chose it at sign-up, always counting `business` itself.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn trade_count(
    conn: &mut PgConnection,
    business: Uuid,
    category: &str,
    waitlist: bool,
) -> Result<u32, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.business b WHERE b.id = $1 \
         OR (NOT $3 AND b.status = 'active' AND EXISTS (SELECT 1 FROM businesses.job_type_choice c \
             WHERE c.business_id = b.id AND c.category = $2)) \
         OR ($3 AND b.status = 'waitlist' AND EXISTS (SELECT 1 FROM businesses.signup s \
             WHERE s.business_id = b.id AND $2 = ANY (s.trades)))",
        business,
        category,
        waitlist
    );
    let n = db(q.fetch_one(conn).await)?;
    u32::try_from(n).map_err(|_| BusinessesError::Database)
}

/// BIZ-031, CON-019, D-187, D-312: claims the one launch notice email ("requests in your trade
/// start", or the waitlist's "it opens"), an account notice; `true` only the first time, so the
/// caller sends it once.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn claim_launch_notice(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup SET launch_notice_sent_at = $2 \
         WHERE business_id = $1 AND launch_notice_sent_at IS NULL",
        business,
        jiff_sqlx::ToSqlx::to_sqlx(now) as _
    );
    Ok(db(q.execute(conn).await)?.rows_affected() == 1)
}
