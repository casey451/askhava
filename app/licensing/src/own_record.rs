//! The sign-up read of a business's own public record: its names for BIZ-016's name check and
//! its contacts for the identity code (T385, D-2954; BIZ-002, BIZ-016, LIC-008, LIC-010, LIC-011,
//! LIC-048).
//!
//! [`read_own_record`] makes one `adapters::api::LicenseRecords::lookup_record` call for one
//! license number, never retried, and reads one row of [`RECORD_HEADER`] into an [`OwnRecord`]
//! (no `Debug`; every contact a `SecretString`). Nothing from the record is written anywhere: no
//! table, audit row, log line or error holds a name, number or contact (LIC-010, PRIV-016,
//! D-1699); the only write is the subject-less `licensing.record_unreadable` alert, at most one per
//! Arizona day, when a read fails or misreads (a misread never counts, LIC-008).
//! [`city_match_number`] finds the city license number a no-license or pest business's record is
//! read under, and [`bound_roc_number`] the ROC number `check_license` bound at the license match.
//! LIC-011's comparison is copied here from `rules.rs` (T347's), never edited there (D-3094).

use askhava_adapters::api::{AdapterError, LicenseRecords};
use askhava_ops::api::{Severity, raise_alert_once_a_day};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::imports::{ImportError, normalise_license_number};

/// The one-row record format a record read answers in (D-2954). `phone_kind` is `mobile`,
/// `landline` or empty; any field but the number and the business name may be empty.
pub const RECORD_HEADER: &str =
    "license_number,business_name,trade_name,phone,phone_kind,email,address,town,zip";

/// The `ops` alert kind of an unreadable own-record read (LIC-008, D-2954): Normal, no subject.
pub const RECORD_UNREADABLE_ALERT: &str = "licensing.record_unreadable";

/// Which public list a record is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordList {
    /// The state ROC record.
    Roc,
    /// Lake Havasu City's business-license list (LIC-011, LIC-048).
    City,
}

impl RecordList {
    /// The list name the adapter reads under (`record/<list>/<number>`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Roc => "roc",
            Self::City => "city",
        }
    }

    /// A number as this list stores it: ROC upper case with no spaces, the city's trimmed.
    fn normalise(self, number: &str) -> String {
        match self {
            Self::Roc => normalise_license_number(number),
            Self::City => number.trim().to_owned(),
        }
    }
}

/// One business's own public record (BIZ-002, BIZ-016). No `Debug`: it holds names and contacts
/// (PRIV-016); no qualifying-party or owner name is read.
pub struct OwnRecord {
    business_name: String,
    trade_name: Option<String>,
    phone: Option<SecretString>,
    landline: bool,
    email: Option<SecretString>,
    street: Option<String>,
    town: String,
    zip: String,
}

impl OwnRecord {
    /// The record's business name.
    #[must_use]
    pub fn business_name(&self) -> &str {
        &self.business_name
    }

    /// The record's trade name (DBA), if any.
    #[must_use]
    pub fn trade_name(&self) -> Option<&str> {
        self.trade_name.as_deref()
    }

    /// BIZ-002, D-366: the record phone when it can get texts; a phone marked `landline` is left
    /// out, and a phone of unknown kind is tried by text first.
    #[must_use]
    pub fn texting_phone(&self) -> Option<&SecretString> {
        self.phone.as_ref().filter(|_| !self.landline)
    }

    /// BIZ-026, D-366: the record phone when it is marked `landline`, for the masked line only;
    /// never texted or called.
    #[must_use]
    pub fn landline(&self) -> Option<&SecretString> {
        self.phone.as_ref().filter(|_| self.landline)
    }

    /// The record email, if any.
    #[must_use]
    pub fn email(&self) -> Option<&SecretString> {
        self.email.as_ref()
    }

    /// The full mailing address (street, town and ZIP) when the record has a street address;
    /// without one there is no letter step (BIZ-002).
    #[must_use]
    pub fn mailing_address(&self) -> Option<SecretString> {
        let street = self.street.as_deref()?;
        let line = format!("{street}, {} {}", self.town, self.zip);
        Some(SecretString::from(line.trim().to_owned()))
    }

    /// The record's town, possibly empty.
    #[must_use]
    pub fn town(&self) -> &str {
        &self.town
    }

    /// The record's ZIP code, possibly empty.
    #[must_use]
    pub fn zip(&self) -> &str {
        &self.zip
    }
}

/// What one own-record read gave (LIC-008).
pub enum RecordRead {
    /// The record, read as one row for the number asked.
    Found(OwnRecord),
    /// The source has no record for the number (a 404).
    NotFound,
    /// The read failed or misread; nothing from it counts.
    Unreadable,
}

/// BIZ-002, BIZ-016, LIC-008, LIC-010: one read of license `number`'s own record on `list`, never
/// retried. `NotFound` only for a vendor 404; any other error, a wrong header, not exactly one row
/// or a row for another number is `Unreadable` and raises one Normal [`RECORD_UNREADABLE_ALERT`]
/// with no subject, at most one per Arizona day. Nothing else is written or logged with a value.
///
/// # Errors
///
/// [`ImportError::Database`] when the alert cannot be written.
pub async fn read_own_record(
    conn: &mut PgConnection,
    source: &impl LicenseRecords,
    list: RecordList,
    number: &str,
) -> Result<RecordRead, ImportError> {
    let number = list.normalise(number);
    let read = match source.lookup_record(list.code(), &number).await {
        Ok(answer) => parse_record(list, &number, &answer).map(RecordRead::Found),
        Err(AdapterError::Vendor { status: 404, .. }) => Some(RecordRead::NotFound),
        Err(_) => None,
    };
    if let Some(read) = read {
        return Ok(read);
    }
    tracing::warn!(list = list.code(), "own record unreadable");
    raise_alert_once_a_day(conn, RECORD_UNREADABLE_ALERT, Severity::Normal)
        .await
        .map_err(|_| ImportError::Database)?;
    Ok(RecordRead::Unreadable)
}

/// Reads the answer as [`RECORD_HEADER`] and exactly one well-formed row for `number`.
fn parse_record(list: RecordList, number: &str, answer: &[u8]) -> Option<OwnRecord> {
    let text = std::str::from_utf8(answer).ok()?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    if lines.next().map(str::trim) != Some(RECORD_HEADER) {
        return None;
    }
    let row = split(lines.next()?)?;
    if lines.next().is_some() {
        return None;
    }
    let [own, name, trade, phone, kind, email, street, town, zip] = row.as_slice() else {
        return None;
    };
    let landline = match kind.trim() {
        "landline" => true,
        "mobile" | "" => false,
        _ => return None,
    };
    let name = given(name)?;
    (list.normalise(own) == number).then(|| OwnRecord {
        business_name: name,
        trade_name: given(trade),
        phone: given(phone).map(SecretString::from),
        landline,
        email: given(email).map(SecretString::from),
        street: given(street),
        town: town.trim().to_owned(),
        zip: zip.trim().to_owned(),
    })
}

/// `text` trimmed, or none when blank.
fn given(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Splits one line on commas, honouring double-quoted fields; `None` on an unclosed quote (as the
/// list readers do).
fn split(line: &str) -> Option<Vec<String>> {
    let (mut fields, mut current, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.trim_end_matches('\r').chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    fields.push(current);
    (!quoted).then_some(fields)
}

/// LIC-011's comparison: lower case, letters and digits only, words joined by one space ("LLC"
/// and "Inc" are kept, so such a difference goes to review).
fn compared(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// BIZ-016, LIC-011: whether the business name or trade name the business gave equals the
/// record's business name or trade name (DBA) under LIC-011's comparison.
#[must_use]
pub fn names_agree(record: &OwnRecord, business_name: &str, trade_name: Option<&str>) -> bool {
    let theirs: Vec<String> = [Some(record.business_name()), record.trade_name()]
        .into_iter()
        .flatten()
        .map(compared)
        .filter(|n| !n.is_empty())
        .collect();
    [Some(business_name), trade_name]
        .into_iter()
        .flatten()
        .map(compared)
        .any(|n| !n.is_empty() && theirs.contains(&n))
}

/// BIZ-002, LIC-011, LIC-048: the city license number a no-license or pest business's record is
/// read under, from the latest city list's rows whose business, trade or owner name matches one
/// of `names` (business name, trade name). With an `entered` number, the matching row's number
/// that agrees with it (LIC-011's comparison; the list's form); else the one distinct number the
/// matching rows carry. `None` when nothing matches, the entered number agrees with no matching
/// row, a matching row has no number, the matching rows carry more than one distinct number, or
/// there is no city list.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn city_match_number(
    conn: &mut PgConnection,
    names: (&str, Option<&str>),
    entered: Option<&str>,
) -> Result<Option<String>, ImportError> {
    let own: Vec<String> = [Some(names.0), names.1]
        .into_iter()
        .flatten()
        .map(compared)
        .filter(|n| !n.is_empty())
        .collect();
    let rows = sqlx::query!(
        "SELECT e.business_name, e.trade_name, e.owner_name, e.license_number \
         FROM licensing.city_license_entry e WHERE e.import_id = (SELECT id FROM \
         licensing.list_import WHERE source = 'city' ORDER BY list_date DESC LIMIT 1)"
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| ImportError::Database)?;
    let matching: Vec<Option<String>> = rows
        .into_iter()
        .filter(|r| {
            [
                Some(&r.business_name),
                r.trade_name.as_ref(),
                r.owner_name.as_ref(),
            ]
            .into_iter()
            .flatten()
            .any(|n| own.contains(&compared(n)))
        })
        .map(|r| r.license_number.filter(|n| !n.trim().is_empty()))
        .collect();
    Ok(pick_number(&matching, entered))
}

/// The number [`city_match_number`] gives from the matching rows' numbers.
fn pick_number(matching: &[Option<String>], entered: Option<&str>) -> Option<String> {
    if let Some(entered) = entered {
        let want = compared(entered);
        return matching
            .iter()
            .flatten()
            .find(|n| compared(n) == want)
            .cloned();
    }
    let mut numbers: Vec<&String> = Vec::new();
    for number in matching {
        let number = number.as_ref()?;
        if !numbers.iter().any(|n| compared(n) == compared(number)) {
            numbers.push(number);
        }
    }
    match numbers.as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    }
}

/// BIZ-016, D-2954: the ROC number bound at the license match: the number of `business`'s latest
/// `licensing.license_check` row (written by `check_license`), or none before any check.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn bound_roc_number(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<String>, ImportError> {
    sqlx::query_scalar!(
        "SELECT license_number FROM licensing.license_check WHERE business_id = $1 \
         ORDER BY checked_at DESC, id DESC LIMIT 1",
        business,
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| ImportError::Database)
}
