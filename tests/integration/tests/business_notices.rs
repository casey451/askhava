//! Business notices by the business's chosen channels across `businesses`, `quotes`, `messaging`
//! and `core`'s task queue (TD-TXT-17, TD-TXT-18, TD-BIZ-14, TD-TXT-22, TD-PRIV-12; BIZ-019,
//! BIZ-036, BIZ-037, CARD-019, CARD-020, QH-005, TXT-014, TXT-035 to TXT-037, PRIV-027, PICK-024;
//! D-303, D-306 to D-308, D-491 to D-493, D-1715, D-1760, D-1761).
//!
//! Proves on a fresh database that a card goes to the owner and "Cards and quotes" members only,
//! by email while business texting is off, by text only to a phone with text consent read at send
//! time (else by email), that a night notice is held whole to 8 a.m., that the release and
//! card-fix emails carry a per-person signed sign-in link and no homeowner detail, that no
//! business text template can carry one, that a card email holds no image and one job-page link,
//! and that the sent count leaves out a business whose card notice hard-bounced. Fake data only.

use askhava_businesses::api::{
    Acting, ChannelError, Checks, ConsentChannel, Event, NoticeChannel, NoticePurpose, Roles,
    add_member, add_owner, apply, create, may_text, notice_recipients, record_consent,
    request_welcome, set_notice_channel, welcome_sent,
};
use askhava_core::api::IdSecret;
use askhava_messaging::api::{
    LINK_LIFE, Line, Lines, MailboxKind, NoticeOutcome, NoticeSender, NumberState, SendChecks,
    SendingNumber, business_notice, held_until, sign_in_link,
};
use askhava_ops::api::Actor;
use askhava_quotes::api::{CardNotice, queue_card_notice, sent_count};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const POSTAL: &str = "PO Box 0000, Lake Havasu City, AZ 86403";
/// Noon in Arizona (19:00 UTC), inside the window.
const NOON: &str = "2026-11-10T19:00:00Z";
const SENTINELS: [&str; 5] = [
    "Zelda Fakename",
    "123 Sentinel St",
    "zelda@example.invalid",
    "Onsite Fakeperson",
    "https://askhava.com/p/sentinel-photo",
];

fn at(s: &str) -> Timestamp {
    s.parse().expect("time")
}

fn links() -> IdSecret {
    IdSecret::new(
        1,
        SecretString::from("test-only-link-secret-not-a-real-key".to_owned()),
    )
    .expect("secret")
}

fn sender() -> NoticeSender {
    let secret = SecretString::from("test-only-email-secret-not-real".to_owned());
    NoticeSender::new((secret, links()), POSTAL, MailboxKind::PoBox).expect("sender")
}

fn lines(business_texting_on: bool) -> Lines {
    let line = |on| Line {
        texting_on: on,
        number: Some(SendingNumber::parse("+15555550100").expect("fake number")),
        state: NumberState::Working,
    };
    Lines {
        homeowner: line(true),
        business: line(business_texting_on),
    }
}

/// Runs the business-notice handler for task `id` with the business line open or closed.
async fn send(
    db: &mut PgConnection,
    (payload, id): (&Value, i64),
    texting: bool,
    now: Timestamp,
) -> Result<NoticeOutcome, askhava_messaging::api::EmailError> {
    business_notice(db, (&sender(), &Gate, &lines(texting)), (id, payload), now).await
}

/// The pick whose notice still waits (its "Accept by" is [`ACCEPT_BY`]); any other moved on.
const WAITING_PICK: u128 = 0x501;
/// 4 p.m. Arizona time on the test day.
const ACCEPT_BY: &str = "2026-11-10T23:00:00Z";

/// The composition's checks: `businesses::api::may_text`, read when the notice is sent (D-1760),
/// and a stand-in for `picks::api::notice_sent` (D-1503).
struct Gate;

impl SendChecks for Gate {
    async fn may_text(&self, conn: &mut PgConnection, business: Uuid, phone: Uuid) -> bool {
        may_text(conn, business, phone).await.unwrap_or(false)
    }

    async fn accept_by(&self, _: &mut PgConnection, pick: Uuid, _: Timestamp) -> Option<Timestamp> {
        (pick == Uuid::from_u128(WAITING_PICK)).then(|| at(ACCEPT_BY))
    }
}

/// An active business with the welcome sent: owner (mailbox 0xa0), a "Cards and quotes" member
/// (0xa1) and a "Confirm and billing" member (0xa2). Returns the business and the member IDs.
async fn business(db: &mut PgConnection, seed: u128) -> (Uuid, [Uuid; 3]) {
    let t = at(NOON);
    let b = create(db, false, t).await.expect("business");
    let owner = add_owner(db, b, Uuid::from_u128(seed), t)
        .await
        .expect("owner");
    let by = Acting {
        business: b,
        member: owner,
    };
    let cards = Roles::new(true, false).expect("roles");
    let bill = Roles::new(false, true).expect("roles");
    let m1 = add_member(db, by, Uuid::from_u128(seed + 1), cards, t)
        .await
        .expect("m1");
    let m2 = add_member(db, by, Uuid::from_u128(seed + 2), bill, t)
        .await
        .expect("m2");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    request_welcome(db, b, t).await.expect("welcome");
    welcome_sent(db, b, t).await.expect("sent");
    (b, [owner, m1, m2])
}

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

fn card<'a>() -> CardNotice<'a> {
    CardNotice {
        number: 1042,
        category: "plumbing",
        town: "Lake Havasu City",
        card: "Kitchen sink drips under the cabinet. How soon: this week. Sent to 3 businesses.",
    }
}

/// Queues `b`'s card for `job` and returns the queued payload.
async fn queued_card(db: &mut PgConnection, job: Uuid, b: Uuid, texting: bool) -> Value {
    let queued = queue_card_notice(db, (job, b), &card(), (texting, at(NOON))).await;
    assert!(queued.expect("card"));
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'business' = $1::text",
    )
    .bind(b)
    .fetch_one(db)
    .await
    .expect("task")
}

async fn emailed(db: &mut PgConnection, b: Uuid) -> Vec<(Uuid, String, String, String)> {
    sqlx::query_as(
        "SELECT recipient_id, subject, html, plain FROM messaging.email_outbox \
         WHERE $1 = ANY(subject_ids) ORDER BY recipient_id",
    )
    .bind(b)
    .fetch_all(db)
    .await
    .expect("emails")
}

async fn texted(db: &mut PgConnection, phone: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1")
        .bind(phone)
        .fetch_one(db)
        .await
        .expect("texts")
}

/// TD-BIZ-14, TD-TXT-17 (BIZ-019, BIZ-037, CARD-020, TXT-035): by default a card goes by email to
/// the owner and the "Cards and quotes" member, never the "Confirm and billing" one, and no text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_14_card_email_to_owner_and_cards_members_only(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, [owner, _, _]) = business(&mut db, 0xa0).await;
    let p = phone(&mut db, b, owner).await;
    let payload = queued_card(&mut db, Uuid::from_u128(0x10b), b, false).await;
    assert_eq!(payload["texts"], json!([]));
    let run = send(&mut db, (&payload, 1), false, at(NOON));
    assert_eq!(
        run.await.expect("sent"),
        NoticeOutcome::Sent {
            emails: 2,
            texts: 0
        }
    );
    let to: Vec<Uuid> = emailed(&mut db, b).await.into_iter().map(|e| e.0).collect();
    assert_eq!(to, vec![Uuid::from_u128(0xa0), Uuid::from_u128(0xa1)]);
    assert_eq!(texted(&mut db, p).await, 0);
    let picks = notice_recipients(&mut db, b, NoticePurpose::PicksAndRelease, false).await;
    let picks: Vec<Uuid> = picks
        .expect("picks")
        .into_iter()
        .map(|r| r.mailbox)
        .collect();
    assert_eq!(picks, vec![Uuid::from_u128(0xa0), Uuid::from_u128(0xa2)]);
}

/// TD-TXT-17 (TXT-035, BIZ-036): text or both is refused while business texting is off, and a
/// stored choice gives no phone while it is off.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_17_text_refused_while_business_texting_is_off(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, [owner, _, _]) = business(&mut db, 0xb0).await;
    phone(&mut db, b, owner).await;
    for choice in [NoticeChannel::Text, NoticeChannel::Both] {
        let refused = set_notice_channel(&mut db, b, (choice, false), at(NOON)).await;
        assert_eq!(refused, Err(ChannelError::TextingOff));
    }
    set_notice_channel(&mut db, b, (NoticeChannel::Both, true), at(NOON))
        .await
        .expect("on");
    let off = notice_recipients(&mut db, b, NoticePurpose::Cards, false)
        .await
        .expect("off");
    assert!(off.iter().all(|r| r.phones.is_empty() && r.email));
}

/// TD-TXT-17, TD-BIZ-14 (TXT-035, CON-019, BIZ-019, D-1715, D-1760): with texting on and "both",
/// the consented phone gets the text; a phone with no ticked box, or any phone once the line is
/// closed, gets email instead.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_17_text_only_to_a_consented_phone_read_at_send(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, [owner, m1, _]) = business(&mut db, 0xc0).await;
    let (p0, p1) = (phone(&mut db, b, owner).await, phone(&mut db, b, m1).await);
    let text = [ConsentChannel::Text];
    record_consent(&mut db, (b, owner, Some(p0)), &text, true, at(NOON))
        .await
        .expect("box");
    set_notice_channel(&mut db, b, (NoticeChannel::Text, true), at(NOON))
        .await
        .expect("text");
    let payload = queued_card(&mut db, Uuid::from_u128(0x10c), b, true).await;
    assert_eq!(payload["to"], json!([]));
    let run = send(&mut db, (&payload, 2), true, at(NOON));
    assert_eq!(
        run.await.expect("sent"),
        NoticeOutcome::Sent {
            emails: 1,
            texts: 1
        }
    );
    assert_eq!(
        (texted(&mut db, p0).await, texted(&mut db, p1).await),
        (1, 0)
    );
    let to: Vec<Uuid> = emailed(&mut db, b).await.into_iter().map(|e| e.0).collect();
    assert_eq!(to, vec![Uuid::from_u128(0xc1)]);
    let closed = send(&mut db, (&payload, 3), false, at(NOON));
    assert_eq!(
        closed.await.expect("sent"),
        NoticeOutcome::Sent {
            emails: 2,
            texts: 0
        }
    );
}

/// TD-BIZ-14 (QH-005, CARD-020): a card notice due at 21:30 is held as a whole (no email, no text)
/// and queued again for 08:00 Arizona time, and its 24-hour window counts from 08:00 (D-1828).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_14_notice_at_2130_held_whole_to_0800(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _) = business(&mut db, 0xd0).await;
    let payload = queued_card(&mut db, Uuid::from_u128(0x10d), b, false).await;
    let night = at("2026-11-11T04:30:00Z");
    let run = send(&mut db, (&payload, 4), false, night);
    let eight = at("2026-11-11T15:00:00Z");
    assert_eq!(
        run.await.expect("held"),
        NoticeOutcome::Held { until: eight }
    );
    assert!(emailed(&mut db, b).await.is_empty());
    let due: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE dedupe_key = 'messaging.business_notice.held.4' \
         AND extract(epoch FROM run_at)::bigint = $1",
    )
    .bind(eight.as_second())
    .fetch_one(&mut *db)
    .await
    .expect("held task");
    assert_eq!(due, 1);
    let morning = send(&mut db, (&payload, 9), false, eight).await;
    assert!(matches!(morning, Ok(NoticeOutcome::Sent { emails: 2, .. })));
    let subject = emailed(&mut db, b).await.remove(0).1;
    assert!(
        subject.ends_with("Quote by 8 a.m. Arizona time."),
        "{subject}"
    );
    assert_eq!(held_until(at("2026-11-10T15:00:00Z")), None);
    assert_eq!(held_until(at("2026-11-11T02:59:59Z")), None);
    assert_eq!(held_until(at("2026-11-11T03:00:00Z")), Some(eight));
}

/// TD-TXT-18 (PICK-024, TXT-036, D-1761): the release email holds only its sentence and each
/// person's own signed sign-in link, never a sentinel; a card-fix email is signed the same way.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_18_release_email_has_a_signed_link_per_person(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _) = business(&mut db, 0xe0).await;
    let to = [Uuid::from_u128(0xe0), Uuid::from_u128(0xe2)];
    for (template, id) in [("release_notice", 5_i64), ("card_fix_notice", 6)] {
        let payload = json!({"template": template, "text_template": "release_text", "business": b,
            "pick": Uuid::from_u128(0x300 + u128::from(id.unsigned_abs())), "to": to,
            "values": {"job": 1042, "time": 1_794_340_800}});
        let run = send(&mut db, (&payload, id), false, at(NOON));
        run.await.expect("sent");
    }
    let expires = at(NOON) + LINK_LIFE;
    let rows = emailed(&mut db, b).await;
    assert_eq!(rows.len(), 4);
    for (person, subject, html, plain) in &rows {
        let link = sign_in_link(&links(), *person, 1042, expires).expect("link");
        assert!(plain.contains(&link), "{plain}");
        assert!(html.contains(&link.replace('&', "&#38;")), "{html}");
        for other in to.iter().filter(|p| *p != person) {
            assert!(!plain.contains(&other.to_string()));
        }
        for s in SENTINELS {
            assert!(!subject.contains(s) && !html.contains(s) && !plain.contains(s));
        }
    }
    let release = rows
        .iter()
        .find(|r| r.1.contains("confirmed"))
        .expect("release");
    let line = "Ask Hava: Job 1042 is confirmed. Sign in to see the homeowner's details.";
    assert!(release.3.starts_with(line));
}

/// TD-TXT-18 (TXT-036, D-306): every business text template, rendered with sentinel homeowner
/// values in any homeowner-shaped placeholder, holds none of them, and none asks for one.
#[test]
fn td_txt_18_no_business_text_template_carries_a_homeowner_detail() {
    let homeowner = [
        "name",
        "first_name",
        "address",
        "street",
        "email",
        "contact",
        "photo",
    ];
    for raw in [
        include_str!("../../../app/messaging/templates/en.json"),
        include_str!("../../../app/messaging/templates/es.json"),
    ] {
        let all: serde_json::Map<String, Value> = serde_json::from_str(raw).expect("json");
        for (id, t) in all.iter().filter(|(_, t)| t["audience"] == "business") {
            let text = t["text"].as_str().expect("text");
            for key in homeowner {
                assert!(!text.contains(&format!("{{{key}}}")), "{id} asks for {key}");
            }
            let filled = SENTINELS.iter().fold(text.to_owned(), |acc, s| {
                homeowner
                    .iter()
                    .fold(acc, |a, k| a.replace(&format!("{{{k}}}"), s))
            });
            assert!(SENTINELS.iter().all(|s| !filled.contains(s)), "{id}");
        }
    }
}

/// TD-PRIV-12, TD-BIZ-14 (PRIV-027, CARD-020): a card email has the CARD-020 subject, the card, the
/// WORD-002 statement and exactly one link to the signed-in job page; no image bytes or tags.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_12_card_email_has_no_image_and_one_job_page_link(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _) = business(&mut db, 0xf0).await;
    let payload = queued_card(&mut db, Uuid::from_u128(0x10f), b, false).await;
    let run = send(&mut db, (&payload, 7), false, at(NOON));
    run.await.expect("sent");
    let (_, subject, html, plain) = emailed(&mut db, b).await.remove(0);
    assert_eq!(
        subject,
        "Ask Hava: New plumbing job in Lake Havasu City. Quote by 12 p.m. Arizona time."
    );
    assert!(plain.contains("Kitchen sink drips") && plain.contains("not a contractor"));
    for bytes in [&html, &plain] {
        assert!(
            !bytes.contains("<img") && !bytes.contains("data:image") && !bytes.contains("base64")
        );
    }
    assert_eq!(
        html.matches("href=\"https://askhava.com/b/jobs/1042\"")
            .count(),
        1
    );
    assert_eq!(
        html.matches("href=").count(),
        2,
        "the job page and the footer's manage link"
    );
}

/// TD-TXT-22 (CARD-019, D-491): N counts card holders whose card notice the provider accepted;
/// one whose every card email hard-bounced drops out.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_22_sent_count_leaves_out_a_hard_bounced_business(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let job = Uuid::from_u128(0x110);
    let mut ids = Vec::new();
    for seed in [0x200_u128, 0x210, 0x220] {
        let (b, _) = business(&mut db, seed).await;
        let payload = queued_card(&mut db, job, b, false).await;
        let run = send(&mut db, (&payload, 8), false, at(NOON));
        run.await.expect("sent");
        sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, now())")
            .bind(job).bind(b).execute(&mut *db).await.expect("card");
        ids.push(b);
    }
    assert_eq!(
        sent_count(&mut db, job).await.expect("count"),
        0,
        "nothing accepted yet"
    );
    sqlx::query(
        "UPDATE messaging.email_outbox SET status = 'sent', sent_at = now(), \
         provider_id = 'fake-' || id, subject = NULL, html = NULL, plain = NULL",
    )
    .execute(&mut *db)
    .await
    .expect("accepted");
    assert_eq!(sent_count(&mut db, job).await.expect("count"), 3);
    sqlx::query(
        "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at) \
         SELECT provider_id, 'email.bounced', now(), now() FROM messaging.email_outbox \
         WHERE $1 = ANY(subject_ids)",
    )
    .bind(ids[0])
    .execute(&mut *db)
    .await
    .expect("bounce");
    assert_eq!(sent_count(&mut db, job).await.expect("count"), 2);
}

/// TD-BIZ-14 (BIZ-019, CON-019, D-1760): "both" sends the consented phone a text and its person
/// the email too; a text box no longer ticked when the notice runs (its row gone, as no revoke
/// path exists yet) sends the email instead.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_14_both_sends_email_and_text_and_an_unticked_box_gets_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, [owner, _, _]) = business(&mut db, 0x230).await;
    let p0 = phone(&mut db, b, owner).await;
    let text = [ConsentChannel::Text];
    record_consent(&mut db, (b, owner, Some(p0)), &text, true, at(NOON))
        .await
        .expect("box");
    set_notice_channel(&mut db, b, (NoticeChannel::Both, true), at(NOON))
        .await
        .expect("both");
    let payload = queued_card(&mut db, Uuid::from_u128(0x111), b, true).await;
    let run = send(&mut db, (&payload, 10), true, at(NOON)).await;
    assert_eq!(
        run.expect("sent"),
        NoticeOutcome::Sent {
            emails: 2,
            texts: 1
        }
    );
    assert_eq!(texted(&mut db, p0).await, 1);
    let to: Vec<Uuid> = emailed(&mut db, b).await.into_iter().map(|e| e.0).collect();
    assert_eq!(to, vec![Uuid::from_u128(0x230), Uuid::from_u128(0x231)]);
    set_notice_channel(&mut db, b, (NoticeChannel::Text, true), at(NOON))
        .await
        .expect("text");
    let job = Uuid::from_u128(0x112);
    let queue = queue_card_notice(&mut db, (job, b), &card(), (true, at(NOON))).await;
    assert!(queue.expect("queued"));
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'job' = $1::text",
    )
    .bind(job)
    .fetch_one(&mut *db)
    .await
    .expect("task");
    assert_eq!(payload["to"], json!([Uuid::from_u128(0x231)]));
    sqlx::query("DELETE FROM businesses.esign_consent WHERE line_id = $1")
        .bind(p0)
        .execute(&mut *db)
        .await
        .expect("unticked");
    let run = send(&mut db, (&payload, 11), true, at(NOON)).await;
    assert_eq!(
        run.expect("sent"),
        NoticeOutcome::Sent {
            emails: 2,
            texts: 0
        }
    );
    assert_eq!(texted(&mut db, p0).await, 1, "no second text");
}

/// TD-BIZ-14, TD-TXT-18 (PICK-019, FEE-020, FEE-038, D-1503, D-1761, D-1828): a pick notice's
/// "Accept by" is set at the send, and a pick that moved on gets nothing; a card-fix notice whose
/// window is stopped sends its untimed words; a `{link}` notice with no job number fails rather
/// than send the shared portal address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_14_times_are_set_at_send_and_a_link_is_never_unsigned(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _) = business(&mut db, 0x240).await;
    let to = [Uuid::from_u128(0x240)];
    let notice = |template: &str, pick: u128, values: Value| {
        json!({"template": template, "business": b, "pick": Uuid::from_u128(pick), "to": to,
               "values": values})
    };
    let moved = notice("pick_notice", 0x502, json!({"job": 1042, "window": 240}));
    let run = send(&mut db, (&moved, 12), false, at(NOON)).await;
    assert_eq!(run.expect("dropped"), NoticeOutcome::Dropped);
    assert!(emailed(&mut db, b).await.is_empty());
    let pick = notice(
        "pick_notice",
        WAITING_PICK,
        json!({"job": 1042, "window": 240}),
    );
    send(&mut db, (&pick, 13), false, at(NOON))
        .await
        .expect("pick");
    let fix = notice("card_fix_notice", 0x503, json!({"job": 1042, "time": null}));
    send(&mut db, (&fix, 14), false, at(NOON))
        .await
        .expect("fix");
    let plains: Vec<String> = emailed(&mut db, b).await.into_iter().map(|e| e.3).collect();
    let line =
        "Ask Hava: A homeowner picked your quote for job 1042. Accept by 4 p.m. Arizona time.";
    assert!(plains.iter().any(|p| p.starts_with(line)), "{plains:?}");
    let untimed = plains
        .iter()
        .find(|p| p.contains("didn't go through"))
        .expect("fix");
    assert!(
        untimed.contains("Try again") && !untimed.contains("After"),
        "{untimed}"
    );
    let no_job = notice("card_fix_notice", 0x504, json!({"time": 1_794_340_800}));
    let run = send(&mut db, (&no_job, 15), false, at(NOON)).await;
    assert!(matches!(
        run,
        Err(askhava_messaging::api::EmailError::Template)
    ));
}
