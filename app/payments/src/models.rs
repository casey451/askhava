//! Stripe customers, the card authorization and saved cards: the checked types (FEE-009, FEE-014,
//! FEE-015, FEE-036, FEE-048, D-073, D-365).
//!
//! A Stripe customer is always a business's ([`BusinessId`]); no type here can name a homeowner,
//! so nothing in `payments` can make a homeowner a Stripe customer or charge one (FEE-009). A
//! saved card is a [`SavedCard`]: the PaymentMethod ID, brand and last four only, never card data,
//! which Stripe Elements keeps on Stripe's side (FEE-014); any method type but `card` is refused
//! (D-073). [`AUTHORIZATION_TEXT`] is the FEE-015 wording, stored with [`AUTHORIZATION_VERSION`]
//! and the acceptance time, and [`SetupIntentRequest`] is the one way a card is saved on a $0
//! Confirm page: `usage=off_session`, cards only (FEE-048).
//!
//! T061 (FEE-010, FEE-011, D-151, D-152, D-195): [`EntityKey`] is a free-first-win entity key
//! stored only as a keyed digest, [`JoinDates`] the dates the 90-day window is judged by, and
//! [`FreeWinError`] why a free-win step failed.
//!
//! T062 (FEE-016, FEE-017, FEE-047, D-1425): [`Press`] is one "Confirm and pay" press with the
//! caller's [`PickNow`] check, [`ChargeOutcome`] what it came to, [`ChargeError`] why it could not
//! finish, and [`intent_label`] and [`attempt_label`] the labels the Stripe keys derive from.
//!
//! T066 (FEE-026, FEE-027, FEE-042, PRIV-027, D-1542 to D-1544): [`RefundTrigger`] is why a pick
//! is refunded, [`RefundStep`] what a request or retry does next, [`RefundOutcome`] and
//! [`RefundError`] what it came to; [`DisputeOpened`] is a Stripe dispute as the webhook reports
//! it, [`EvidencePack`] the IDs-and-timestamps evidence built from a charge's FEE-042 record, and
//! [`DisputeView`] one dispute as the owner portal and the weekly digest list it.

use askhava_adapters::api::IdempotencyKey;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::ledger::LedgerError;
use crate::refund_rules::RefundReason;

/// The FEE-015 card authorization, exactly as `docs/spec/fees.md` words it (D-195, D-196, D-365).
/// A change of wording is a new [`AUTHORIZATION_VERSION`], accepted again before the next charge
/// (FEE-007).
pub const AUTHORIZATION_TEXT: &str = "I authorize Ask Hava LLC to save this card with Stripe for \
introduction fees. I'm charged only when I press Confirm and pay for a homeowner's choice of my \
quote, or when a homeowner picks a quote I set to auto-confirm, and the fee is shown before I \
press it or set it: $25, $75 or $199 by job size, ongoing services counted at 4 months and capped \
at $75. First win free if I join in the first 90 days or within 90 days after my trade opens. \
Refunds per the Terms. I can remove this card at any time.";

/// The version of [`AUTHORIZATION_TEXT`] stored with each acceptance (FEE-015, FEE-007).
pub const AUTHORIZATION_VERSION: i32 = 1;

/// Why a customer or card step was refused or could not finish. No variant carries a card
/// detail, a vendor message or a database message (R5, D-698).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CardError {
    /// The payment method is not a card: bank debits and every other type are refused (FEE-014,
    /// D-073).
    #[error("only cards are accepted")]
    CardsOnly,
    /// A Stripe ID, brand or last four is not of the expected form.
    #[error("a card field has the wrong form")]
    BadField,
    /// The business has no Stripe customer yet, so no card can be saved for it.
    #[error("the business has no Stripe customer")]
    NoCustomer,
    /// The card is already saved for another business.
    #[error("the card is saved for another business")]
    OtherBusiness,
    /// The Stripe call failed; nothing was stored.
    #[error("the Stripe request failed")]
    Stripe,
    /// The database refused or dropped the statement.
    #[error("the payments database statement failed")]
    Database,
}

/// A business's record ID: the only party that can be a Stripe customer (FEE-009, FEE-036).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusinessId(pub Uuid);

/// A Stripe customer ID, `cus_…`, checked once at the edge (R1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerId(String);

impl CustomerId {
    /// Checks `id` is a `cus_…` ID.
    ///
    /// # Errors
    /// [`CardError::BadField`] for any other form.
    pub fn parse(id: &str) -> Result<Self, CardError> {
        vendor_id(id, "cus_").map(Self)
    }

    /// The ID as Stripe knows it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A card saved through Stripe Elements: IDs, brand and last four only (FEE-014). Only
/// [`SavedCard::new`] makes one, so a bank debit never reaches the database (D-073, R2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedCard {
    payment_method: String,
    brand: String,
    last4: String,
}

impl SavedCard {
    /// Checks a PaymentMethod Stripe reported (`setup_intent.succeeded` or the Confirm page):
    /// `method_type` must be `card` (FEE-014, D-073), the ID `pm_…`, the brand lower-case and
    /// the last four four digits.
    ///
    /// # Errors
    /// [`CardError::CardsOnly`] for any type but `card`; [`CardError::BadField`] for a bad field.
    pub fn new(
        payment_method: &str,
        method_type: &str,
        brand: &str,
        last4: &str,
    ) -> Result<Self, CardError> {
        if method_type != "card" {
            return Err(CardError::CardsOnly);
        }
        let brand_ok = (1..=30).contains(&brand.len())
            && brand.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
        let last4_ok = last4.len() == 4 && last4.bytes().all(|b| b.is_ascii_digit());
        if !brand_ok || !last4_ok {
            return Err(CardError::BadField);
        }
        Ok(Self {
            payment_method: vendor_id(payment_method, "pm_")?,
            brand: brand.to_owned(),
            last4: last4.to_owned(),
        })
    }

    /// The PaymentMethod ID, `pm_…`.
    #[must_use]
    pub fn payment_method(&self) -> &str {
        &self.payment_method
    }

    /// The card brand, for example `visa`.
    #[must_use]
    pub fn brand(&self) -> &str {
        &self.brand
    }

    /// The last four digits.
    #[must_use]
    pub fn last4(&self) -> &str {
        &self.last4
    }
}

/// The SetupIntent that saves a card on a $0 Confirm page, charging nothing (FEE-036, FEE-048).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupIntentRequest {
    customer: CustomerId,
}

impl SetupIntentRequest {
    /// The request for `customer`, a business's Stripe customer (FEE-009).
    #[must_use]
    pub const fn for_customer(customer: CustomerId) -> Self {
        Self { customer }
    }

    /// The Stripe form fields: `usage=off_session`, because FEE-015 covers the optional PICK-048
    /// auto-confirm charge (FEE-048, D-490), and `card` as the only method type (FEE-014, D-073).
    #[must_use]
    pub fn form(&self) -> [(&'static str, &str); 3] {
        [
            ("customer", self.customer.as_str()),
            ("usage", "off_session"),
            ("payment_method_types[]", "card"),
        ]
    }
}

/// The most characters after a vendor ID's prefix, as the table checks allow
/// (`'^cus_[A-Za-z0-9_]{1,200}$'`, `'^pm_[A-Za-z0-9_]{1,200}$'`).
const VENDOR_TAIL_MAX: usize = 200;

fn vendor_id(id: &str, prefix: &str) -> Result<String, CardError> {
    let ok = id.strip_prefix(prefix).is_some_and(|tail| {
        (1..=VENDOR_TAIL_MAX).contains(&tail.len())
            && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    });
    if ok {
        Ok(id.to_owned())
    } else {
        Err(CardError::BadField)
    }
}

/// The five free-first-win entity keys (FEE-011, D-019, D-151): a business may have several of
/// each, and a free first win is allowed once across every business sharing any one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    /// The Arizona ROC license number.
    Roc,
    /// The entity on the city business-license list.
    CityEntity,
    /// The Stripe card fingerprint of a saved card.
    CardFingerprint,
    /// The identity-code contact on the record (its phone or email, D-151).
    IdentityContact,
    /// The owner name on the city list (D-151).
    OwnerName,
}

impl EntityKind {
    /// The stored code, matching `payments.entity_key.kind`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Roc => "roc",
            Self::CityEntity => "city-entity",
            Self::CardFingerprint => "card-fingerprint",
            Self::IdentityContact => "identity-contact",
            Self::OwnerName => "owner-name",
        }
    }
}

/// The HMAC action under which an entity key's value is digested (PRIV-002, postgres brain P12).
pub const ENTITY_KEY_ACTION: &str = "payments.entity_key";

/// One entity key as stored: its kind and a keyed digest of the normalized value, never the value
/// itself, so no contact or owner name sits in a `payments` column (FEE-011, PRIV-002). Only
/// [`EntityKey::new`] makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityKey {
    kind: EntityKind,
    digest: String,
}

impl EntityKey {
    /// FEE-011: digests `value` under the ID secret after trimming, lower-casing and collapsing
    /// inner whitespace, so "ACME  Roofing " and "acme roofing" are one key. The caller passes
    /// the identity-code contact already in its stored form (`+1` and ten digits, or the email).
    ///
    /// # Errors
    /// [`FreeWinError::EmptyKey`] when nothing is left after normalizing.
    pub fn new(secret: &SecretString, kind: EntityKind, value: &str) -> Result<Self, FreeWinError> {
        let normal = value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if normal.is_empty() {
            return Err(FreeWinError::EmptyKey);
        }
        let digest = IdempotencyKey::derive(secret, ENTITY_KEY_ACTION, &[kind.code(), &normal]);
        Ok(Self {
            kind,
            digest: digest.as_str().to_owned(),
        })
    }

    /// The key's kind.
    #[must_use]
    pub const fn kind(&self) -> EntityKind {
        self.kind
    }

    /// The 64-hex-digit keyed digest stored in `payments.entity_key.digest`.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// The dates FEE-010 judges a business by: when its sign-up form was submitted (D-152), the
/// launch date (Gate C, a setting, D-019) and when each category it signed up for was switched on
/// (D-195). The caller reads them from `businesses` and `catalog`, which `payments` cannot read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinDates {
    /// When the business's sign-up form was submitted (FEE-010, D-152).
    pub joined: Timestamp,
    /// Launch: the date of Gate C (D-019).
    pub launch: Timestamp,
    /// When each category the business signed up for was switched on; none while waitlisted.
    pub trades_opened: Vec<Timestamp>,
}

/// Why a free-win step was refused or could not finish. No variant carries a key value (R5).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FreeWinError {
    /// An entity key's value is empty after normalizing.
    #[error("an entity key is empty")]
    EmptyKey,
    /// Another Confirm used a shared key or this business's win at the same moment; nothing was
    /// written, and the caller runs its Confirm again, which then sees that win.
    #[error("a free first win was used at the same moment; try again")]
    Race,
    /// The ledger refused the $0 entry.
    #[error("the ledger refused the entry")]
    Ledger(#[from] LedgerError),
    /// The database refused or dropped the statement.
    #[error("the payments database statement failed")]
    Database,
}

/// What the caller read about the pick and its job just before the press: payable only while
/// the job is still waiting for payment and the pick is still the current one (FEE-017, D-491).
/// `payments` cannot read `picks` or `jobs`, so the Confirm page passes it; `payments` checks its
/// own record of the pick as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickNow {
    /// The job is in Payment pending and this pick is current.
    Payable,
    /// The job or the pick moved on: nothing is charged.
    MovedOn,
}

/// One "Confirm and pay" press (FEE-016): record IDs, the server's fee and the card, never card
/// data (FEE-014). `press` is the Confirm form's one-time ID, so a repeated press, a reload or a
/// duplicate callback is the same attempt with the same key.
#[derive(Debug, Clone, Copy)]
pub struct Press<'a> {
    /// The pick being paid for.
    pub pick: Uuid,
    /// The business pressing.
    pub business: BusinessId,
    /// The pick's job.
    pub job: Uuid,
    /// The fee fixed at the pick, in cents (FEE-005); above 0, a $0 pick never charges.
    pub amount_cents: u32,
    /// The card chosen in Stripe Elements, `pm_…`.
    pub payment_method: &'a str,
    /// The press's one-time ID.
    pub press: Uuid,
    /// The job was chosen as an emergency: the card-fix window is 60 clock minutes (PICK-044).
    pub emergency: bool,
    /// The FEE-017 facts the caller read.
    pub now: PickNow,
}

/// What a press came to; the Confirm page (T224) shows each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChargeOutcome {
    /// Stripe says the PaymentIntent `pi_…` succeeded; the release waits on the verified event
    /// (PICK-022, T189).
    Succeeded(String),
    /// Stripe has not decided; the page waits, and the app asks Stripe at 30 minutes (FEE-019).
    Pending,
    /// The card failed: try again or use another card by the time given (FEE-020).
    Declined(Option<Timestamp>),
    /// The bank must approve the card in the page by the time given (FEE-020).
    NeedsApproval(Option<Timestamp>),
    /// Stripe could not be reached; nothing was charged (FEE-018).
    StripeDown,
    /// Charges are paused by safe mode or the charging switch: "try again later" (OPS-024).
    Paused,
    /// The press would take the month's charges over the business's limit: not charged, and the
    /// pick ended at once (FEE-041).
    OverLimit,
    /// The job or the pick moved on, or the pick already ended: nothing charged (FEE-017).
    MovedOn,
    /// The pick is already paid: nothing charged again (FEE-016, FEE-037).
    AlreadyPaid,
}

/// Why a charge step could not finish. No variant carries a card detail, a vendor message or a
/// database message (R5, D-698).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChargeError {
    /// The amount is 0 or a Stripe ID has the wrong form.
    #[error("a charge field has the wrong form")]
    BadField,
    /// The business has no Stripe customer.
    #[error("the business has no Stripe customer")]
    NoCustomer,
    /// The PaymentIntent could not be created or read; nothing was charged.
    #[error("the Stripe request failed")]
    Stripe,
    /// The database refused or dropped the statement.
    #[error("the payments database statement failed")]
    Database,
}

/// The label a pick's PaymentIntent key is derived from: `pick-<id>-intent` (FEE-047).
#[must_use]
pub fn intent_label(pick: Uuid) -> String {
    format!("pick-{pick}-intent")
}

/// The label attempt `try_no` of a pick is keyed by: `pick-<id>-try<n>` (FEE-016).
#[must_use]
pub fn attempt_label(pick: Uuid, try_no: i32) -> String {
    format!("pick-{pick}-try{try_no}")
}

/// The account statement-descriptor prefix set once in the Stripe dashboard (FEE-049, D-490).
pub const STATEMENT_PREFIX: &str = "ASKHAVA";
/// The longest statement descriptor a card statement shows (FEE-049).
pub const DESCRIPTOR_MAX: usize = 22;

/// What a pick's PaymentIntent says on the receipt and the card statement: the description "Ask
/// Hava introduction service, job <n>" and the suffix "JOB <n>" after [`STATEMENT_PREFIX`]
/// (FEE-049, TD-FEE-38, D-491). The adapter form that sends them is T132's (D-1476).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentText {
    number: i64,
}

impl IntentText {
    /// The text for job number `number` (JOB-024's `jobs.job_number`).
    ///
    /// # Errors
    /// [`ChargeError::BadField`] for a number below 1, or one long enough to take the full
    /// descriptor over [`DESCRIPTOR_MAX`] characters (FEE-049).
    pub fn for_job(number: i64) -> Result<Self, ChargeError> {
        let text = Self { number };
        if number < 1 || text.statement_descriptor().len() > DESCRIPTOR_MAX {
            return Err(ChargeError::BadField);
        }
        Ok(text)
    }

    /// "Ask Hava introduction service, job <n>" (D-491).
    #[must_use]
    pub fn description(&self) -> String {
        format!("Ask Hava introduction service, job {}", self.number)
    }

    /// The statement-descriptor suffix "JOB <n>" (FEE-049).
    #[must_use]
    pub fn statement_suffix(&self) -> String {
        format!("JOB {}", self.number)
    }

    /// The full descriptor as the card statement shows it, "ASKHAVA* JOB <n>" (FEE-049).
    #[must_use]
    pub fn statement_descriptor(&self) -> String {
        format!("{STATEMENT_PREFIX}* {}", self.statement_suffix())
    }
}

/// Why a pick's charge is refunded (FEE-022 to FEE-025 through T065's rules, FEE-052's early fraud
/// warning); stored as [`RefundTrigger::code`] in `payments.refund.trigger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundTrigger {
    /// A refund rule of `refund_rules` decided it.
    Rule(RefundReason),
    /// FEE-052: an early fraud warning on a charge whose details were not yet released.
    FraudWarning,
}

impl RefundTrigger {
    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Rule(RefundReason::Unreachable) => "unreachable",
            Self::Rule(RefundReason::CancelledBeforeTalk) => "cancelled-before-talk",
            Self::Rule(RefundReason::ChargedAfterCancel) => "charged-after-cancel",
            Self::FraudWarning => "fraud-warning",
        }
    }
}

/// What a refund request or an hourly retry does next (FEE-026), from `RefundStatus::next`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundStep {
    /// Send try `n` to Stripe, keyed by the pick's refund label and `n`.
    Send(i32),
    /// Nothing: refunded already, a try still in flight, or a retry overtaken by a later try.
    Nothing,
    /// Record the block; `true` when it is new, so Casey is alerted once.
    Block(bool),
}

/// What a refund request came to (FEE-026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundOutcome {
    /// Stripe took the refund (`re_…`); the ledger holds the pick's one refund entry.
    Refunded(String),
    /// Nothing was sent: the pick was refunded already, or a try is still in flight.
    Unchanged,
    /// The charge is disputed: no refund, Casey is alerted (FEE-026).
    Blocked,
    /// Stripe refused or could not be reached: an alert is raised and the hourly retry is queued.
    Failed,
}

/// Why a refund or dispute step could not finish. No variant carries a vendor or database message
/// (R5, D-698).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MoneyEventError {
    /// The pick has no charge evidence record: nothing was charged, so nothing is refunded.
    #[error("nothing was charged for this pick")]
    NothingCharged,
    /// A dispute names a PaymentIntent no pick was charged through, or has a malformed field.
    #[error("the dispute does not match a charge")]
    UnknownCharge,
    /// Stripe refused the evidence or could not be reached; the task is run again.
    #[error("the Stripe request failed")]
    Stripe,
    /// The ledger refused the entry.
    #[error("the ledger refused the entry")]
    Ledger(#[from] LedgerError),
    /// The database refused or dropped the statement.
    #[error("the payments database statement failed")]
    Database,
}

/// A `charge.dispute.created` event's fields, after the webhook route checked its signature
/// (FEE-027). IDs, an amount and Stripe's evidence due date only.
#[derive(Debug, Clone, Copy)]
pub struct DisputeOpened<'a> {
    /// The dispute, `dp_…` (or `du_…`).
    pub dispute: &'a str,
    /// The disputed PaymentIntent, `pi_…`.
    pub payment_intent: &'a str,
    /// The disputed amount in cents.
    pub amount_cents: i64,
    /// `evidence_details.due_by`.
    pub due_at: Timestamp,
}

/// The evidence sent to Stripe for a dispute, built only from the charge's FEE-042 record and its
/// refund: IDs and timestamps, never a contact field or help-message text (PRIV-027, D-365).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePack {
    pub(crate) customer: String,
    pub(crate) payment_intent: String,
    pub(crate) attempt_label: String,
    pub(crate) authorization_version: Option<i32>,
    pub(crate) amount_cents: i64,
    pub(crate) pressed_at: Timestamp,
    pub(crate) charged_at: Timestamp,
    pub(crate) refund: Option<String>,
}

/// What the evidence calls the product: the introduction fee by job size (TERMS-027, WORD-002).
pub const EVIDENCE_PRODUCT: &str = "Introduction fee by job size. The business pressed Confirm \
    and pay to be put in touch with a homeowner about one home-service job.";

/// A time as the evidence shows it: `2026-11-04 10:00:00 UTC`.
fn utc(at: Timestamp) -> String {
    at.strftime("%Y-%m-%d %H:%M:%S UTC").to_string()
}

impl EvidencePack {
    /// The Stripe form fields: the product line, the record lines and `submit=true`.
    #[must_use]
    pub fn form(&self) -> Vec<(&'static str, String)> {
        let version = self
            .authorization_version
            .map_or_else(|| "none".to_owned(), |v| v.to_string());
        let refund = self.refund.clone().unwrap_or_else(|| "none".to_owned());
        let (dollars, cents) = (self.amount_cents / 100, self.amount_cents % 100);
        let record = format!(
            "Stripe customer: {}\nStripe payment ID: {}\nOur payment reference: {}\n\
             Card payment terms version the business accepted: {version}\n\
             Amount: ${dollars}.{cents:02}\nConfirm and pay pressed at: {}\n\
             Payment went through at: {}\nRefund: {refund}",
            self.customer,
            self.payment_intent,
            self.attempt_label,
            utc(self.pressed_at),
            utc(self.charged_at),
        );
        vec![
            ("evidence[product_description]", EVIDENCE_PRODUCT.to_owned()),
            ("evidence[uncategorized_text]", record),
            ("submit", "true".to_owned()),
        ]
    }
}

/// One dispute as the owner portal and the weekly digest list it (FEE-027, FEE-050).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisputeView {
    /// `dp_…`.
    pub dispute: String,
    /// The disputed amount in cents.
    pub amount_cents: i64,
    /// Stripe's evidence due date.
    pub due_at: Timestamp,
    /// Marked by T191 for auto-submit (FEE-050's condition held).
    pub auto_submit: bool,
    /// When the evidence went in, if it has.
    pub submitted_at: Option<Timestamp>,
    /// The countering fee at stake if Casey counters (FEE-050): `DISPUTE_FEE_CENTS`.
    pub fee_at_stake_cents: i64,
}
