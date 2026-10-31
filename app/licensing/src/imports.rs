//! Reading the daily ROC and city license lists (CARD-015, LIC-006, LIC-007, LIC-010).
//!
//! Pure: text in, rows out; `service` stores them. A list is comma-separated with a fixed header
//! line (fields may be double-quoted); a wrong header rejects the whole list, so a changed file
//! layout never changes a status (LIC-008), while a malformed row is skipped and counted. A list with
//! no usable row, or with more malformed rows than good ones, is refused whole (a truncated download
//! must never become the latest list); `service` also refuses a list under half the size of the
//! previous one (`shrank_too_far`, D-905). ROC license numbers are normalised (upper case, no spaces)
//! here and at every lookup. Only the columns verification needs are read; any other column (an
//! address or a phone) is dropped here and never stored (LIC-010).

use jiff::civil::Date;
use thiserror::Error;

use crate::models::CityRecord;

/// The ROC list's header: one row per license class (LIC-006, LIC-007).
pub const ROC_HEADER: &str = "license_number,class,status,issued_on";
/// The city business-license list's header (LIC-011).
pub const CITY_HEADER: &str = "license_number,business_name,trade_name,owner_name,issued_on";

/// Why an import or check could not run. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImportError {
    /// The first line is not the list's header: the layout changed, so nothing is recorded.
    #[error("the list header is not the expected one")]
    BadHeader,
    /// The list has no good row, more malformed rows than good ones, or fewer than half the rows
    /// the previous list of its source recorded (D-905): it looks truncated or misread, so nothing
    /// is recorded and the previous list stays the latest (LIC-008).
    #[error("the list has too few good rows")]
    TooFewRows,
    /// No list of this source has been imported yet, so no daily check can run (LIC-008).
    #[error("no list has been imported")]
    NoList,
    /// The database refused a statement.
    #[error("the licensing database step failed")]
    Database,
}

/// A license's standing on the ROC list (LIC-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RocStatus {
    /// The list shows it Active.
    Active,
    /// Suspended, expired, revoked, cancelled or inactive.
    Inactive,
}

impl RocStatus {
    /// The `licensing.agency_record.status` code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
        }
    }

    /// Whether this is the lenient standing, so a repeated row keeps the stricter one (LIC-007).
    pub(crate) const fn is_active(self) -> bool {
        match self {
            Self::Active => true,
            Self::Inactive => false,
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "inactive" | "suspended" | "expired" | "revoked" | "cancelled" => Some(Self::Inactive),
            _ => None,
        }
    }
}

/// A license number as stored and looked up: upper case with every space removed (LIC-007).
#[must_use]
pub fn normalise_license_number(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// One ROC list row: the state record a business's class comes from (LIC-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RocRow {
    /// The license number, normalised by [`normalise_license_number`].
    pub license_number: String,
    /// The license class, upper case (such as `CR-37`).
    pub class_code: String,
    /// Its standing.
    pub status: RocStatus,
    /// The original issue date, when the list gives one (LIC-027, LIC-037).
    pub issued_on: Option<Date>,
}

/// One city list row. No `Debug`: it holds business and owner names (PRIV-016).
#[derive(Clone)]
pub struct CityRow {
    /// The names and city license number matched under LIC-011.
    pub record: CityRecord,
    /// The issue date, when the list gives one (LIC-038).
    pub issued_on: Option<Date>,
}

/// The well-formed rows of one list and how many malformed rows were skipped.
#[derive(Clone)]
pub struct ParsedList<T> {
    /// The rows to record.
    pub rows: Vec<T>,
    /// The malformed rows skipped.
    pub skipped: u32,
}

/// Reads an ROC list (LIC-006, LIC-007): a row needs a number, a class, a known status and an empty
/// or `YYYY-MM-DD` date, else it is skipped and counted.
///
/// # Errors
///
/// [`ImportError::BadHeader`] when the first line is not [`ROC_HEADER`]; [`ImportError::TooFewRows`]
/// when no row is good or more rows are malformed than good.
pub fn parse_roc_list(text: &str) -> Result<ParsedList<RocRow>, ImportError> {
    parse(text, ROC_HEADER, |f| {
        let [number, class, status, issued] = f else {
            return None;
        };
        Some(RocRow {
            license_number: field(&normalise_license_number(number), 64)?,
            class_code: field(class, 16)?.to_ascii_uppercase(),
            status: RocStatus::parse(status)?,
            issued_on: date(issued)?,
        })
    })
}

/// Reads the city business-license list (LIC-010, LIC-011): a row needs a business name; the other
/// names and the number are optional.
///
/// # Errors
///
/// [`ImportError::BadHeader`] when the first line is not [`CITY_HEADER`]; [`ImportError::TooFewRows`]
/// as for [`parse_roc_list`].
pub fn parse_city_list(text: &str) -> Result<ParsedList<CityRow>, ImportError> {
    parse(text, CITY_HEADER, |f| {
        let [number, business, trade, owner, issued] = f else {
            return None;
        };
        let record = CityRecord {
            business_name: field(business, 200)?,
            trade_name: optional(trade, 200)?,
            owner_name: optional(owner, 200)?,
            license_number: optional(number, 64)?,
        };
        Some(CityRow {
            record,
            issued_on: date(issued)?,
        })
    })
}

fn parse<T>(
    text: &str,
    header: &str,
    row: impl Fn(&[String]) -> Option<T>,
) -> Result<ParsedList<T>, ImportError> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    if lines.next().map(str::trim) != Some(header) {
        return Err(ImportError::BadHeader);
    }
    let mut parsed = ParsedList {
        rows: Vec::new(),
        skipped: 0,
    };
    for line in lines {
        match split(line).as_deref().and_then(&row) {
            Some(r) => parsed.rows.push(r),
            None => parsed.skipped += 1,
        }
    }
    let good = u32::try_from(parsed.rows.len()).unwrap_or(u32::MAX);
    if good == 0 || parsed.skipped > good {
        return Err(ImportError::TooFewRows);
    }
    Ok(parsed)
}

/// Whether a list recording `recorded` rows shrank to under half of the previous list's
/// `previous` recorded rows, so it is refused as a clean truncation (LIC-008, D-905).
pub(crate) fn shrank_too_far(recorded: u32, previous: Option<u32>) -> bool {
    previous.is_some_and(|before| u64::from(recorded) * 2 < u64::from(before))
}

/// Splits one line on commas, honouring double-quoted fields; `None` on an unclosed quote.
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

fn field(text: &str, max: usize) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= max).then(|| text.to_owned())
}

fn optional(text: &str, max: usize) -> Option<Option<String>> {
    if text.trim().is_empty() {
        Some(None)
    } else {
        field(text, max).map(Some)
    }
}

fn date(text: &str) -> Option<Option<Date>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    text.parse::<Date>().ok().map(Some)
}
