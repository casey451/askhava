//! The under-18 stop through the route's entry, `intake::api::receive_raw_reply` (TD-INT-20:
//! INT-038, INT-002, PRIV-007; D-4051, D-4064, D-4142). Fake IDs, fake keys, fake names, fake
//! addresses and fake phones only.
//!
//! "I'm 11" closes the request and clears her vault contact's name, address and stated names in
//! the stop's own transaction, the phone and consent record kept; "I'm 15" closes it and keeps
//! them; a failing purge keeps nothing of the stop; an earlier request's paid pick does not spare
//! the contact and its job is left as it is; a 911 text goes before the INT-038 text, in an open
//! request and while the name-and-address question is open; a first text is stopped on the
//! request it opened.

use askhava_intake::api::rules::{AgeStop, Inbound, SAFETY_TEMPLATE, Settings, UNDER_18_TEMPLATE};
use askhava_intake::api::{
    Handled, IntakeError, JobStart, Opened, RawReply, Reply, Taken, TextIn, begin_job,
    receive_raw_reply,
};
use askhava_jobs::api::current;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const NAME_ADDRESS: &str = "Zelda Canary 12 Test Street";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x58), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: Uuid, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(contact.as_u128() ^ (0x572_0000 + minute.unsigned_abs() as u128)),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: words.contains("gas"),
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// A fake vault contact holding only a fake phone, as the inbound route finds or makes it.
async fn contact(db: &mut Db, phone: &str) -> Uuid {
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// The reply to `msg` through the route's entry.
async fn try_reply(db: &mut Db, msg: &TextIn<'_>) -> Result<Reply, IntakeError> {
    let raw = SecretString::from(msg.inbound.words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (msg, keys), (Settings::default(), &[])).await
}

async fn reply(db: &mut Db, msg: &TextIn<'_>) -> Reply {
    try_reply(db, msg).await.expect("reply")
}

fn handled(reply: Reply) -> Handled {
    match reply {
        Reply::Handled(handled) => handled,
        other => panic!("handled after its screen, got {other:?}"),
    }
}

fn templates(handled: &Handled) -> Vec<&'static str> {
    handled.texts.iter().map(|q| q.template).collect()
}

/// A request opened by a first text; its ID.
async fn request(db: &mut Db, who: Uuid, minute: i64) -> Uuid {
    let first = text(who, "my kitchen sink is leaking under the cabinet", minute);
    handled(reply(db, &first).await)
        .conversation
        .expect("request")
}

/// Marks the name-and-address question asked in request `id`.
async fn ask_name_address(db: &mut Db, id: Uuid, minute: i64) {
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, 'name_address_asked', 'asked', to_timestamp($2))",
    )
    .bind(id)
    .bind(at(minute).as_second() as f64)
    .execute(&mut **db)
    .await
    .expect("ask");
}

/// A request in which she gave her fake name and address, and one stated name on her contact.
async fn named(db: &mut Db, who: Uuid, minute: i64) -> Uuid {
    let id = request(db, who, minute).await;
    ask_name_address(db, id, minute + 1).await;
    let given = reply(db, &text(who, NAME_ADDRESS, minute + 2)).await;
    assert!(matches!(given, Reply::NameAddress(Taken::Stored(c), _) if c == who));
    sqlx::query(
        "INSERT INTO privacy.stated_name (contact_id, name, key_version, digest, index_version) \
         VALUES ($1, '\\x00ff'::bytea, 1, sha256('fake-stated-name'::bytea), 1)",
    )
    .bind(who)
    .execute(&mut **db)
    .await
    .expect("stated name");
    id
}

/// (name, address, phone kept, homeowner-phone index rows, stated names) of contact `who`.
async fn vault_row(db: &mut Db, who: Uuid) -> (bool, bool, bool, i64, i64) {
    sqlx::query_as(
        "SELECT name IS NOT NULL, address IS NOT NULL, phone IS NOT NULL,
           (SELECT count(*) FROM privacy.contact_index WHERE contact_id = $1
            AND purpose = 'homeowner-phone'),
           (SELECT count(*) FROM privacy.stated_name WHERE contact_id = $1)
         FROM privacy.contact WHERE id = $1",
    )
    .bind(who)
    .fetch_one(&mut **db)
    .await
    .expect("contact row")
}

async fn count(db: &mut Db, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut **db)
        .await
        .expect("count")
}

async fn purged_audits(db: &mut Db, who: Uuid) -> i64 {
    let sql =
        "SELECT count(*) FROM ops.audit_log WHERE action = 'contact_purged' AND subject_id = $1";
    count(db, sql, who).await
}

async fn consent_records(db: &mut Db, who: Uuid) -> i64 {
    let sql = "SELECT count(*) FROM intake.consent_record r JOIN intake.homeowner h \
               ON h.id = r.homeowner_id WHERE h.contact_id = $1";
    count(db, sql, who).await
}

async fn closed(db: &mut Db, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT closed_at IS NOT NULL FROM intake.conversation WHERE id = $1")
        .bind(id)
        .fetch_one(&mut **db)
        .await
        .expect("request")
}

/// TD-INT-20 (INT-038, PRIV-007, D-4142): "I'm 11" after she gave a fake name and address closes
/// the request with the INT-038 text, clears the name, address and stated names, keeps the phone,
/// its index and the consent record, and audits one `contact_purged`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_under_13_clears_the_vault_name_and_address(mut db: Db) {
    let who = contact(&mut db, "(928) 555-0172").await;
    let id = named(&mut db, who, 0).await;
    assert_eq!(vault_row(&mut db, who).await, (true, true, true, 1, 1));
    let Reply::UnderAge(stop, sent) = reply(&mut db, &text(who, "I'm 11", 10)).await else {
        panic!("I'm 11 stops");
    };
    assert_eq!((stop, sent.template), (AgeStop::Under13, UNDER_18_TEMPLATE));
    assert!(closed(&mut db, id).await);
    assert_eq!(vault_row(&mut db, who).await, (false, false, true, 1, 0));
    assert_eq!(consent_records(&mut db, who).await, 1);
    assert_eq!(purged_audits(&mut db, who).await, 1);
}

/// TD-INT-20 (INT-038): "I'm 15" closes the request and keeps her name and address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_under_18_keeps_the_vault_contact(mut db: Db) {
    let who = contact(&mut db, "(928) 555-0173").await;
    let id = named(&mut db, who, 0).await;
    let Reply::UnderAge(stop, _) = reply(&mut db, &text(who, "I'm 15", 10)).await else {
        panic!("I'm 15 stops");
    };
    assert_eq!(stop, AgeStop::Under18);
    assert!(closed(&mut db, id).await);
    assert_eq!(vault_row(&mut db, who).await, (true, true, true, 1, 1));
    assert_eq!(purged_audits(&mut db, who).await, 0);
}

/// TD-INT-20 (INT-038, D-4142): a purge that fails rolls the whole stop back: the request stays
/// open, no INT-038 text is queued and the name and address remain.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_a_failing_purge_keeps_nothing(mut db: Db) {
    let who = contact(&mut db, "(928) 555-0174").await;
    let id = named(&mut db, who, 0).await;
    for sql in [
        "CREATE FUNCTION public.t572_refuse() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'fake vault failure'; END $$",
        "CREATE TRIGGER t572_refuse BEFORE UPDATE ON privacy.contact FOR EACH ROW \
         EXECUTE FUNCTION public.t572_refuse()",
    ] {
        sqlx::query(sql).execute(&mut *db).await.expect("refuse");
    }
    assert!(try_reply(&mut db, &text(who, "I'm 11", 10)).await.is_err());
    assert!(!closed(&mut db, id).await);
    let under_18: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1 AND template_id = $2",
    )
    .bind(who)
    .bind(UNDER_18_TEMPLATE)
    .fetch_one(&mut *db)
    .await
    .expect("outbox");
    assert_eq!(under_18, 0);
    assert_eq!(vault_row(&mut db, who).await, (true, true, true, 1, 1));
    assert_eq!(purged_audits(&mut db, who).await, 0);
}

/// TD-INT-20 (INT-038, D-4051, D-4142): a sender whose earlier request's job is past a paid pick
/// says "I'm 11" in a new request: her contact's name and address are cleared too, and the earlier
/// job keeps its status and its paid pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_an_earlier_paid_pick_does_not_spare_the_contact(mut db: Db) {
    let who = contact(&mut db, "(928) 555-0175").await;
    let earlier = named(&mut db, who, 0).await;
    let start = JobStart {
        conversation: earlier,
        words: "my kitchen sink is leaking under the cabinet",
        emergency: false,
        trade: "plumbing",
        at: at(3),
    };
    let Opened::Created(job) = begin_job(&mut db, &start).await.expect("job") else {
        panic!("a new job");
    };
    for sql in [
        "UPDATE jobs.job SET status = 'connected' WHERE id = $1",
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, '00000000-0000-0000-0000-000000057201', 1, 'page', 'paid', now())",
    ] {
        sqlx::query(sql)
            .bind(job)
            .execute(&mut *db)
            .await
            .expect("seed");
    }
    sqlx::query("UPDATE intake.conversation SET closed_at = opened_at WHERE id = $1")
        .bind(earlier)
        .execute(&mut *db)
        .await
        .expect("close");
    assert!(closed(&mut db, earlier).await);
    let id = request(&mut db, who, 20).await;
    assert_ne!(id, earlier);
    let Reply::UnderAge(AgeStop::Under13, _) = reply(&mut db, &text(who, "I'm 11", 30)).await
    else {
        panic!("I'm 11 stops");
    };
    assert_eq!(vault_row(&mut db, who).await, (false, false, true, 1, 0));
    let status = current(&mut db, job).await.expect("job").status.code();
    assert_eq!(status, "connected");
    let sql = "SELECT count(*) FROM picks.pick WHERE job_id = $1 AND status = 'paid'";
    assert_eq!(count(&mut db, sql, job).await, 1);
}

/// TD-INT-20 (INT-038, INT-002, D-385, D-4064): "I'm 11 and I smell gas" queues the 911 text
/// before the INT-038 text and asks nothing more, in an open request and while the
/// name-and-address question is open; both requests close.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_911_text_goes_before_the_under_18_text(mut db: Db) {
    for (phone, asked) in [("(928) 555-0176", false), ("(928) 555-0177", true)] {
        let who = contact(&mut db, phone).await;
        let id = request(&mut db, who, 0).await;
        if asked {
            ask_name_address(&mut db, id, 1).await;
        }
        let got = handled(reply(&mut db, &text(who, "I'm 11 and I smell gas", 10)).await);
        let sent = templates(&got);
        let safety = sent.iter().position(|t| *t == SAFETY_TEMPLATE);
        assert!(safety.is_some(), "{phone}: {sent:?}");
        assert_eq!(sent.last(), Some(&UNDER_18_TEMPLATE), "{phone}: {sent:?}");
        assert!(closed(&mut db, id).await, "{phone}");
    }
}

/// TD-INT-20 (INT-038): a first text "I'm 11, my sink leaks" is stopped on the request it opened.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_20_a_first_text_is_stopped_on_its_own_request(mut db: Db) {
    let who = contact(&mut db, "(928) 555-0178").await;
    let got = handled(reply(&mut db, &text(who, "I'm 11, my sink leaks", 0)).await);
    let id = got.conversation.expect("the request it opened");
    assert_eq!(templates(&got).last(), Some(&UNDER_18_TEMPLATE));
    assert!(closed(&mut db, id).await);
}
