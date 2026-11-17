//! T481 (OPS-016, OPS-017; D-3753, D-3844, D-3877, D-3889): the helpers T110's replay calls,
//! against PostgreSQL and the stand-in Stripe. A replayed text is marked sent once with the SID
//! found in the carrier's log and never sent; a `blocked` row, a SID already on another row, a
//! wrong recipient or sender are refused; `replay_match` matches the unsent rows by recipient,
//! sender, body digest and send time and answers row IDs only. A charge lost in a restore is
//! rebuilt once with one ledger row; an unknown pick, a mismatched PaymentIntent, a pick with no
//! attempt row answer unmatched and a refunded charge is booked nowhere. All data is fake.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use askhava_adapters::api::ListedCharge;
use askhava_messaging::api::{
    LaunchChannel, Line, Lines, ListedText, NewText, Relation, ReplayMatch, ReplayRefusal,
    ReplaySent, SendingNumber, enqueue, mark_replayed_sent, replay_match, replay_window,
};
use askhava_payments::api::{RECEIPT_KIND, Rebuilt, rebuild_charge};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const FROM: &str = "+19285550100";
const HER: &str = "+19285550142";
const HIM: &str = "+19285550177";
const BODY: &str = "Laftly: fake replay body";
const T0: i64 = 1_794_000_000;

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

fn lines() -> Lines {
    let line = |n: Option<&str>| {
        Line::at_launch(
            LaunchChannel::Sms,
            n.and_then(|n| SendingNumber::parse(n).ok()),
        )
    };
    Lines {
        homeowner: line(Some(FROM)),
        business: line(None),
    }
}

fn at(mins: i64) -> Timestamp {
    Timestamp::from_second(T0).expect("time") + SignedDuration::from_mins(mins)
}

fn sid(n: u128) -> String {
    format!("SM{n:032}")
}

fn listed(n: u128, (to, from): (&str, &str), body: Option<&str>, mins: i64) -> ListedText {
    let body = body.map(|b| SecretString::from(b.to_owned()));
    ListedText::new(
        (&sid(n), SecretString::from(to.to_owned()), from),
        body,
        at(mins),
    )
    .expect("sid")
}

async fn contact(pool: &PgPool, vault: &Vault, phone: &str) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Fake Homeowner".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let mut db = pool.acquire().await.expect("connection");
    store_contact(&mut db, vault, &new).await.expect("contact")
}

/// Queues a row to `who` queued at `T0`, then moves it to `status` (claimed at `claimed` minutes).
async fn row(pool: &PgPool, (who, n): (Uuid, u128), status: &str, claimed: Option<i64>) -> Uuid {
    let new = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(n)],
        recipient: who,
        relation: Relation::OpenRequest,
        body: BODY,
    };
    let mut db = pool.acquire().await.expect("connection");
    let (id, _) = enqueue(&mut db, &secret(), &new).await.expect("queued");
    let keep_body = matches!(status, "queued" | "sending");
    sqlx::query(
        "UPDATE messaging.outbox SET status = $2, created_at = $3::timestamptz, \
         sending_since = $4::timestamptz, body = CASE WHEN $5 THEN body END WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(at(0).to_string())
    .bind(claimed.map(|m| at(m).to_string()))
    .bind(keep_body)
    .execute(pool)
    .await
    .expect("status");
    id
}

async fn stored(pool: &PgPool, id: Uuid) -> (String, Option<String>, bool, i64) {
    sqlx::query_as(
        "SELECT status, provider_sid, body IS NULL, \
         (SELECT count(*) FROM ops.audit_log a WHERE a.subject_id = o.id AND a.to_status = 'sent') \
         FROM messaging.outbox o WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("row")
}

/// OPS-017, D-3753: each unsent status is marked sent once with the found SID, its body cleared,
/// one audit row; a second call changes nothing. Nothing here can send: no carrier is given.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_unsent_status_is_marked_sent_once_with_the_found_sid(pool: PgPool) {
    let (vault, lines) = (vault(), lines());
    let her = contact(&pool, &vault, HER).await;
    let mut db = pool.acquire().await.expect("connection");
    for (n, status) in [(1, "queued"), (2, "sending"), (3, "unknown"), (4, "failed")] {
        let id = row(&pool, (her, n), status, Some(1)).await;
        let found = listed(n, (HER, FROM), None, 2);
        let marked = mark_replayed_sent(&mut db, (&vault, &lines), id, &found, at(2)).await;
        assert_eq!(marked.expect("marked"), ReplaySent::Marked, "{status}");
        let again = mark_replayed_sent(&mut db, (&vault, &lines), id, &found, at(3)).await;
        assert_eq!(again.expect("again"), ReplaySent::AlreadySent, "{status}");
        assert_eq!(
            stored(&pool, id).await,
            ("sent".to_owned(), Some(sid(n)), true, 1)
        );
    }
}

/// OPS-017, D-3753: a `blocked` or `expired` row, a SID on another row, a wrong recipient, a
/// wrong sender and a missing row are refused, and nothing changes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn closed_rows_taken_sids_and_wrong_numbers_are_refused(pool: PgPool) {
    let (vault, lines) = (vault(), lines());
    let her = contact(&pool, &vault, HER).await;
    let mut db = pool.acquire().await.expect("connection");
    let mut mark = async |id, m: &ListedText| {
        mark_replayed_sent(&mut db, (&vault, &lines), id, m, at(2))
            .await
            .expect("answer")
    };
    let refused = ReplaySent::Refused;
    for (n, status) in [(1, "blocked"), (2, "expired")] {
        let id = row(&pool, (her, n), status, None).await;
        assert_eq!(
            mark(id, &listed(9, (HER, FROM), None, 2)).await,
            refused(ReplayRefusal::Closed)
        );
        assert_eq!(stored(&pool, id).await.0, status);
    }
    let first = row(&pool, (her, 3), "unknown", Some(1)).await;
    let second = row(&pool, (her, 4), "unknown", Some(1)).await;
    assert_eq!(
        mark(first, &listed(5, (HER, FROM), None, 2)).await,
        ReplaySent::Marked
    );
    assert_eq!(
        mark(second, &listed(5, (HER, FROM), None, 2)).await,
        refused(ReplayRefusal::SidTaken)
    );
    assert_eq!(
        mark(second, &listed(6, (HIM, FROM), None, 2)).await,
        refused(ReplayRefusal::WrongRecipient)
    );
    assert_eq!(
        mark(second, &listed(6, (HER, HIM), None, 2)).await,
        refused(ReplayRefusal::WrongSender)
    );
    let missing = Uuid::from_u128(0x481);
    assert_eq!(
        mark(missing, &listed(6, (HER, FROM), None, 2)).await,
        refused(ReplayRefusal::NotFound)
    );
    assert_eq!(
        stored(&pool, second).await,
        ("unknown".to_owned(), None, true, 0)
    );
    assert!(
        ListedText::new(
            ("SMshort", SecretString::from(HER.to_owned()), FROM),
            None,
            at(0)
        )
        .is_none()
    );
}

/// D-2764, D-3889: the window spans the unsent rows; each row is matched by recipient, sender,
/// body digest and time, a body-less row by time near its claim; a SID already stored or from
/// another number matches nothing; two candidates are ambiguous; only row IDs come back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn replay_match_answers_row_ids_by_recipient_sender_digest_and_time(pool: PgPool) {
    let (vault, lines) = (vault(), lines());
    let (her, him) = (
        contact(&pool, &vault, HER).await,
        contact(&pool, &vault, HIM).await,
    );
    let mut db = pool.acquire().await.expect("connection");
    assert_eq!(replay_window(&mut db, &lines).await.expect("window"), None);
    let queued = row(&pool, (her, 1), "queued", None).await;
    let unknown = row(&pool, (her, 2), "unknown", Some(60)).await;
    let failed = row(&pool, (him, 3), "failed", None).await;
    let quiet = row(&pool, (Uuid::from_u128(0x0dd), 4), "queued", None).await;
    let done = row(&pool, (her, 5), "unknown", Some(1)).await;
    let first = listed(9, (HER, FROM), None, 1);
    assert_eq!(
        mark_replayed_sent(&mut db, (&vault, &lines), done, &first, at(1))
            .await
            .expect("mark"),
        ReplaySent::Marked
    );
    let window = replay_window(&mut db, &lines)
        .await
        .expect("window")
        .expect("rows");
    assert_eq!(
        (window.numbers, window.earliest, window.latest),
        (vec![FROM.to_owned()], at(-5), at(60))
    );
    let log = [
        listed(1, (HER, FROM), Some(BODY), 30),
        listed(2, (HER, FROM), Some(""), 62),
        listed(3, (HER, HIM), Some(BODY), 31),
        listed(4, (HIM, FROM), None, 2),
        listed(5, (HIM, FROM), None, 4),
        listed(9, (HER, FROM), Some(BODY), 32),
    ];
    let mut found = replay_match(&mut db, (&vault, &lines), &log)
        .await
        .expect("match");
    found.sort_by_key(|(id, _)| *id);
    let mut want = vec![
        (queued, ReplayMatch::Matched(sid(1))),
        (unknown, ReplayMatch::Matched(sid(2))),
        (failed, ReplayMatch::Ambiguous),
        (quiet, ReplayMatch::Ambiguous),
    ];
    want.sort_by_key(|(id, _)| *id);
    assert_eq!(found, want);
    let other = row(&pool, (him, 6), "queued", None).await;
    let lone = replay_match(&mut db, (&vault, &lines), &log[..1])
        .await
        .expect("match");
    assert!(lone.contains(&(other, ReplayMatch::Unmatched)));
}

/// The stand-in server on a port it picked itself; killed on drop.
struct StandIn(Child, String);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 stand-in");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("listening line");
    let base = line
        .trim_end()
        .strip_prefix("stand-ins listening on ")
        .expect("listening")
        .to_owned();
    StandIn(child, base)
}

/// One call to the stand-in Stripe at `base` (`METHOD /v1/...`, form body), as plain HTTP/1.1
/// with a test key; its JSON. The adapter's Stripe client is `payments`' alone (FEE-032, D-595).
async fn stripe(base: &str, head: &str, body: &str) -> Value {
    let address = base.trim_start_matches("http://");
    let (verb, path) = head.split_once(' ').expect("method and path");
    let request = format!(
        "{verb} /stripe{path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer sk_test_fake\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.expect("answer");
    let (_, json) = answer.split_once("\r\n\r\n").expect("http answer");
    serde_json::from_str(json).expect("json")
}

/// Charges pick `pick` at the stand-in Stripe for customer `cus`; its PaymentIntent ID.
async fn charge_at_stripe(base: &str, cus: &str, pick: Uuid) -> String {
    let body = format!(
        "amount=7500&currency=usd&customer={cus}&payment_method=pm_card_visa&confirm=true\
         &metadata[pick_id]={pick}"
    );
    let intent = stripe(base, "POST /v1/payment_intents", &body).await;
    assert_eq!(intent["status"], "succeeded");
    intent["id"].as_str().expect("pi").to_owned()
}

/// Stripe's charge list, each charge as `list_charge_page` keeps it (IDs and amounts).
async fn listed_charges(base: &str) -> Vec<ListedCharge> {
    let page = stripe(base, "GET /v1/charges?created[gte]=0&limit=100", "").await;
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let charges = page["data"].as_array().expect("charges").iter();
    charges
        .map(|c| ListedCharge {
            id: text(&c["id"]).expect("ch"),
            payment_intent: text(&c["payment_intent"]),
            pick_id: text(&c["metadata"]["pick_id"]),
            amount: c["amount"].as_i64().expect("amount"),
            amount_refunded: c["amount_refunded"].as_i64().expect("refunded"),
            balance_transaction: None,
            dispute: None,
        })
        .collect()
}

/// Stores pick `pick`'s intent (`intent`) and, when `attempt`, its first try, as the restore kept them.
async fn hold(
    pool: &PgPool,
    (business, cus): (Uuid, &str),
    (pick, intent): (Uuid, &str),
    attempt: bool,
) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(business).bind(cus).execute(pool).await.expect("customer");
    sqlx::query("INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, now())")
        .bind(pick).bind(business).bind(Uuid::from_u128(pick.as_u128() + 0x100)).bind(intent).execute(pool).await.expect("intent");
    if attempt {
        sqlx::query("INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at) VALUES ($1, 1, $2, 'sent', now())")
            .bind(pick).bind(Uuid::from_u128(pick.as_u128() + 0x200)).execute(pool).await.expect("attempt");
    }
}

async fn counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM payments.ledger_entry WHERE kind = 'charge'), \
         (SELECT count(*) FROM payments.charge_evidence), (SELECT count(*) FROM core.task WHERE kind = $1)",
    )
    .bind(RECEIPT_KIND).fetch_one(pool).await.expect("counts")
}

/// OPS-016, D-3753, D-3844, TD-OPS-04: a lost charge is rebuilt once from Stripe's record with
/// one ledger row; an unknown pick, a mismatched PaymentIntent and a pick with no attempt row are
/// unmatched; a charge refunded in full or in part is booked nowhere; Stripe is never charged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_lost_charge_is_rebuilt_once_and_the_rest_are_answered(pool: PgPool) {
    let server = stand_in();
    let base = server.1.as_str();
    let business = Uuid::from_u128(0xb481);
    let made = stripe(
        base,
        "POST /v1/customers",
        &format!("metadata[business_id]={business}"),
    )
    .await;
    let cus = made["id"].as_str().expect("cus").to_owned();
    let pick = |n: u128| Uuid::from_u128(0x481_0000 + n);
    let mut intents = Vec::new();
    for n in 1..=5 {
        intents.push(charge_at_stripe(base, &cus, pick(n)).await);
    }
    hold(&pool, (business, &cus), (pick(1), &intents[0]), true).await;
    hold(
        &pool,
        (business, &cus),
        (pick(3), "pi_FakeOtherIntent481"),
        true,
    )
    .await;
    hold(&pool, (business, &cus), (pick(4), &intents[3]), false).await;
    hold(&pool, (business, &cus), (pick(5), &intents[4]), true).await;
    let refund = stripe(
        base,
        "POST /v1/refunds",
        &format!("payment_intent={}", intents[4]),
    )
    .await;
    assert_eq!(refund["status"], "succeeded");
    let page = listed_charges(base).await;
    let of = |n: u128| -> ListedCharge {
        let id = pick(n).to_string();
        page.iter()
            .find(|c| c.pick_id.as_deref() == Some(id.as_str()))
            .expect("listed")
            .clone()
    };
    let mut db = pool.acquire().await.expect("connection");
    let now = Timestamp::now();
    assert_eq!(
        rebuild_charge(&mut db, &of(1), now).await.expect("rebuild"),
        Rebuilt::Booked
    );
    assert_eq!(counts(&pool).await, (1, 1, 1));
    assert_eq!(
        rebuild_charge(&mut db, &of(1), now).await.expect("again"),
        Rebuilt::AlreadyBooked
    );
    for n in [2, 3, 4] {
        let charge = of(n);
        assert_eq!(
            rebuild_charge(&mut db, &charge, now).await.expect("n"),
            Rebuilt::Unmatched(charge.id.clone())
        );
    }
    let refunded = of(5);
    assert_eq!(
        rebuild_charge(&mut db, &refunded, now).await.expect("r"),
        Rebuilt::Refunded(refunded.id.clone())
    );
    let part = ListedCharge {
        amount_refunded: 100,
        ..of(4)
    };
    assert_eq!(
        rebuild_charge(&mut db, &part, now).await.expect("p"),
        Rebuilt::Refunded(part.id.clone())
    );
    let bare = ListedCharge {
        pick_id: None,
        ..of(4)
    };
    assert_eq!(
        rebuild_charge(&mut db, &bare, now).await.expect("b"),
        Rebuilt::Unmatched(bare.id.clone())
    );
    let odd = ListedCharge {
        pick_id: Some("not-a-pick".to_owned()),
        ..of(4)
    };
    assert_eq!(
        rebuild_charge(&mut db, &odd, now).await.expect("o"),
        Rebuilt::Unmatched(odd.id.clone())
    );
    let no_intent = ListedCharge {
        payment_intent: None,
        ..of(1)
    };
    let not_card = ListedCharge {
        id: "py_FakeNotACard481".to_owned(),
        ..of(1)
    };
    for charge in [no_intent, not_card] {
        let answer = rebuild_charge(&mut db, &charge, now).await.expect("u");
        assert_eq!(answer, Rebuilt::Unmatched(charge.id.clone()));
    }
    assert_eq!(counts(&pool).await, (1, 1, 1));
}
