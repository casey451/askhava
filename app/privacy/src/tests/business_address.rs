//! A business notice address that could read as two recipients is refused (TD-SEC-77, SEC-025, D-4327, D-4822).
//!
//! `bare_address` is the gate every sign-in address passes before it is sealed and later sent as
//! the email provider's `to`; fake addresses only.

use crate::business_contacts::{BusinessContactError, bare_address};

/// TD-SEC-77: each refused character inside an address is a bad address.
#[test]
fn td_sec_77_a_split_or_control_character_is_a_bad_address() {
    for refused in ['\r', '\n', '\t', ' ', ',', ';', '<', '>'] {
        let typed = format!("owner{refused}x@example.test");
        assert_eq!(
            bare_address(&typed),
            Err(BusinessContactError::BadAddress),
            "{refused:?}"
        );
    }
    let two = "owner@example.test, other@example.test";
    assert_eq!(bare_address(two), Err(BusinessContactError::BadAddress));
    let nul = "owner\u{0}@example.test";
    assert_eq!(bare_address(nul), Err(BusinessContactError::BadAddress));
}

/// TD-SEC-77: only a well-formed `Name <addr>` is stripped; anything else is checked as typed.
#[test]
fn td_sec_77_only_a_well_formed_name_form_is_stripped() {
    for typed in [
        "own<er@example.test",
        "Name <owner@example.test",
        "Name <>",
        "Name < >",
        "A <b> <owner@example.test>",
        "Name <owner@example.test>>",
        "Name <owner@example.test> trailing",
    ] {
        assert_eq!(
            bare_address(typed),
            Err(BusinessContactError::BadAddress),
            "{typed:?}"
        );
    }
}

/// TD-SEC-77: a valid address still reads bare and lower-cased.
#[test]
fn td_sec_77_a_valid_address_still_reads_bare() {
    for typed in [
        "Name <owner@example.test>",
        " OWNER@Example.test ",
        "<owner@example.test>",
    ] {
        assert_eq!(
            bare_address(typed).as_deref(),
            Ok("owner@example.test"),
            "{typed:?}"
        );
    }
}
