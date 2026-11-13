//! The sign-up read of a business's own public record (T385, D-2954; BIZ-002, BIZ-016, LIC-008,
//! LIC-010, LIC-011, LIC-048).
//!
//! [`signup_record`] is the one composite the sign-up (T302) calls on the screens that need the
//! name check or a record contact. It takes no number from the request: an ROC business is read
//! under the number `licensing::api::check_license` bound at the license match
//! (`bound_roc_number`); a city-list or pest business under the number its stored names
//! (`businesses.business_name`, T165) and stored city license number match on the latest city list
//! (`city_match_number`). It makes one `licensing::api::read_own_record` and gives the BIZ-016 name
//! flag, the [`RecordContacts`], the full record address for [`crate::signup::RecordSeen`] and the
//! masked contact. Nothing from the record is stored or logged (LIC-010, PRIV-016, D-1699).

use askhava_adapters::api::LicenseRecords;
use askhava_catalog::api::category;
use askhava_licensing::api::{
    OwnRecord, RecordList, RecordRead, bound_roc_number, city_match_number, names_agree,
    read_own_record,
};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::identity::{Flag, RecordContacts, Step, next_step};
use crate::links::{BusinessNames, business_names};
use crate::models::{self, BusinessesError};
use crate::signup::{MaskedContact, RecordSeen};

/// The catalog license kinds read on the ROC record (LIC-006).
const ROC_KINDS: [&str; 2] = ["roc", "roc_and_state_agency"];

/// A record that was read with at least one usable contact (BIZ-002, BIZ-016). No `Debug`: it
/// holds the record's contacts (PRIV-016).
pub struct RecordFound {
    flag: Option<Flag>,
    contacts: RecordContacts,
    address: Option<SecretString>,
    masked: Option<MaskedContact>,
}

impl RecordFound {
    /// BIZ-016: `Some(Flag::NameMismatch)` for an ROC business whose stored names agree with
    /// neither record name, or that stored no name; none for a city-list business (LIC-011 is
    /// its name match).
    #[must_use]
    pub const fn flag(&self) -> Option<Flag> {
        self.flag
    }

    /// BIZ-002: the record contacts for the identity chain.
    #[must_use]
    pub const fn contacts(&self) -> &RecordContacts {
        &self.contacts
    }

    /// BIZ-026, D-1712: the record contacts with the full record address.
    #[must_use]
    pub fn seen(&self) -> RecordSeen<'_> {
        RecordSeen {
            contacts: &self.contacts,
            address: self.address.as_ref(),
        }
    }

    /// BIZ-026: the masked contact for the step reached (the stored step, else the first step the
    /// record can take), or the masked landline (D-366) before any step when the record phone is
    /// one.
    #[must_use]
    pub const fn masked(&self) -> Option<&MaskedContact> {
        self.masked.as_ref()
    }
}

/// What one sign-up record read gave (BIZ-002, BIZ-016, LIC-008).
pub enum SignupRecord {
    /// The record, with at least one usable contact.
    Read(RecordFound),
    /// No record, no single matching city number, or no usable contact: `no_record_contact`.
    NoContact,
    /// The read failed or misread; nothing changes and the business tries again (LIC-008).
    Unreadable,
    /// An ROC business with no number bound by a license check yet.
    Unbound,
}

impl SignupRecord {
    /// BIZ-016, D-059: the flag this read sends the sign-up to review with, if any.
    #[must_use]
    pub const fn flag(&self) -> Option<Flag> {
        match self {
            Self::Read(found) => found.flag,
            Self::NoContact => Some(Flag::NoRecordContact),
            Self::Unreadable | Self::Unbound => None,
        }
    }
}

/// BIZ-002, BIZ-016, LIC-008, LIC-010, LIC-011, LIC-048, D-2954: one read of `business`'s own
/// record through `source`, under the ROC number bound at the license match (an ROC business:
/// one whose sign-up trades include a category licensed by an ROC class) or the city number its
/// stored names match; see [`SignupRecord`].
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] with no sign-up row; [`BusinessesError::Database`].
pub async fn signup_record(
    conn: &mut PgConnection,
    source: &impl LicenseRecords,
    business: Uuid,
) -> Result<SignupRecord, BusinessesError> {
    let names = business_names(conn, business).await?;
    let bound = bound_roc_number(conn, business).await.map_err(db)?;
    let roc = bound.is_some() || is_roc_business(conn, business).await?;
    let (list, number) = match (roc, bound) {
        (true, Some(number)) => (RecordList::Roc, number),
        (true, None) => return Ok(SignupRecord::Unbound),
        (false, _) => match city_number(conn, names.as_ref()).await? {
            Some(number) => (RecordList::City, number),
            None => return Ok(SignupRecord::NoContact),
        },
    };
    let read = read_own_record(conn, source, list, &number).await;
    let record = match read.map_err(db)? {
        RecordRead::Found(record) => record,
        RecordRead::NotFound => return Ok(SignupRecord::NoContact),
        RecordRead::Unreadable => return Ok(SignupRecord::Unreadable),
    };
    let flag = (roc && !stored_names_agree(&record, names.as_ref())).then_some(Flag::NameMismatch);
    found(conn, business, &record, flag).await
}

fn db<E>(_: E) -> BusinessesError {
    BusinessesError::Database
}

/// Whether any category the business chose at sign-up is licensed by an ROC class.
async fn is_roc_business(conn: &mut PgConnection, business: Uuid) -> Result<bool, BusinessesError> {
    let trades = sqlx::query_scalar!(
        "SELECT trades FROM businesses.signup WHERE business_id = $1",
        business,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(db)?
    .ok_or(BusinessesError::UnknownBusiness)?;
    for code in trades {
        let kind = category(conn, &code).await.map_err(db)?;
        if kind.is_some_and(|c| ROC_KINDS.contains(&c.license_kind.as_str())) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// LIC-011, LIC-048: the city number the stored names and entered number match, if any.
async fn city_number(
    conn: &mut PgConnection,
    names: Option<&BusinessNames>,
) -> Result<Option<String>, BusinessesError> {
    let Some(names) = names else {
        return Ok(None);
    };
    let given = (names.business_name.as_str(), names.trade_name.as_deref());
    let entered = names.city_license_number.as_deref();
    city_match_number(conn, given, entered).await.map_err(db)
}

/// BIZ-016: whether the stored names agree with the record's; no stored name never agrees.
fn stored_names_agree(record: &OwnRecord, names: Option<&BusinessNames>) -> bool {
    names.is_some_and(|n| names_agree(record, &n.business_name, n.trade_name.as_deref()))
}

/// BIZ-002, BIZ-026: the read record as contacts, address and masked contact, or `NoContact`.
async fn found(
    conn: &mut PgConnection,
    business: Uuid,
    record: &OwnRecord,
    flag: Option<Flag>,
) -> Result<SignupRecord, BusinessesError> {
    let address = record.mailing_address();
    let contacts = RecordContacts {
        texting_phone: record.texting_phone().cloned(),
        email: record.email().cloned(),
        has_address: address.is_some(),
    };
    let stored = models::signup_state(conn, business).await?.step;
    let stored = [Step::Text, Step::Email, Step::Letter]
        .into_iter()
        .find(|s| stored.as_deref() == Some(s.code()));
    let Some(first) = next_step(&contacts, None) else {
        return Ok(SignupRecord::NoContact);
    };
    let step = stored.unwrap_or(first);
    let place = (record.town(), record.zip());
    let masked = match (stored, record.landline()) {
        (None, Some(landline)) => Some(MaskedContact::phone(landline, true)),
        _ => MaskedContact::for_step(&contacts, step, place),
    };
    Ok(SignupRecord::Read(RecordFound {
        flag,
        contacts,
        address,
        masked,
    }))
}
