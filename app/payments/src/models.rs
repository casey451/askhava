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

use sqlx::types::Uuid;

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
