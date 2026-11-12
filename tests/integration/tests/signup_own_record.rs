//! The sign-up read of a business's own public record (TD-LIC-30; T385, D-2954; BIZ-002, BIZ-016,
//! LIC-008, LIC-010, LIC-011, LIC-048).
//!
//! Fake records are written to a temporary `file://` directory and read through the real
//! `adapters::api::LicenseLists`: `licensing::api::read_own_record` (found, sparse, landline, a
//! 404, every unreadable shape and its one alert a day), `names_agree`, `city_match_number`, and
//! `businesses::api::signup_record` (the bound ROC number only, the name flag, the contacts and
//! masked contact, `NoContact`, `Unbound`, a city-list business); the settings key; and that no
//! read writes a `licensing` or `businesses` row or logs a name or contact. All data is fake.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::LicenseLists;
use askhava_businesses::api::{
    ContactKind, Flag, SignupRecord, create, save_business_name, signup_record,
};
use askhava_core::api::{MIGRATOR, Settings};
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    RECORD_HEADER, RECORD_UNREADABLE_ALERT, RecordList, RecordRead, check_license,
    city_match_number, import_city_list, import_roc_list, names_agree, read_own_record,
};
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::ExposeSecret;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const FULL: &str = "ROC100001,Example Plumbing Co,Sample Pipes,928-555-0142,mobile,office@example.test,100 Sample Way,Lake Havasu City,86403";
const SPARSE: &str = "ROC100002,Sample Drains LLC,,928-555-0143,,,,,";
const LANDLINE: &str =
    "ROC100003,Fake Heat Inc,,928-555-0144,landline,heat@example.test,,Lake Havasu City,86404";
const BARE: &str = "ROC100004,Placeholder Air Co,,,,,,,";
const LANDLINE_ONLY: &str = "ROC100005,Fake Cool Co,,928-555-0145,landline,,,,";
const CITY_RECORD: &str = "CL-100001,Sample Yard Care LLC,Sample Yards,928-555-0150,mobile,,,,";

const ROC_LIST: &str = "license_number,class,status,issued_on\n\
ROC100001,R-37,Active,2015-06-01\nROC100002,R-37,Active,2016-01-02\n\
ROC100003,R-37,Active,2016-08-09\nROC100004,R-37,Active,2017-03-04\n\
ROC100005,R-37,Active,2018-04-05\n";

const CITY_LIST: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-100001,Sample Yard Care LLC,Sample Yards,Fake Owner Person,2020-02-03\n\
CL-100002,Example Window Wash,,,2021-07-08\n\
CL-100003,Example Window Wash,,,2022-01-01\n\
,Fake Pool Folks,,,2019-05-05\n";

/// Every value a log line or error must never hold.
const SECRETS: [&str; 10] = [
    "Example Plumbing Co",
    "Sample Pipes",
    "555-0142",
    "office@example.test",
    "Sample Way",
    "ROC100001",
    "Sample Yard Care",
    "Sample Yards",
    "555-0150",
    "CL-100001",
];

/// A temporary fixture directory holding `record/<list>/<number>` files.
struct Fixtures(PathBuf);

impl Fixtures {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("askhava-records-{}", Uuid::new_v4()));
        for list in ["roc", "city"] {
            std::fs::create_dir_all(dir.join("record").join(list)).expect("fixture dir");
        }
        let fixtures = Self(dir);
        for row in [FULL, SPARSE, LANDLINE, BARE, LANDLINE_ONLY] {
            fixtures.record("roc", &row[..9], &format!("{RECORD_HEADER}\n{row}\n"));
        }
        fixtures.record(
            "city",
            "CL-100001",
            &format!("{RECORD_HEADER}\n{CITY_RECORD}\n"),
        );
        fixtures.record(
            "roc",
            "ROC200001",
            "license_number,business_name\nROC200001,Fake\n",
        );
        let twice = FULL.replace("ROC100001", "ROC200002");
        fixtures.record(
            "roc",
            "ROC200002",
            &format!("{RECORD_HEADER}\n{twice}\n{twice}\n"),
        );
        fixtures.record("roc", "ROC200003", &format!("{RECORD_HEADER}\n{FULL}\n"));
        let pager = FULL
            .replace("ROC100001", "ROC200004")
            .replace("mobile", "pager");
        fixtures.record("roc", "ROC200004", &format!("{RECORD_HEADER}\n{pager}\n"));
        let renamed = RECORD_HEADER.replace("trade_name", "dba");
        let valid = FULL.replace("ROC100001", "ROC200005");
        fixtures.record("roc", "ROC200005", &format!("{renamed}\n{valid}\n"));
        fixtures
    }

    fn record(&self, list: &str, number: &str, text: &str) {
        std::fs::write(self.0.join("record").join(list).join(number), text).expect("record");
    }

    fn source(&self) -> LicenseLists {
        LicenseLists::new(&format!("file://{}", self.0.display())).expect("file source")
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now() -> Timestamp {
    "2026-11-12T17:00:00Z".parse().expect("fake time")
}

async fn app(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn read(conn: &mut PgConnection, source: &LicenseLists, number: &str) -> RecordRead {
    read_own_record(conn, source, RecordList::Roc, number)
        .await
        .expect("read")
}

async fn unreadable_alerts(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = $1 AND subject_kind IS NULL \
         AND severity = 'normal'",
    )
    .bind(RECORD_UNREADABLE_ALERT)
    .fetch_one(pool)
    .await
    .expect("alerts")
}

/// TD-LIC-30 (BIZ-002, BIZ-016): the record's names, mobile phone, email and address read back;
/// a record with only a name and a phone reads with the rest empty.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_own_record_reads_names_and_contacts(pool: PgPool) {
    let (fixtures, mut conn) = (Fixtures::new(), app(&pool).await);
    let source = fixtures.source();
    let RecordRead::Found(full) = read(&mut conn, &source, "roc 100001").await else {
        panic!("the full record is found");
    };
    assert_eq!(full.business_name(), "Example Plumbing Co");
    assert_eq!(full.trade_name(), Some("Sample Pipes"));
    let phone = full.texting_phone().expect("mobile phone");
    assert_eq!(phone.expose_secret(), "928-555-0142");
    assert!(full.landline().is_none());
    assert_eq!(
        full.email().expect("email").expose_secret(),
        "office@example.test"
    );
    let address = full.mailing_address().expect("address");
    assert_eq!(
        address.expose_secret(),
        "100 Sample Way, Lake Havasu City 86403"
    );
    assert_eq!((full.town(), full.zip()), ("Lake Havasu City", "86403"));
    let RecordRead::Found(sparse) = read(&mut conn, &source, "ROC100002").await else {
        panic!("the sparse record is found");
    };
    assert_eq!(sparse.business_name(), "Sample Drains LLC");
    assert!(sparse.trade_name().is_none() && sparse.email().is_none());
    assert!(sparse.mailing_address().is_none() && sparse.texting_phone().is_some());
    assert_eq!((sparse.town(), sparse.zip()), ("", ""));
    assert_eq!(unreadable_alerts(&pool).await, 0);
}

/// TD-LIC-30 (D-366): a landline record gives no texting phone and shows the landline; a missing
/// record is `NotFound`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_landline_and_missing_record(pool: PgPool) {
    let (fixtures, mut conn) = (Fixtures::new(), app(&pool).await);
    let source = fixtures.source();
    let RecordRead::Found(landline) = read(&mut conn, &source, "ROC100003").await else {
        panic!("the landline record is found");
    };
    assert!(landline.texting_phone().is_none());
    assert_eq!(
        landline.landline().expect("landline").expose_secret(),
        "928-555-0144"
    );
    assert!(matches!(
        read(&mut conn, &source, "ROC999999").await,
        RecordRead::NotFound
    ));
    assert_eq!(unreadable_alerts(&pool).await, 0);
}

/// TD-LIC-30 (LIC-008): a failed read, a wrong header, two rows, a row for another number or an
/// unknown phone kind is `Unreadable`; the first raises one Normal alert with no subject and the
/// rest the same Arizona day raise none.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_unreadable_reads_raise_one_alert_a_day(pool: PgPool) {
    let (fixtures, mut conn) = (Fixtures::new(), app(&pool).await);
    let source = fixtures.source();
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    for number in [
        "../ROC100001",
        "ROC200001",
        "ROC200002",
        "ROC200003",
        "ROC200004",
        "ROC200005",
    ] {
        let got = read(&mut conn, &source, number).await;
        assert!(
            matches!(got, RecordRead::Unreadable),
            "{number} is unreadable"
        );
        assert_eq!(unreadable_alerts(&pool).await, 1);
    }
    let down = LicenseLists::new("http://127.0.0.1:9/lists").expect("closed port");
    assert!(matches!(
        read(&mut conn, &down, "ROC100001").await,
        RecordRead::Unreadable
    ));
    assert_eq!(unreadable_alerts(&pool).await, 1);
    lines.assert_clean();
}

/// TD-LIC-30 (LIC-008): more misread shapes (not text, empty, a header alone, a short row, no
/// business name, an unclosed quote) are `Unreadable`; quoted fields, a doubled quote, blank lines
/// and CRLF read, and a city record reads under its trimmed number.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_misread_shapes_and_quoted_fields(pool: PgPool) {
    let (fixtures, mut conn) = (Fixtures::new(), app(&pool).await);
    let dir = fixtures.0.join("record").join("roc");
    std::fs::write(dir.join("ROC300001"), [0xff_u8, 0xfe, 0x00]).expect("bytes");
    let shapes = [
        ("ROC300002", String::new()),
        ("ROC300003", format!("{RECORD_HEADER}\n")),
        (
            "ROC300004",
            format!("{RECORD_HEADER}\nROC300004,Fake Co,,,,,,\n"),
        ),
        (
            "ROC300005",
            format!("{RECORD_HEADER}\nROC300005, ,,,,,,,\n"),
        ),
        (
            "ROC300006",
            format!("{RECORD_HEADER}\nROC300006,\"Fake Co,,,,,,,\n"),
        ),
    ];
    for (number, text) in &shapes {
        fixtures.record("roc", number, text);
    }
    let quoted = "ROC300007,\"Fake \"\"Q\"\" Pipes, Co\",,,,,,,\r\n";
    fixtures.record(
        "roc",
        "ROC300007",
        &format!("\n{RECORD_HEADER}\r\n\n{quoted}\n"),
    );
    let source = fixtures.source();
    for number in [
        "ROC300001",
        "ROC300002",
        "ROC300003",
        "ROC300004",
        "ROC300005",
        "ROC300006",
    ] {
        let got = read(&mut conn, &source, number).await;
        assert!(matches!(got, RecordRead::Unreadable), "{number}");
    }
    assert_eq!(unreadable_alerts(&pool).await, 1);
    let RecordRead::Found(record) = read(&mut conn, &source, "ROC300007").await else {
        panic!("the quoted record is found");
    };
    assert_eq!(record.business_name(), "Fake \"Q\" Pipes, Co");
    assert!(record.texting_phone().is_none() && record.landline().is_none());
    let city = read_own_record(&mut conn, &source, RecordList::City, " CL-100001 ").await;
    let Ok(RecordRead::Found(city)) = city else {
        panic!("the city record is found");
    };
    assert_eq!(city.trade_name(), Some("Sample Yards"));
}

/// Seeds one Normal subject-less unreadable alert `offset` (an SQL interval) from the start of
/// today's Arizona day (UTC-7, by the database clock).
async fn seed_alert(pool: &PgPool, offset: &str) {
    sqlx::query(
        "INSERT INTO ops.alert (kind, severity, raised_at) SELECT $1, 'normal', \
           ((date_trunc('day', (now() AT TIME ZONE 'UTC') - interval '7 hours') \
             + interval '7 hours') AT TIME ZONE 'UTC') + $2::interval",
    )
    .bind(RECORD_UNREADABLE_ALERT)
    .bind(offset)
    .execute(pool)
    .await
    .expect("seeded alert");
}

/// TD-LIC-30 (LIC-008): the one-a-day limit follows the Arizona day: an alert from the previous
/// Arizona day, or from 06:00 UTC on today's Arizona date (still the day before in Arizona), lets
/// a new one be raised; one from earlier the same Arizona day suppresses it.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_unreadable_alert_follows_the_arizona_day(pool: PgPool) {
    let (fixtures, mut conn) = (Fixtures::new(), app(&pool).await);
    let source = fixtures.source();
    let cases = [("-12 hours", 2), ("-1 hour", 2), ("0 seconds", 1)];
    for (at, want) in cases {
        sqlx::query("DELETE FROM ops.alert WHERE kind = $1")
            .bind(RECORD_UNREADABLE_ALERT)
            .execute(&pool)
            .await
            .expect("clear alerts");
        seed_alert(&pool, at).await;
        let got = read(&mut conn, &source, "ROC200002").await;
        assert!(matches!(got, RecordRead::Unreadable), "{at}");
        assert_eq!(unreadable_alerts(&pool).await, want, "{at}");
    }
}

/// TD-LIC-30 (BIZ-016, LIC-011): a given name equal to the record's name or DBA, case and
/// punctuation aside, agrees; a different name, or one differing by "LLC", does not.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_names_agree_under_lic_011(pool: PgPool) {
    let (fixtures, mut conn) = (Fixtures::new(), app(&pool).await);
    let RecordRead::Found(record) = read(&mut conn, &fixtures.source(), "ROC100001").await else {
        panic!("found");
    };
    assert!(names_agree(&record, "EXAMPLE plumbing, co.", None));
    assert!(names_agree(&record, "Other Name", Some("sample-pipes")));
    assert!(!names_agree(&record, "Example Plumbing Co LLC", None));
    assert!(!names_agree(&record, "Fake Fixers", Some("")));
}

/// TD-LIC-30 (LIC-011, LIC-048): the city number a business's names match.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_city_match_number(pool: PgPool) {
    let mut conn = app(&pool).await;
    import_city_list(&mut conn, date(2026, 11, 12), CITY_LIST)
        .await
        .expect("city list");
    let yard = ("Other Name", Some("SAMPLE YARDS"));
    let number = |n: &str| Some(n.to_owned());
    let cases = [
        (yard, Some("CL-100001"), number("CL-100001")),
        (yard, Some("CL-999999"), None),
        (yard, Some("CL-100003"), None),
        (yard, None, number("CL-100001")),
        (("Example Window Wash", None), None, None),
        (
            ("Example Window Wash", None),
            Some("CL-100003"),
            number("CL-100003"),
        ),
        (("Fake Pool Folks", None), None, None),
        (("Nobody Listed", None), None, None),
    ];
    for (names, entered, want) in cases {
        let got = city_match_number(&mut conn, names, entered)
            .await
            .expect("match");
        assert_eq!(got, want, "{names:?} {entered:?}");
    }
}

/// A new business that chose `trades`, with its stored names when given.
async fn business(conn: &mut PgConnection, trades: &[&str], name: Option<&str>) -> Uuid {
    let id = create(conn, false, now()).await.expect("business");
    let trades: Vec<String> = trades.iter().map(|t| (*t).to_owned()).collect();
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, gen_random_uuid(), now(), $2)",
    )
    .bind(id)
    .bind(&trades)
    .execute(&mut *conn)
    .await
    .expect("sign-up row");
    if let Some(name) = name {
        save_business_name(conn, id, (name, None), None)
            .await
            .expect("names");
    }
    id
}

async fn catalog_and_lists(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut admin = pool.acquire().await.expect("connection");
    askhava_catalog::api::load_embedded(&mut admin)
        .await
        .expect("catalog");
    let mut conn = app(pool).await;
    import_roc_list(&mut conn, date(2026, 11, 12), ROC_LIST)
        .await
        .expect("roc list");
    import_city_list(&mut conn, date(2026, 11, 12), CITY_LIST)
        .await
        .expect("city list");
    conn
}

async fn signup(conn: &mut PgConnection, source: &LicenseLists, id: Uuid) -> SignupRecord {
    signup_record(conn, source, id)
        .await
        .expect("signup record")
}

/// TD-LIC-30 (BIZ-002, BIZ-016, D-2954): only the number `check_license` bound for the business
/// is read; the name flag, the contacts and the masked contact; `Unbound` before any check.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_signup_record_reads_the_bound_number(pool: PgPool) {
    let fixtures = Fixtures::new();
    let source = fixtures.source();
    let mut conn = catalog_and_lists(&pool).await;
    let ours = business(&mut conn, &["plumbing_repair"], Some("example plumbing co")).await;
    let other = business(&mut conn, &["plumbing_repair"], Some("Sample Drains LLC")).await;
    check_license(&mut conn, other, "ROC100002")
        .await
        .expect("other check");
    assert!(matches!(
        signup(&mut conn, &source, ours).await,
        SignupRecord::Unbound
    ));
    check_license(&mut conn, ours, "ROC100001")
        .await
        .expect("check");
    let SignupRecord::Read(found) = signup(&mut conn, &source, ours).await else {
        panic!("read");
    };
    assert_eq!(found.flag(), None);
    let contacts = found.contacts();
    assert_eq!(
        contacts
            .texting_phone
            .as_ref()
            .expect("phone")
            .expose_secret(),
        "928-555-0142"
    );
    assert_eq!(
        contacts.email.as_ref().expect("email").expose_secret(),
        "office@example.test"
    );
    assert!(contacts.has_address && found.seen().address.is_some());
    let masked = found.masked().expect("masked");
    assert_eq!(
        (masked.kind(), masked.shown()),
        (ContactKind::Mobile, "(•••) •••-0142")
    );
    let unnamed = business(&mut conn, &["plumbing_repair"], None).await;
    let renamed = business(&mut conn, &["plumbing_repair"], Some("Fake Fixers")).await;
    for id in [unnamed, renamed] {
        check_license(&mut conn, id, "ROC100001")
            .await
            .expect("check");
        let read = signup(&mut conn, &source, id).await;
        assert_eq!(read.flag(), Some(Flag::NameMismatch));
        assert!(matches!(read, SignupRecord::Read(_)));
    }
}

/// TD-LIC-30 (BIZ-002, D-059, LIC-048): a record with no usable contact is `NoContact`; a city-list
/// business is read under its matched city number with no name flag.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_no_contact_and_a_city_business(pool: PgPool) {
    let fixtures = Fixtures::new();
    let source = fixtures.source();
    let mut conn = catalog_and_lists(&pool).await;
    let bare = business(&mut conn, &["plumbing_repair"], Some("Placeholder Air Co")).await;
    check_license(&mut conn, bare, "ROC100004")
        .await
        .expect("check");
    let read = signup(&mut conn, &source, bare).await;
    assert_eq!(read.flag(), Some(Flag::NoRecordContact));
    assert!(matches!(read, SignupRecord::NoContact));
    let yard = business(
        &mut conn,
        &["house_cleaning"],
        Some("Sample Yard Care, LLC"),
    )
    .await;
    let SignupRecord::Read(found) = signup(&mut conn, &source, yard).await else {
        panic!("city record read");
    };
    assert_eq!(found.flag(), None);
    assert_eq!(found.masked().expect("masked").shown(), "(•••) •••-0150");
    let unlisted = business(&mut conn, &["house_cleaning"], Some("Nobody Listed")).await;
    assert!(matches!(
        signup(&mut conn, &source, unlisted).await,
        SignupRecord::NoContact
    ));
}

/// TD-LIC-30 (BIZ-016, LIC-011, LIC-048): a city-list business matched by its owner name, which
/// agrees with neither the record's business name nor its DBA, still gets no name flag.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_city_business_gets_no_name_flag(pool: PgPool) {
    let fixtures = Fixtures::new();
    let mut conn = catalog_and_lists(&pool).await;
    let owner = business(&mut conn, &["house_cleaning"], Some("Fake Owner Person")).await;
    let SignupRecord::Read(found) = signup(&mut conn, &fixtures.source(), owner).await else {
        panic!("city record read");
    };
    assert_eq!(found.flag(), None);
    assert_eq!(found.masked().expect("masked").shown(), "(•••) •••-0150");
}

/// TD-LIC-30 (BIZ-002, D-366, D-3249): a record whose only contact is a landline has no usable
/// contact, since a landline is never texted or called: `NoContact`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_landline_only_record_is_no_contact(pool: PgPool) {
    let fixtures = Fixtures::new();
    let mut conn = catalog_and_lists(&pool).await;
    let id = business(&mut conn, &["plumbing_repair"], Some("Fake Cool Co")).await;
    check_license(&mut conn, id, "ROC100005")
        .await
        .expect("check");
    let read = signup(&mut conn, &fixtures.source(), id).await;
    assert_eq!(read.flag(), Some(Flag::NoRecordContact));
    assert!(matches!(read, SignupRecord::NoContact));
}

/// TD-LIC-30 (D-366, BIZ-026): a record whose only phone is a landline shows the landline before
/// any step, and never offers it as a texting phone.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_landline_record_shows_the_landline(pool: PgPool) {
    let fixtures = Fixtures::new();
    let mut conn = catalog_and_lists(&pool).await;
    let id = business(&mut conn, &["plumbing_repair"], Some("Fake Heat Inc")).await;
    check_license(&mut conn, id, "ROC100003")
        .await
        .expect("check");
    let SignupRecord::Read(found) = signup(&mut conn, &fixtures.source(), id).await else {
        panic!("read");
    };
    assert!(found.contacts().texting_phone.is_none());
    assert_eq!(
        found.masked().expect("masked").kind(),
        ContactKind::Landline
    );
}

/// TD-LIC-30 (D-493, D-2954): the settings parser accepts `ASKHAVA_LICENSE_BASE_URL`; absent in
/// `ci`, no source is built and every read is `Unreadable`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_setting_accepted_and_absent_reads_unreadable(pool: PgPool) {
    let pairs = |extra: Option<&str>| {
        let mut p = vec![
            ("ASKHAVA_APP_ENV".to_owned(), "ci".to_owned()),
            (
                "ASKHAVA_DATABASE_URL".to_owned(),
                "postgres://fake@localhost/fake".to_owned(),
            ),
        ];
        if let Some(url) = extra {
            p.push(("ASKHAVA_LICENSE_BASE_URL".to_owned(), url.to_owned()));
        }
        p
    };
    let set = Settings::from_pairs(pairs(Some("file:///tmp/fake-lists"))).expect("accepted");
    assert_eq!(set.license_base_url(), Some("file:///tmp/fake-lists"));
    let unset = Settings::from_pairs(pairs(None)).expect("optional");
    let source = unset
        .license_base_url()
        .map(LicenseLists::new)
        .transpose()
        .expect("none");
    assert!(source.is_none());
    let mut conn = app(&pool).await;
    let got = read_own_record(&mut conn, &source, RecordList::Roc, "ROC100001").await;
    assert!(matches!(got.expect("read"), RecordRead::Unreadable));
    assert_eq!(unreadable_alerts(&pool).await, 1);
}

/// The row count of every `licensing` and `businesses` table.
async fn row_counts(pool: &PgPool) -> Vec<(String, i64)> {
    sqlx::query_as(
        "SELECT table_schema || '.' || table_name, \
           (xpath('/row/n/text()', query_to_xml(format('SELECT count(*) AS n FROM %I.%I', \
             table_schema, table_name), false, true, '')))[1]::text::bigint \
         FROM information_schema.tables \
         WHERE table_schema IN ('licensing', 'businesses') AND table_type = 'BASE TABLE' \
         ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .expect("row counts")
}

/// TD-LIC-30 (LIC-010, PRIV-016, D-1699): every read leaves every `licensing` and `businesses`
/// table's row count unchanged and no log line holds a record name, number or contact.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_lic_30_reads_write_nothing_and_log_no_value(pool: PgPool) {
    let fixtures = Fixtures::new();
    let source = fixtures.source();
    let mut conn = catalog_and_lists(&pool).await;
    let ours = business(&mut conn, &["plumbing_repair"], Some("Example Plumbing Co")).await;
    check_license(&mut conn, ours, "ROC100001")
        .await
        .expect("check");
    let yard = business(&mut conn, &["house_cleaning"], Some("Sample Yards")).await;
    let before = row_counts(&pool).await;
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    for id in [ours, yard] {
        assert!(matches!(
            signup(&mut conn, &source, id).await,
            SignupRecord::Read(_)
        ));
    }
    for number in ["ROC100001", "ROC200002", "ROC999999"] {
        read(&mut conn, &source, number).await;
    }
    assert_eq!(row_counts(&pool).await, before);
    lines.assert_clean();
}

/// Every log line's and span's fields, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn assert_clean(&self) {
        let logged = self.0.lock().expect("lines").clone();
        assert!(!logged.is_empty(), "the unreadable read is logged");
        for line in logged {
            for secret in SECRETS {
                assert!(!line.contains(secret), "a log line holds a record value");
            }
        }
    }
}

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Subscriber for Lines {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut line = String::new();
        attributes.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.0.lock().expect("lines").push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}
