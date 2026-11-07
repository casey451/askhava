//! A business's notice consent, welcome, STOP, auto-reply and recruiting rules across
//! `businesses`, `ops`' audit log, `core`'s task queue, `messaging`'s templates and `portals`' done
//! screen (TD-CON-08, TD-BIZ-17, TD-TXT-13, TD-BIZ-35, and BIZ-032's auto-reply case of TD-BIZ-12;
//! CON-019, CON-020, BIZ-017, BIZ-032, BIZ-048; D-191, D-312, D-344, D-491, D-492).
//!
//! Proves on a fresh database that the E-SIGN box starts unticked and is stored with its wording
//! version and time, once per box, that no call consent can exist, that a phone may be texted
//! only after a current member ticked the text box for that phone, that the welcome is requested once and gates the first card, that a
//! business STOP leaves the account active and queues one owner email, which texts get the
//! automatic reply, and that a recruiting email opens with the advertisement line and its stop is
//! honored at once. All IDs and addresses are fake.

use askhava_businesses::api::{
    ADVERTISEMENT_LINE, AUTO_REPLY_TEMPLATE, Acting, CONSENT_VERSION, Checks, ConsentChannel,
    ConsentError, Event, NO_CALLS_LINE, PHONE_STOPPED_KIND, Roles, WELCOME_KIND, add_member,
    add_owner, apply, auto_reply_due, business, consent_box, create, may_get_cards, may_recruit,
    may_text, phone_stopped, record_consent, recruiting_email, remove_member, request_welcome,
    stop_recruiting, stopped_email_line, welcome_lines, welcome_sent,
};
use askhava_messaging::api::{Language, MailboxKind, count_segments, render, template_ids};
use askhava_ops::api::Actor;
use askhava_portals::api::{DoneFacts, done_lines};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use ConsentChannel::{Email, Text};

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(db)
        .await
        .expect("count")
}

/// A pending business with its owner.
async fn business_with_owner(db: &mut PgConnection) -> (Uuid, Uuid) {
    let b = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, b, Uuid::from_u128(0xd0), at(0)).await;
    (b, owner.expect("owner"))
}

async fn activate(db: &mut PgConnection, b: Uuid) {
    let done = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let to = apply(db, b, Event::Checked(done), Actor::System, at(1)).await;
    assert_eq!(to.expect("activate").code(), "active");
}

/// A linked phone of `b`, linked by `by` (the number lives in `messaging`, never here).
async fn phone(db: &mut PgConnection, b: Uuid, by: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO businesses.staff_phone (business_id, linked_by, linked_at) \
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(b)
    .bind(by)
    .fetch_one(db)
    .await
    .expect("phone")
}

const CONSENTS: &str = "SELECT count(*) FROM businesses.esign_consent WHERE business_id = $1";

/// TD-CON-08, TD-BIZ-17 (CON-019, D-312, D-344): the box starts unticked; its wording names email,
/// names texts only when chosen, never asks for calls and says no calls are placed.
#[test]
fn td_con_08_box_starts_unticked_and_names_each_channel() {
    let email = consent_box(&[Email]);
    assert!(!email.ticked);
    assert_eq!(email.version, CONSENT_VERSION);
    assert!(email.wording.contains("by email"));
    assert!(!email.wording.contains("texts"));
    let both = consent_box(&[Email, Text]);
    assert!(!both.ticked);
    assert!(both.wording.contains("by email") && both.wording.contains("job-card texts"));
    for w in [&email.wording, &both.wording, &consent_box(&[Text]).wording] {
        assert!(w.contains(NO_CALLS_LINE));
        assert!(!w.contains("agree to get calls") && !w.to_lowercase().contains("emergency"));
    }
}

/// TD-CON-08, TD-BIZ-17 (CON-019): an unticked box stores nothing; a ticked one stores each channel
/// with the wording version and time, with an audit row; no call row can exist.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_08_consent_is_stored_with_version_and_time(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, owner) = business_with_owner(&mut db).await;
    let no = record_consent(&mut db, (b, owner, None), &[Email], false, at(2)).await;
    assert_eq!(no, Err(ConsentError::NotTicked));
    let none = record_consent(&mut db, (b, owner, None), &[], true, at(2)).await;
    assert_eq!(none, Err(ConsentError::NotTicked));
    assert_eq!(count(&mut db, CONSENTS, b).await, 0);
    let stranger = record_consent(
        &mut db,
        (b, Uuid::from_u128(9), None),
        &[Email],
        true,
        at(2),
    )
    .await;
    assert_eq!(stranger, Err(ConsentError::UnknownMember));
    for _ in 0..2 {
        record_consent(&mut db, (b, owner, None), &[Email, Email], true, at(2))
            .await
            .expect("consent");
    }
    let row: (String, String, i64) = sqlx::query_as(
        "SELECT channel, wording_version, extract(epoch FROM consented_at)::bigint FROM businesses.esign_consent \
         WHERE business_id = $1",
    )
    .bind(b)
    .fetch_one(&mut *db)
    .await
    .expect("row");
    let when = at(2).as_second();
    assert_eq!(row, ("email".to_owned(), CONSENT_VERSION.to_owned(), when));
    let audit = "SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.consent_recorded' \
                 AND actor_id = $1";
    assert_eq!(count(&mut db, audit, b).await, 1);
    let kind = "SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.consent_recorded' \
                AND subject_kind = 'businesses.team_member' AND subject_id = $1";
    assert_eq!(count(&mut db, kind, owner).await, 1);
    let call = sqlx::query(
        "INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version, \
         consented_at) VALUES ($1, $2, 'call', 'con019-v1', now())",
    )
    .bind(b)
    .bind(owner)
    .execute(&mut *db)
    .await;
    assert!(call.is_err(), "no automated-call consent can be stored");
}

/// TD-CON-08, TD-BIZ-35 (CON-019, BIZ-048, D-191): no text goes to a phone without a text box
/// ticked for that phone; the owner's boxes never cover another phone, a text box needs a linked
/// phone of the same business, and a removed member's box no longer counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_08_each_member_phone_needs_its_own_text_box(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, owner) = business_with_owner(&mut db).await;
    let by = Acting {
        business: b,
        member: owner,
    };
    let roles = Roles::new(true, false).expect("roles");
    let member = add_member(&mut db, by, Uuid::from_u128(0xd1), roles, at(1)).await;
    let member = member.expect("member");
    let (main, staff) = (
        phone(&mut db, b, owner).await,
        phone(&mut db, b, owner).await,
    );
    record_consent(&mut db, (b, owner, None), &[Email], true, at(2))
        .await
        .expect("owner email");
    assert!(!may_text(&mut db, b, main).await.expect("main"));
    let none = record_consent(&mut db, (b, owner, None), &[Text], true, at(3)).await;
    assert_eq!(none, Err(ConsentError::UnknownPhone));
    record_consent(&mut db, (b, owner, Some(main)), &[Text], true, at(3))
        .await
        .expect("owner text");
    assert!(may_text(&mut db, b, main).await.expect("main"));
    assert!(!may_text(&mut db, b, staff).await.expect("staff"));
    record_consent(&mut db, (b, member, Some(staff)), &[Text], true, at(4))
        .await
        .expect("member text");
    assert!(may_text(&mut db, b, staff).await.expect("staff"));
    let (other, other_owner) = business_with_owner(&mut db).await;
    assert!(!may_text(&mut db, other, staff).await.expect("cross"));
    let theirs = record_consent(
        &mut db,
        (other, other_owner, Some(staff)),
        &[Text],
        true,
        at(4),
    );
    assert_eq!(theirs.await, Err(ConsentError::UnknownPhone));
    remove_member(&mut db, by, member, at(5))
        .await
        .expect("remove");
    assert!(!may_text(&mut db, b, staff).await.expect("removed"));
    assert!(may_text(&mut db, b, main).await.expect("owner keeps"));
}

/// TD-CON-08 (CON-020, D-312): the welcome is requested once, only for an active business, and no
/// card may go before it is sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_08_welcome_goes_once_before_the_first_card(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _) = business_with_owner(&mut db).await;
    assert!(!request_welcome(&mut db, b, at(1)).await.expect("pending"));
    activate(&mut db, b).await;
    assert!(request_welcome(&mut db, b, at(2)).await.expect("first"));
    assert!(!request_welcome(&mut db, b, at(3)).await.expect("again"));
    let tasks = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'business' = $1";
    let n: i64 = sqlx::query_scalar(tasks)
        .bind(b.to_string())
        .bind(WELCOME_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(n, 1);
    assert!(!may_get_cards(&mut db, b).await.expect("before"));
    assert!(welcome_sent(&mut db, b, at(4)).await.expect("sent"));
    assert!(!welcome_sent(&mut db, b, at(5)).await.expect("once"));
    assert!(may_get_cards(&mut db, b).await.expect("after"));
}

/// TD-CON-08 (CON-020, SEC-025, BIZ-046): the welcome email's fixed lines.
#[test]
fn td_con_08_welcome_email_says_what_con_020_requires() {
    let all = welcome_lines().join("\n");
    for needed in [
        "Ask Hava",
        "To change how you get notices",
        "text STOP from a phone to stop texts to that phone",
        "Ask Hava never calls you.",
        "\"Ask Hava\" <jobs@notify.askhava.com>",
        "in Gmail, tap Not spam",
        "\"Never send it to Spam\"",
        "In any other email app, add us to your contacts.",
        "\"Ask Hava homeowner calls\"",
        "Ask Hava emails come only from addresses ending in askhava.com, never ask for your card or a code, and every button goes to askhava.com.",
    ] {
        assert!(all.contains(needed), "missing: {needed}");
    }
    assert!(
        !all.contains("come to this email"),
        "BIZ-036: cards may come by text"
    );
}

/// TD-CON-08 (BIZ-017, D-311, D-344): a STOP from a business phone leaves the account active and
/// queues one owner email per opt-out text, with the BIZ-017 wording.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_08_business_stop_stops_texts_only(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _) = business_with_owner(&mut db).await;
    activate(&mut db, b).await;
    let inbox = Uuid::from_u128(0x5709);
    phone_stopped(&mut db, b, inbox, at(2)).await.expect("stop");
    let kind = "SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.phone_stopped' \
                AND subject_kind = 'messaging.inbox' AND subject_id = $1";
    assert_eq!(count(&mut db, kind, inbox).await, 1);
    phone_stopped(&mut db, b, inbox, at(2))
        .await
        .expect("repeat");
    let status = business(&mut db, b).await.expect("read").status;
    assert_eq!(status.code(), "active");
    let tasks = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'inbox' = $1";
    let n: i64 = sqlx::query_scalar(tasks)
        .bind(inbox.to_string())
        .bind(PHONE_STOPPED_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(n, 1);
    assert_eq!(
        stopped_email_line("0142"),
        "Ask Hava: Texts to the phone ending 0142 are off because it texted STOP. Text START from it to turn them back on."
    );
}

/// TD-TXT-13, TD-BIZ-12 (BIZ-032, D-228, D-491): "thanks!", a question and a Spanish charge
/// complaint get the automatic reply; command words, HELP, STOP and expected quotes never do.
#[test]
fn td_txt_13_only_unexpected_business_texts_get_the_auto_reply() {
    for t in [
        "thanks!",
        "why was I charged?",
        "cargo incorrecto",
        "what is this $25 for",
        "Yes, why was I charged?",
        "Hold on, what is this $75?",
        "confirm 1042 but why so much",
        "",
        "  ",
    ] {
        assert!(auto_reply_due(t, false), "{t}");
    }
    for t in [
        "CONFIRM",
        "YES 1042",
        "sí 1042.",
        "HOLD",
        "hold 1042!",
        "PAUSE",
        "resume",
        "BUSY 5",
        "AWAY 3",
        "HELP",
        "STOP",
        "cancel",
    ] {
        assert!(!auto_reply_due(t, false), "{t}");
    }
    assert!(!auto_reply_due("$350 parts and labor", true), "a quote");
}

/// TD-TXT-13 (BIZ-032, TXT-001, TXT-044): the reply exists in English and Spanish, with the portal
/// link and HELP, inside its recorded segments.
#[test]
fn td_txt_13_auto_reply_has_both_languages() {
    for (lang, help) in [(Language::En, "HELP"), (Language::Es, "AYUDA")] {
        let text = render(AUTO_REPLY_TEMPLATE, lang, true, &[]).expect("render");
        assert!(text.body.contains("https://askhava.com/b") && text.body.contains(help));
        assert_eq!(count_segments(&text.body).segments, 1);
    }
}

/// TD-BIZ-35 (BIZ-048, SEC-028, D-374, D-492): a recruiting email opens with the advertisement line,
/// has an honest subject, the postal mailbox and a stop link honored at once; no text template
/// recruits.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_35_recruiting_is_email_only_and_stops_at_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let postal = (
        "PO Box 0000, Lake Havasu City, AZ 86403",
        MailboxKind::PoBox,
    );
    let stop = "https://askhava.com/stop/fake-token";
    let mail = recruiting_email("Get job requests in Havasu", &["Hi."], postal, stop);
    let mail = mail.expect("email");
    assert_eq!(
        mail.lines.first().map(String::as_str),
        Some(ADVERTISEMENT_LINE)
    );
    assert!(mail.lines.iter().any(|l| l.contains(stop)));
    assert!(mail.lines.iter().any(|l| l.contains("PO Box 0000")));
    for bad in ["", "Re: your quote", "Ask Hava: New job card", "Fwd: hi"] {
        let r = recruiting_email(bad, &[], postal, stop);
        assert_eq!(r, Err(ConsentError::Subject), "{bad}");
    }
    let home = ("12 Example St", MailboxKind::PoBox);
    let r = recruiting_email("Join Ask Hava", &[], home, stop);
    assert_eq!(r, Err(ConsentError::Footer));
    let r = recruiting_email("Join Ask Hava", &[], postal, "https://example.com/stop");
    assert_eq!(r, Err(ConsentError::Footer));
    let key = "ab".repeat(32);
    assert!(may_recruit(&mut db, &key).await.expect("before"));
    stop_recruiting(&mut db, &key, at(1)).await.expect("stop");
    assert!(!may_recruit(&mut db, &key).await.expect("after"));
    assert_eq!(may_recruit(&mut db, "x").await, Err(ConsentError::BadKey));
    let ids = template_ids().expect("ids");
    assert!(
        !ids.iter()
            .any(|id| id.contains("recruit") || id.contains("invite"))
    );
}

/// TD-BIZ-17 (BIZ-031, CON-019, D-312): the done screen promises one email, never a text.
#[test]
fn td_biz_17_done_screen_says_email_once() {
    let facts = DoneFacts {
        trade: "Plumbing".to_owned(),
        count: 3,
        waitlist: false,
        open_jobs: Vec::new(),
    };
    let (lines, _) = done_lines(&facts);
    let line = lines.join(" ");
    assert!(line.contains("We'll tell you once, by email, when requests in your trade start."));
    assert!(!line.to_lowercase().contains("text"));
}
