//! The pure list readers in `src/imports.rs` (CARD-015, LIC-006, LIC-007, LIC-010): the header
//! check, skipped and counted malformed rows, the truncated-list refusals (D-905), quoted fields, field
//! limits, dates and statuses. Every license number, business and owner name here is fake.

use crate::imports::{
    CITY_HEADER, ImportError, ROC_HEADER, normalise_license_number, parse_city_list,
    parse_roc_list, shrank_too_far,
};

fn roc(rows: &[&str]) -> String {
    format!("{ROC_HEADER}\n{}", rows.join("\n"))
}

fn city(rows: &[&str]) -> String {
    format!("{CITY_HEADER}\n{}", rows.join("\n"))
}

#[test]
fn roc_rows_take_class_status_and_date_from_the_list() {
    let text = roc(&[
        "roc 000001,cr-37,Active,2015-03-02",
        "ROC000002,B,SUSPENDED,",
        "",
        "ROC000003,C-11,expired,2020-01-01\r",
        "ROC000004,C-39,Revoked,",
        "ROC000005,K-21,cancelled,",
        "ROC000006,L-37,inactive,",
    ]);
    let list = parse_roc_list(&text).expect("a good list");
    assert_eq!(list.skipped, 0);
    assert_eq!(list.rows.len(), 6);
    let first = &list.rows[0];
    assert_eq!(first.license_number, "ROC000001");
    assert_eq!(first.class_code, "CR-37");
    assert_eq!(first.status.code(), "active");
    assert!(first.status.is_active());
    assert_eq!(first.issued_on.map(|d| d.year()), Some(2015));
    assert!(list.rows[1..].iter().all(|r| r.status.code() == "inactive"));
    assert!(list.rows[1..].iter().all(|r| !r.status.is_active()));
    assert_eq!(list.rows[1].issued_on, None);
}

#[test]
fn malformed_roc_rows_are_skipped_and_counted() {
    let text = roc(&[
        "ROC000001,CR-37,active,2015-03-02",
        "ROC000002,CR-37,active,2015-03-02",
        "ROC000003,CR-37,active,2015-03-02",
        "ROC000004,CR-37,active,2015-03-02",
        "ROC000005,CR-37,active,2015-03-02",
        "ROC000006,CR-37,active,2015-03-02",
        ",CR-37,active,",
        "ROC000007,,active,",
        "ROC000008,CR-37,pending,",
        "ROC000009,CR-37,active,not-a-date",
        "ROC000010,CR-37,active",
        "ROC000011,\"CR-37,active,",
    ]);
    let list = parse_roc_list(&text).expect("more good rows than bad");
    assert_eq!(list.rows.len(), 6);
    assert_eq!(list.skipped, 6);
}

#[test]
fn over_long_fields_are_malformed() {
    let long_number = "9".repeat(65);
    let long_class = "C".repeat(17);
    let r1 = format!("{long_number},CR-37,active,");
    let r2 = format!("ROC000002,{long_class},active,");
    let text = roc(&[
        "ROC000001,CR-37,active,",
        "ROC000003,CR-37,active,",
        &r1,
        &r2,
    ]);
    let list = parse_roc_list(&text).expect("two good, two bad");
    assert_eq!((list.rows.len(), list.skipped), (2, 2));
}

#[test]
fn a_changed_header_rejects_the_whole_list() {
    let text = "number,class,status,issued_on\nROC000001,CR-37,active,";
    assert_eq!(parse_roc_list(text).err(), Some(ImportError::BadHeader));
    assert_eq!(parse_roc_list("").err(), Some(ImportError::BadHeader));
    let city_text = format!("{ROC_HEADER}\nROC000001,CR-37,active,");
    assert_eq!(
        parse_city_list(&city_text).err(),
        Some(ImportError::BadHeader)
    );
}

#[test]
fn empty_or_mostly_malformed_lists_are_refused() {
    assert_eq!(
        parse_roc_list(ROC_HEADER).err(),
        Some(ImportError::TooFewRows)
    );
    let text = roc(&["ROC000001,CR-37,active,", "bad", "worse,row"]);
    assert_eq!(parse_roc_list(&text).err(), Some(ImportError::TooFewRows));
    let even = roc(&["ROC000001,CR-37,active,", "bad"]);
    assert_eq!(parse_roc_list(&even).map(|l| l.skipped).ok(), Some(1));
    let empty_city = city(&["", "  "]);
    assert_eq!(
        parse_city_list(&empty_city).err(),
        Some(ImportError::TooFewRows)
    );
    let truncated_city = city(&["CL-1,Good One,,,", "CL-2,,,,", "CL-3,\"Cut off"]);
    assert_eq!(
        parse_city_list(&truncated_city).err(),
        Some(ImportError::TooFewRows)
    );
}

#[test]
fn a_list_under_half_the_previous_one_is_too_shrunk() {
    assert!(
        !shrank_too_far(1, None),
        "the first list has nothing to compare"
    );
    assert!(shrank_too_far(100, Some(10_000)), "a clean truncation");
    assert!(shrank_too_far(4, Some(9)));
    assert!(!shrank_too_far(5, Some(10)), "exactly half is kept");
    assert!(!shrank_too_far(12_000, Some(10_000)), "growth is kept");
    assert!(shrank_too_far(0, Some(1)));
    assert!(!shrank_too_far(u32::MAX, Some(u32::MAX)));
}

#[test]
fn city_rows_keep_names_and_honour_quotes() {
    let text = city(&[
        "CL-1,\"Fake Fixers, LLC\",\"The \"\"Fake\"\" Shop\",Pat Example,2019-06-01",
        "  ,Sample Services,,,",
    ]);
    let list = parse_city_list(&text).expect("a good city list");
    assert_eq!(list.skipped, 0);
    let first = &list.rows[0].record;
    assert_eq!(first.business_name, "Fake Fixers, LLC");
    assert_eq!(first.trade_name.as_deref(), Some("The \"Fake\" Shop"));
    assert_eq!(first.owner_name.as_deref(), Some("Pat Example"));
    assert_eq!(first.license_number.as_deref(), Some("CL-1"));
    assert_eq!(list.rows[0].issued_on.map(|d| d.year()), Some(2019));
    let second = &list.rows[1];
    assert_eq!(second.record.business_name, "Sample Services");
    assert!(second.record.trade_name.is_none() && second.record.owner_name.is_none());
    assert!(second.record.license_number.is_none() && second.issued_on.is_none());
}

#[test]
fn malformed_city_rows_are_skipped_and_counted() {
    let long_name = "N".repeat(201);
    let long_number = "1".repeat(65);
    let rows = [
        "CL-1,Good One,,,".to_owned(),
        "CL-2,Good Two,,,".to_owned(),
        "CL-3,Good Three,,,".to_owned(),
        "CL-4,Good Four,,,".to_owned(),
        "CL-5,Good Five,,,".to_owned(),
        "CL-10,Good Six,,,".to_owned(),
        "CL-11,Too Few Fields,,".to_owned(),
        "CL-6,,,,".to_owned(),
        format!("CL-7,Fine,{long_name},,"),
        format!("CL-8,Fine,,{long_name},"),
        format!("{long_number},Fine,,,"),
        "CL-9,Fine,,,2020-13-40".to_owned(),
    ];
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let list = parse_city_list(&city(&refs)).expect("six good, six bad");
    assert_eq!((list.rows.len(), list.skipped), (6, 6));
}

#[test]
fn license_numbers_are_upper_case_without_spaces() {
    assert_eq!(normalise_license_number(" roc 12\t34 "), "ROC1234");
}

#[test]
fn errors_carry_no_values() {
    let all = [
        ImportError::BadHeader,
        ImportError::TooFewRows,
        ImportError::NoList,
        ImportError::Database,
    ];
    for e in all {
        let text = e.to_string();
        assert!(!text.is_empty() && !text.chars().any(|c| c.is_ascii_digit()));
    }
}
