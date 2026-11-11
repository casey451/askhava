//! TD-PICK-10 security cases (PICK-022, PICK-023, PICK-024, PRIV-005, PRIV-020, BIZ-014, D-454):
//! the release grant cannot open the phone, the release code never names it, the release texts and
//! emails hold only the job number and a link, and the release records are append-only for the app
//! role. The proof itself is unforgeable by T192's compile-fail cases (SEC-030).

use std::path::Path;

use askhava_picks::api::{
    EMAIL_CHOSEN_FIELDS, RELEASE_FIELDS, RELEASE_TEMPLATE, RELEASE_TEXT_TEMPLATE,
};
use askhava_privacy::api::{Field, Purpose, Reader, grant_for};
use sqlx::PgPool;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// PICK-023, PRIV-020: the picks release grant refuses the phone, alone or with the other fields.
#[test]
fn td_pick_10_the_release_grant_never_opens_the_phone() {
    let release = (Reader::Picks, Purpose::Release);
    assert!(grant_for(release.0, release.1, &RELEASE_FIELDS).is_some());
    assert!(grant_for(release.0, release.1, &EMAIL_CHOSEN_FIELDS).is_some());
    assert!(grant_for(release.0, release.1, &[Field::Phone]).is_none());
    let mut with_phone = EMAIL_CHOSEN_FIELDS.to_vec();
    with_phone.push(Field::Phone);
    assert!(grant_for(release.0, release.1, &with_phone).is_none());
}

/// PICK-023: the release code never names the phone field.
#[test]
fn td_pick_10_the_release_code_never_names_the_phone() {
    let src = std::fs::read_to_string(root().join("app/picks/src/release.rs")).expect("release.rs");
    assert!(!src.contains("Field::Phone"));
    assert!(!src.contains(".phone"));
}

/// PICK-024, TXT-036, D-306, D-454: in both languages the release email and text hold only the
/// job number and, for the email, the sign-in link; the English words are the spec's.
/// TD-WORD-35 (WORD-001, WORD-008, TXT-001, D-2843): each file is read with its brand filled.
#[test]
fn td_pick_10_the_release_email_and_text_carry_no_homeowner_detail() {
    for lang in ["en", "es"] {
        let path = root().join(format!("app/messaging/templates/{lang}.json"));
        let read = std::fs::read_to_string(path).expect("templates");
        let raw = askhava_core::api::brand()
            .fill(&read, lang == "es")
            .expect("brand tokens");
        let all: serde_json::Value = serde_json::from_str(&raw).expect("json");
        for (id, allowed) in [
            (RELEASE_TEMPLATE, &["job", "link"][..]),
            (RELEASE_TEXT_TEMPLATE, &["job"][..]),
        ] {
            let text = all[id]["text"].as_str().expect("template text");
            assert_eq!(all[id]["audience"], "business", "{lang} {id}");
            let slots: Vec<&str> = text
                .split('{')
                .skip(1)
                .filter_map(|s| s.split_once('}').map(|(slot, _)| slot))
                .collect();
            assert!(slots.iter().all(|s| allowed.contains(s)), "{lang} {id}");
            assert!(!text.contains('+') && !text.contains('@'), "{lang} {id}");
        }
        if lang == "en" {
            let words = |id: &str| all[id]["text"].as_str().unwrap_or_default().to_owned();
            let email = "Ask Hava: Job {job} is confirmed. Sign in to see the homeowner's details.";
            assert!(words(RELEASE_TEMPLATE).starts_with(email));
            let text = "Ask Hava: Job {job} is confirmed. The details are at askhava.com/business.";
            assert_eq!(words(RELEASE_TEXT_TEMPLATE), text);
        }
    }
}

/// PICK-022 "exactly once": the app role cannot change or remove a release or an address wait.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_10_release_records_are_append_only(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection").detach();
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut conn)
        .await
        .expect("SET ROLE askhava_app");
    for statement in [
        "UPDATE picks.release SET job_id = job_id",
        "DELETE FROM picks.release",
        "TRUNCATE picks.release",
        "UPDATE picks.address_wait SET job_id = job_id",
        "DELETE FROM picks.address_wait",
        "TRUNCATE picks.address_wait",
    ] {
        let refused = sqlx::query(statement).execute(&mut conn).await;
        let code = refused.err().and_then(|e| {
            e.as_database_error()
                .and_then(|d| d.code().map(|c| c.into_owned()))
        });
        assert_eq!(code.as_deref(), Some("42501"), "{statement}");
    }
}
