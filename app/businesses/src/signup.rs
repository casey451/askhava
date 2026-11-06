//! Business sign-up (BIZ-001, BIZ-019, BIZ-020, BIZ-021, BIZ-053, FEE-011, TERMS-016, TERMS-033;
//! D-145, D-175, D-176, D-457, D-492).
//!
//! Sign-up starts from the category list, each "Open now" or "Coming later" by `licensing`'s
//! `launch_open` map (LIC-046), before any license number; only coming-later choices put the
//! business on the waitlist. It needs an email (by its `privacy` mailbox ID), asks the optional
//! source question, stores the business and its sign-up row with an audit row in one transaction,
//! and then, outside it, creates the Stripe customer with no card (R11). The identity chain and
//! activation are in `identity`.

use askhava_adapters::api::PaymentsAdapter;
use askhava_licensing::api::{ClassMap, Opening};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_payments::api::{
    BusinessId, EntityKey, EntityKind, ensure_customer, record_entity_keys,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::about_us::AboutUsRefusal;
use crate::models::{self, BusinessesError, db};
use crate::status::AccountStatus;

/// The waitlist line shown when only coming-later categories were chosen (BIZ-021, D-175, D-288).
pub const WAITLIST_LINE: &str = "The trades you picked open on Ask Hava later, so you're on the \
waitlist for now. Your join date is kept. No priority; everyone waiting is told at once.";

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
/// item like any other (LIC-012 limits who may tick it later).
#[must_use]
pub fn category_list(map: &ClassMap, codes: &[&str]) -> Vec<(String, &'static str)> {
    let label = |o: Opening| match o {
        Opening::OpenNow => Some("Open now"),
        Opening::ComingLater => Some("Coming later"),
        Opening::Off => None,
    };
    let labelled = codes.iter().map(|c| (*c, label(map.opening(c))));
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
    let codes: Vec<&str> = form.categories.iter().map(String::as_str).collect();
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
