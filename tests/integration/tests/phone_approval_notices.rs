//! TD-BIZ-53 (BIZ-033, BIZ-019, BIZ-037; D-3081, IC-95, T398): business notices reach only
//! approved phones, and every number a member other than the owner sets emails the owner.
//!
//! Real PostgreSQL through `businesses::api` and `quotes`' card notice step: a job card texts no
//! phone waiting for the owner's approval and emails its member instead, texts it once approved,
//! never once declined; a member's link and each later number change queue one
//! `businesses.phone_linked` owner email, keyed by the number's `number_set_at`, the owner's own
//! change none. Fake numbers only (555-01xx) and fake IDs.

use askhava_businesses::api::{
    Acting, Checks, Event, NoticeChannel, PHONE_LINKED_KIND, Roles, SenderLink, TeamError,
    add_member, add_owner, apply, approve_phone, change_phone_at, create, link_phone, reject_phone,
    request_welcome, set_notice_channel, waiting_phones, welcome_sent,
};
use askhava_messaging::api::{OutboxError, sender_add};
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{CardNotice, queue_card_notice};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

struct Directory(Vault);

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &self.0, business_line, number.expose_secret())
            .await
            .map_err(|e| match e {
                OutboxError::BadPhone => TeamError::BadPhone,
                OutboxError::SenderTaken => TeamError::PhoneTaken,
                _ => TeamError::Database,
            })
    }
}

fn directory() -> Directory {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Directory(Vault::new(&key(0x55), &key(0x65), &url).expect("vault"))
}

/// Noon in Arizona (19:00 UTC) on a fixed day, plus `minute` minutes.
fn at(minute: i64) -> Timestamp {
    let noon: Timestamp = "2026-11-10T19:00:00Z".parse().expect("time");
    noon + jiff::SignedDuration::from_mins(minute)
}

fn n(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// An active business with the welcome sent and text notices chosen: the owner (mailbox `seed`), a
/// "Confirm and billing" member (`seed + 1`) and two members with both roles (`seed + 2`, `+ 3`).
async fn team(db: &mut PgConnection, seed: u128) -> [Acting; 4] {
    let business = create(db, false, at(0)).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(seed), at(0))
        .await
        .expect("owner");
    let owner = Acting { business, member };
    let mut add = async |id: u128, roles: Roles| {
        let member = add_member(db, owner, Uuid::from_u128(id), roles, at(0))
            .await
            .expect("member");
        Acting { business, member }
    };
    let biller = add(seed + 1, Roles::new(false, true).expect("roles")).await;
    let both = add(seed + 2, Roles::BOTH).await;
    let other = add(seed + 3, Roles::BOTH).await;
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let checked = apply(db, business, Event::Checked(ok), Actor::System, at(0)).await;
    checked.expect("active");
    request_welcome(db, business, at(0)).await.expect("welcome");
    welcome_sent(db, business, at(0)).await.expect("sent");
    set_notice_channel(db, business, (NoticeChannel::Text, true), at(0))
        .await
        .expect("text");
    [owner, biller, both, other]
}

async fn link(db: &mut PgConnection, dir: &Directory, by: Acting, number: &str, m: i64) -> Uuid {
    link_phone(db, dir, (by, false), &n(number), at(m))
        .await
        .expect("link")
}

/// Queues `business`'s card for job `job` through `quotes`' card notice step and returns its
/// payload's emailed mailboxes (sorted) and texted phones (oldest link first).
async fn card(db: &mut PgConnection, business: Uuid, job: u128) -> (Value, Value) {
    let notice = CardNotice {
        number: 1042,
        category: "plumbing",
        town: "Lake Havasu City",
        card: "Kitchen sink drips under the cabinet. How soon: this week.",
    };
    let job = Uuid::from_u128(job);
    let queued = queue_card_notice(db, (job, business), &notice, (true, at(1))).await;
    assert!(queued.expect("card"));
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'job' = $1::text",
    )
    .bind(job)
    .fetch_one(db)
    .await
    .expect("task");
    let phones = payload["texts"].as_array().expect("texts").iter();
    let phones: Vec<Value> = phones.map(|t| t["phone"].clone()).collect();
    let mut to: Vec<String> = serde_json::from_value(payload["to"].clone()).expect("to");
    to.sort();
    (json!(to), json!(phones))
}

/// TD-BIZ-53 (BIZ-033, BIZ-019, BIZ-037): a waiting phone gets no job card text and its member,
/// with no other phone, gets the email; once approved it gets the text; a declined one never does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_53_card_texts_only_approved_phones(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    let [owner, _, both, other] = team(&mut db, 0x530).await;
    let number = n("928-555-0141");
    let main = link_phone(&mut db, &dir, (owner, true), &number, at(1));
    let main = main.await.expect("main");
    let waiting = link(&mut db, &dir, both, "928-555-0142", 2).await;
    let declined = link(&mut db, &dir, other, "928-555-0143", 3).await;
    let (to, texts) = card(&mut db, owner.business, 0x5301).await;
    let mailbox = |id: u128| json!(Uuid::from_u128(id));
    assert_eq!(to, json!([mailbox(0x532), mailbox(0x533)]));
    assert_eq!(texts, json!([main]), "no waiting phone is texted");
    for w in waiting_phones(&mut db, owner.business)
        .await
        .expect("waiting")
    {
        let shown = (w.line, w.number_set_at);
        if w.line == waiting {
            approve_phone(&mut db, owner, shown, at(4))
                .await
                .expect("ok");
        } else {
            reject_phone(&mut db, owner, shown, at(4))
                .await
                .expect("no");
        }
    }
    let (to, texts) = card(&mut db, owner.business, 0x5302).await;
    assert_eq!(to, json!([mailbox(0x533)]));
    assert_eq!(
        texts,
        json!([main, waiting]),
        "the approved phone is texted"
    );
    assert!(!texts.to_string().contains(&declined.to_string()));
}

/// The owner emails queued for phone `line`, oldest first, by their `number_set_at`.
async fn owner_emails(db: &mut PgConnection, line: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT payload->'number_set_at' FROM core.task WHERE kind = $1 \
         AND payload->>'line' = $2::text ORDER BY id",
    )
    .bind(PHONE_LINKED_KIND)
    .bind(line)
    .fetch_all(db)
    .await
    .expect("emails")
}

/// TD-BIZ-53 (BIZ-033): a member's link queues one owner email; the member's later number change
/// a second, carrying the new `number_set_at`; the same change repeated in one transaction no
/// third; the owner's own change none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_53_each_member_number_emails_the_owner(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    let [owner, biller, _, _] = team(&mut db, 0x540).await;
    let line = link(&mut db, &dir, biller, "928-555-0151", 2).await;
    assert_eq!(
        owner_emails(&mut db, line).await,
        vec![json!(at(2).to_string())]
    );
    let mut tx = db.begin().await.expect("transaction");
    for _ in 0..2 {
        change_phone_at(&mut tx, &dir, (biller, line), &n("928-555-0152"), at(5))
            .await
            .expect("change");
    }
    tx.commit().await.expect("commit");
    let emails = owner_emails(&mut db, line).await;
    assert_eq!(
        emails,
        vec![json!(at(2).to_string()), json!(at(5).to_string())]
    );
    change_phone_at(&mut db, &dir, (owner, line), &n("928-555-0153"), at(8))
        .await
        .expect("owner change");
    assert_eq!(owner_emails(&mut db, line).await.len(), 2, "owner: none");
}
