//! The fake-data fixture crate (`askhava-fixtures`; PRIV-019, D-109).
//!
//! The one place code and tests get personal-looking data. Every value is
//! fake by construction: names from a fixed fake list, phones in the
//! fictional 555-0100 to 555-0199 block, emails at `example.com` and streets
//! such as "1 Test Way". `scripts/pii_scan.py` fails the build on anything
//! else, so a test that needs a name, phone, email or street takes it from
//! here instead of typing one. The sentinel values are fixed and never
//! produced by the generator, so a sentinel scan (TD-PRIV-02) can search for
//! them without matching ordinary fixtures.

pub mod fake_data;

pub use fake_data::{
    FAKE_AREA_CODES, FAKE_NAMES, FakePerson, SENTINEL_EMAIL, SENTINEL_NAME, SENTINEL_PHONE,
    SENTINEL_STREET, fake_email, fake_name, fake_person, fake_phone, fake_street, sentinel_person,
};
