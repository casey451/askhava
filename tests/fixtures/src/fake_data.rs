//! The fake-data generator and the fixed sentinel values (PRIV-019, D-109, TD-PRIV-02).
//!
//! Every function is deterministic: the same index always gives the same
//! value, so a failing test can be rerun exactly. Phones are E.164 numbers in
//! the 555-0100 to 555-0198 block under Arizona area codes, because 555-01xx
//! numbers are reserved for fiction in every area code (D-109). The sentinel
//! phone takes the last number of the block, 555-0199, and the sentinel name,
//! email and street are never generated, so a sentinel found where it should
//! not be is always a leak and never an ordinary fixture.

/// Fake full names, all on the PII scanner's fake-name list (PRIV-019, D-109).
///
/// The sentinel name is deliberately not here; `fake_name` cycles through
/// these in order.
pub const FAKE_NAMES: [&str; 7] = [
    "Ada Example",
    "Sam Sample",
    "Pat Placeholder",
    "Alex Fakeperson",
    "Test Homeowner",
    "Test Owner",
    "Test User",
];

/// Arizona area codes used for fake phones, so a fixture number has a
/// quiet-hours zone (QH-003, D-109).
pub const FAKE_AREA_CODES: [u16; 5] = [928, 602, 480, 520, 623];

/// The sentinel homeowner name planted to prove nothing leaks (TD-PRIV-02).
pub const SENTINEL_NAME: &str = "Zelda Canary";

/// The sentinel phone, E.164, the last number of the fictional block (TD-PRIV-02).
pub const SENTINEL_PHONE: &str = "+19285550199";

/// The sentinel email address (TD-PRIV-02).
pub const SENTINEL_EMAIL: &str = "canary@example.com";

/// The sentinel street address (TD-PRIV-02).
pub const SENTINEL_STREET: &str = "1 Canary Way";

/// First line number of the generated block (555-0100).
const FIRST_LINE: usize = 100;

/// How many line numbers the generator uses: 0100 to 0198; 0199 is the sentinel's.
const LINES_PER_AREA: usize = 99;

/// One fake person's contact details, every field fake (PRIV-019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakePerson {
    /// A name from [`FAKE_NAMES`].
    pub name: String,
    /// An E.164 phone in the 555-0100 to 555-0199 block.
    pub phone: String,
    /// An address at `example.com`.
    pub email: String,
    /// A street such as "1 Test Way".
    pub street: String,
}

/// The fake name for `index`, cycling through [`FAKE_NAMES`] (PRIV-019, D-109).
#[must_use]
pub fn fake_name(index: u32) -> &'static str {
    FAKE_NAMES[index as usize % FAKE_NAMES.len()]
}

/// The fake E.164 phone for `index`: `+1 <area> 555 01xx`, never the
/// sentinel's 0199 (PRIV-019, D-109). Distinct for 495 consecutive indexes.
#[must_use]
pub fn fake_phone(index: u32) -> String {
    let index = index as usize;
    let area = FAKE_AREA_CODES[(index / LINES_PER_AREA) % FAKE_AREA_CODES.len()];
    let line = FIRST_LINE + index % LINES_PER_AREA;
    format!("+1{area}555{line:04}")
}

/// The fake email for `index`, at `example.com` (PRIV-019, D-109).
#[must_use]
pub fn fake_email(index: u32) -> String {
    format!("person{index}@example.com")
}

/// The fake street address for `index`, such as "1 Test Way" (PRIV-019, D-109).
#[must_use]
pub fn fake_street(index: u32) -> String {
    format!("{} Test Way", u64::from(index) + 1)
}

/// A whole fake person for `index` (PRIV-019, D-109).
#[must_use]
pub fn fake_person(index: u32) -> FakePerson {
    FakePerson {
        name: fake_name(index).to_owned(),
        phone: fake_phone(index),
        email: fake_email(index),
        street: fake_street(index),
    }
}

/// The fixed sentinel person planted in journeys to prove no contact detail
/// leaves the vault (TD-PRIV-02, PRIV-019).
#[must_use]
pub fn sentinel_person() -> FakePerson {
    FakePerson {
        name: SENTINEL_NAME.to_owned(),
        phone: SENTINEL_PHONE.to_owned(),
        email: SENTINEL_EMAIL.to_owned(),
        street: SENTINEL_STREET.to_owned(),
    }
}
