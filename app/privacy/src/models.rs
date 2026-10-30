//! The vault's vocabulary: contact fields, readers, purposes and the read grants (PRIV-001, PRIV-004, SEC-047).
//!
//! A plaintext read names its reader and purpose; the pair must be one of [`GRANTS`], the readers
//! of `docs/architecture.md` section 8 (D-093), and every field it asks for must be one that grant
//! allows. Anything else is an unregistered purpose: the read is refused and a High alert raised at
//! once (SEC-047). Each grant's `action` is the code name its audit rows and read counts carry, so
//! the reader and purpose of every read can be found in `ops.audit_log` (PRIV-003, PRIV-028).

use secrecy::SecretString;
use sqlx::types::Uuid;
use thiserror::Error;

/// Why a vault call failed. No variant carries a value, a key or the database's message (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    /// `VAULT_KEY` or `VAULT_INDEX_KEY` is missing, malformed, not 32 bytes, repeats a version,
    /// is shared between the two, or appears in the database credential (PRIV-002, PRIV-024).
    #[error("the vault keys are not set as required ({0})")]
    Keys(&'static str),
    /// The reader and purpose (or a field asked for) are not a registered grant; refused and
    /// alerted (SEC-047).
    #[error("unregistered vault read purpose")]
    UnregisteredPurpose,
    /// The phone number is not a 10-digit US number.
    #[error("not a US phone number")]
    BadPhone,
    /// Another contact already holds this phone number.
    #[error("the phone number belongs to another contact")]
    PhoneTaken,
    /// No contact has this ID.
    #[error("no such contact")]
    NotFound,
    /// A sealed value did not open: wrong row, column or key, or tampered (PRIV-002).
    #[error("a vault value failed to open")]
    Sealed,
    /// The `ops` audit or alert write failed, so nothing was read.
    #[error("the vault read could not be audited")]
    Audit,
    /// The database refused or dropped the statement.
    #[error("the vault database statement failed")]
    Database,
}

/// One homeowner contact field, each its own sealed column of `privacy.contact` (PRIV-001, PRIV-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    /// The homeowner's name.
    Name,
    /// The phone number, normalized to `+1` and ten digits.
    Phone,
    /// The email address.
    Email,
    /// The street address.
    Address,
    /// The on-site contact.
    OnsiteContact,
}

impl Field {
    /// Every field, in column order.
    pub const ALL: [Self; 5] = [
        Self::Name,
        Self::Phone,
        Self::Email,
        Self::Address,
        Self::OnsiteContact,
    ];

    /// The column name, also the audit row's `field` (PRIV-028).
    #[must_use]
    pub fn column(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Phone => "phone",
            Self::Email => "email",
            Self::Address => "address",
            Self::OnsiteContact => "onsite_contact",
        }
    }
}

/// A module that reads vault plaintext (PRIV-004, D-093).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// `messaging`: sending, and handing an inbound message to the inbound reader.
    Messaging,
    /// `picks`: the paid or fee-free release, through a `ReleaseProof` only (SEC-030).
    Picks,
    /// `calls`: bridging a masked call, and the CALL-005 read-back.
    Calls,
    /// `portals`, homeowner realm, through a `HomeownerSessionProof` only (SEC-046).
    PortalsHomeowner,
    /// `portals`, owner realm, through an `OwnerSessionProof` only (SEC-046).
    PortalsOwner,
    /// `privacy` itself (key rotation now; scrubbing and purges in later tasks).
    Privacy,
}

/// Why a reader reads (PRIV-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Send a text, email or letter.
    Send,
    /// Hand a raw inbound homeowner message to the inbound reader.
    Inbound,
    /// Dial the homeowner on a masked call (CALL-002).
    BridgeCall,
    /// Read first name and street back on CALL-005.
    CallReadback,
    /// Release details on a confirmed, paid or fee-free pick (PRIV-005).
    Release,
    /// A signed-in homeowner's own record (PRIV-021).
    OwnRecord,
    /// The contact on a call-back alert Casey opens (INT-023).
    CallbackAlert,
    /// A help request's contact on its page (OPS-036).
    HelpRequest,
    /// Re-seal under a new key version (PRIV-024).
    Rotation,
}

/// Why the owner realm reads; the purpose an `OwnerSessionProof` read carries (SEC-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerPurpose {
    /// The contact on a call-back alert.
    CallbackAlert,
    /// A help request's contact on its page.
    HelpRequest,
}

impl From<OwnerPurpose> for Purpose {
    fn from(purpose: OwnerPurpose) -> Self {
        match purpose {
            OwnerPurpose::CallbackAlert => Self::CallbackAlert,
            OwnerPurpose::HelpRequest => Self::HelpRequest,
        }
    }
}

/// One registered reader and purpose, the fields it may read and its audit action (PRIV-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    /// Who reads.
    pub reader: Reader,
    /// Why.
    pub purpose: Purpose,
    /// The only fields this grant opens.
    pub fields: &'static [Field],
    /// The audit row's and read count's action code name.
    pub action: &'static str,
}

/// Every allowed plaintext read (`docs/architecture.md` section 8, D-093, D-491): the phone is
/// never released (PICK-023), the call read-back is first name and street only (CALL-005), and
/// `quotes` reads no contact field at all (PRIV-004).
pub const GRANTS: [Grant; 9] = {
    use Field::{Address, Email, Name, OnsiteContact, Phone};
    const ALL: &[Field] = &[Name, Phone, Email, Address, OnsiteContact];
    const CONTACT: &[Field] = &[Name, Phone, Email];
    [
        grant(
            Reader::Messaging,
            Purpose::Send,
            ALL,
            "vault_read.messaging.send",
        ),
        grant(
            Reader::Messaging,
            Purpose::Inbound,
            &[Name, Phone],
            "vault_read.messaging.inbound",
        ),
        grant(
            Reader::Calls,
            Purpose::BridgeCall,
            &[Phone],
            "vault_read.calls.bridge_call",
        ),
        grant(
            Reader::Calls,
            Purpose::CallReadback,
            &[Name, Address],
            "vault_read.calls.call_readback",
        ),
        grant(
            Reader::Picks,
            Purpose::Release,
            &[Name, Email, Address, OnsiteContact],
            "vault_read.picks.release",
        ),
        grant(
            Reader::PortalsHomeowner,
            Purpose::OwnRecord,
            ALL,
            "vault_read.portals_homeowner.own_record",
        ),
        grant(
            Reader::PortalsOwner,
            Purpose::CallbackAlert,
            CONTACT,
            "vault_read.portals_owner.callback_alert",
        ),
        grant(
            Reader::PortalsOwner,
            Purpose::HelpRequest,
            CONTACT,
            "vault_read.portals_owner.help_request",
        ),
        grant(
            Reader::Privacy,
            Purpose::Rotation,
            ALL,
            "vault_read.privacy.rotation",
        ),
    ]
};

const fn grant(
    reader: Reader,
    purpose: Purpose,
    fields: &'static [Field],
    action: &'static str,
) -> Grant {
    Grant {
        reader,
        purpose,
        fields,
        action,
    }
}

/// The grant for `reader` and `purpose` if it covers every field in `fields` (PRIV-004, SEC-047).
#[must_use]
pub fn grant_for(reader: Reader, purpose: Purpose, fields: &[Field]) -> Option<&'static Grant> {
    GRANTS
        .iter()
        .find(|g| g.reader == reader && g.purpose == purpose)
        .filter(|g| fields.iter().all(|f| g.fields.contains(f)))
}

/// A new contact's fields, plaintext only on the way in; none of them is ever logged (PRIV-016).
#[derive(Debug, Default)]
pub struct NewContact {
    /// The name.
    pub name: Option<SecretString>,
    /// The phone number in any common US spelling; stored normalized.
    pub phone: Option<SecretString>,
    /// The email address.
    pub email: Option<SecretString>,
    /// The street address.
    pub address: Option<SecretString>,
    /// The on-site contact.
    pub onsite_contact: Option<SecretString>,
}

/// The fields a read opened, each `SecretString` so `Debug` prints none (PRIV-016); a field the read
/// did not ask for, or the contact does not have, is `None`.
#[derive(Debug, Default)]
pub struct ContactDetails {
    /// The contact's ID.
    pub id: Uuid,
    /// The name.
    pub name: Option<SecretString>,
    /// The phone number, as `+1` and ten digits.
    pub phone: Option<SecretString>,
    /// The email address.
    pub email: Option<SecretString>,
    /// The street address.
    pub address: Option<SecretString>,
    /// The on-site contact.
    pub onsite_contact: Option<SecretString>,
}

impl ContactDetails {
    /// The slot for `field`.
    pub(crate) fn slot(&mut self, field: Field) -> &mut Option<SecretString> {
        match field {
            Field::Name => &mut self.name,
            Field::Phone => &mut self.phone,
            Field::Email => &mut self.email,
            Field::Address => &mut self.address,
            Field::OnsiteContact => &mut self.onsite_contact,
        }
    }
}

/// What a blind-index digest is for; each purpose has its own derived key (PRIV-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexPurpose {
    /// A homeowner's phone in the contact vault.
    HomeownerPhone,
    /// A business or staff phone in `messaging`'s sender directory.
    SenderDir,
    /// An email sender (PRIV-026).
    Email,
}

impl IndexPurpose {
    /// The stored purpose and the label its key is derived from (`homeowner-phone|v1`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::HomeownerPhone => "homeowner-phone",
            Self::SenderDir => "sender-dir",
            Self::Email => "email",
        }
    }
}

/// Normalizes a US phone number to `+1` and ten digits, the only form the blind index hashes.
///
/// # Errors
///
/// [`VaultError::BadPhone`] when it is not ten digits, or eleven starting with 1.
pub fn normalize_phone(raw: &str) -> Result<String, VaultError> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let ten = match digits.len() {
        10 => digits.as_str(),
        11 if digits.starts_with('1') => &digits[1..],
        _ => return Err(VaultError::BadPhone),
    };
    let bad_char = raw
        .chars()
        .any(|c| !(c.is_ascii_digit() || " +-.()".contains(c)));
    if bad_char || ten.starts_with(['0', '1']) {
        return Err(VaultError::BadPhone);
    }
    Ok(format!("+1{ten}"))
}
