//! TD-OPS-08, TD-OPS-09, TD-OPS-11 and TD-OPS-07's crowding flag (T524; OPS-027, OPS-030,
//! OPS-031, OPS-033, OPS-034, OPS-041, BIZ-054, CON-016, D-3880, D-5318, D-5657): the weekly
//! digest email and the owner digest view on PostgreSQL with fake data only.
//!
//! The email goes through `server::digest_email::run_digest` and the owner session proof comes
//! from the composition root's `session_minter`, so `app/server/src/lib.rs` is compiled in place
//! as `problem_complaint.rs` does (D-2853's exception, logged for this file in D-5705). No test
//! moves the test-control clock; every row is planted before the time the digest is read at
//! (D-4053). Every word, name, number and address is fictional.

use askhava_businesses::api::{
    AskedBy, NewComplaint, ReasonAnswer, list_closed, record_complaint, record_reason, reply,
};
use askhava_core::api::{OwnerFlagLimits, arizona, arizona_time};
use askhava_jobs::api::rules::CheckIn;
use askhava_jobs::api::{Answer, PROBLEM_ALERT, create, job_number, record_check_in_answer};
use askhava_messaging::api::{AUTO_ANSWERED_ACTION, MailboxKind, WROTE_AGAIN_ACTION};
use askhava_ops::api::{
    Actor, Asker, Audience, AuditEntry, Change, DeliveryCounts, DeliveryOutcome, HEALTH_ALERT,
    HelpLimits, NewAlert, Severity, VOLUME_LOW_ALERT, acknowledge_cleared, audit, digest_opened,
    latest_digest, open_help_request, raise_alert, record_delivery_counts,
};
use askhava_portals::api::{
    CrowdedTrade, DigestInputs, OWNER_DIGEST_ROUTE, OwnerDigestView, is_crowded, is_no_or_other,
    owner_digest_view, owner_flags,
};
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, NewContact, Vault, keep_problem_words, scrub, store_contact,
};
use askhava_recovery::api::{RESTORE_FAILED_ACTION, RESTORE_PASSED_ACTION};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate
/// for its `digest_email` and `session_minter` only.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the digest task and the session minter are called here"
)]
mod server;
use server::digest_email::{DIGEST_TEMPLATE, DigestMailer, DigestRun, REMINDER_TEMPLATE};
use server::*;

/// Casey's fake owner record ID.
const OWNER: Uuid = Uuid::from_u128(0x524);
/// A fictional PROBLEM reply.
const WORDS: &str = "the gate latch they fixed fell off again on the second day";

fn days(d: i64) -> SignedDuration {
    SignedDuration::from_hours(d * 24)
}

fn inputs() -> DigestInputs<'static> {
    DigestInputs {
        limits: OwnerFlagLimits::default(),
        restore_actions: [RESTORE_PASSED_ACTION, RESTORE_FAILED_ACTION],
    }
}

fn mailer() -> DigestMailer {
    DigestMailer {
        secret: SecretString::from("digest-test-key-not-a-secret".to_owned()),
        postal: (
            "PO Box 0000, Lake Havasu City, AZ 86403".to_owned(),
            MailboxKind::PoBox,
        ),
    }
}

/// What [`plant`] made: the job numbers the digest lists, the kept words and the vault contact.
struct Planted {
    number: i64,
    words: String,
    no_show: i64,
    complaints: [i64; 2],
    contact: Uuid,
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x53), &url).expect("vault")
}

/// A fictional homeowner's vault contact (a 555 number and an example.com address).
async fn contact(db: &mut PgConnection) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Zelda Canary".to_owned())),
        phone: Some(SecretString::from("(928) 555-0152".to_owned())),
        email: Some(SecretString::from("zelda.canary@example.com".to_owned())),
        ..NewContact::default()
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// A homeowner row for vault `contact`, created at `at`.
async fn homeowner(db: &mut PgConnection, contact: Uuid, at: Timestamp) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $2) RETURNING id",
    )
    .bind(contact)
    .bind(at.to_sqlx())
    .fetch_one(db)
    .await
    .expect("homeowner")
}

/// A request of `home` opened and closed at `at`; returns the conversation's ID.
async fn request(db: &mut PgConnection, home: Uuid, at: Timestamp) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at, closed_at) \
         VALUES ($1, 'en', gen_random_uuid(), 'consent_first', $2, $2, $2) RETURNING id",
    )
    .bind(home)
    .bind(at.to_sqlx())
    .fetch_one(db)
    .await
    .expect("request")
}

/// A request by vault `contact` closed as not covered for `trade` at `at`.
async fn not_covered(db: &mut PgConnection, contact: Uuid, trade: &str, at: Timestamp) {
    let home = homeowner(db, contact, at).await;
    let conversation = request(db, home, at).await;
    sqlx::query(
        "INSERT INTO intake.recruiting_demand (conversation_id, town, trade, created_at) \
         VALUES ($1, 'lake_havasu_city', $2, $3)",
    )
    .bind(conversation)
    .bind(trade)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("demand");
}

/// A day-3 PROBLEM on a new job at `at`: its kept words, its answer row and its alert; returns
/// the job's number and the kept words.
async fn problem(db: &mut PgConnection, at: Timestamp) -> (i64, String) {
    let job = create(db, at, false, Actor::System).await.expect("job");
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(WORDS, &known, &mut NameMemory::default());
    let inbox = Uuid::new_v4();
    let kept = keep_problem_words(db, inbox, job, &scrubbed, false).await;
    assert!(kept.expect("kept"));
    let how = (CheckIn::Day3, Answer::Problem);
    record_check_in_answer(db, job, how, Actor::System)
        .await
        .expect("answer");
    let alert = NewAlert {
        kind: PROBLEM_ALERT,
        severity: Severity::Normal,
        subject: Some(("messaging.inbox", inbox)),
    };
    raise_alert(db, &alert).await.expect("alert");
    let number = job_number(db, job).await.expect("number");
    (number, scrubbed.as_str().to_owned())
}

/// One alert of `kind` with no subject.
async fn alert(db: &mut PgConnection, kind: &'static str) {
    let new = NewAlert {
        kind,
        severity: Severity::Normal,
        subject: None,
    };
    raise_alert(db, &new).await.expect(kind);
}

/// One audit row by the system.
async fn audit_row(
    db: &mut PgConnection,
    action: &'static str,
    (subject_kind, subject_id): (&'static str, Uuid),
    change: Change,
) {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind,
        subject_id,
        change,
    };
    audit(db, &entry).await.expect(action);
}

/// TD-OPS-08: a health check failing at `now` and passing 10 minutes later, and a text-delivery
/// line falling at `now` and back at 97% 10 minutes later, each acknowledged by the system; and
/// one alert of each other kind the digest counts.
async fn plant_alerts(db: &mut PgConnection, now: Timestamp) {
    alert(db, HEALTH_ALERT).await;
    let later = now + SignedDuration::from_mins(10);
    let cleared = acknowledge_cleared(db, HEALTH_ALERT, later).await;
    assert_eq!(cleared.expect("cleared").len(), 1);
    let counts = |failed| DeliveryCounts::new(Audience::Homeowner, failed, failed).expect("n");
    let low = record_delivery_counts(db, counts(20), now).await;
    assert!(matches!(low.expect("low"), DeliveryOutcome::Raised(_)));
    let back = record_delivery_counts(db, counts(0), later).await;
    assert!(matches!(
        back.expect("back"),
        DeliveryOutcome::Acknowledged(_)
    ));
    for kind in [
        VOLUME_LOW_ALERT,
        askhava_licensing::api::LIST_FAILING_ALERT,
        askhava_payments::api::DISPUTE_ACTION,
        askhava_payments::api::UNKNOWN_CHARGE_ALERT,
        askhava_intake::api::CALL_BACK_ALERT,
    ] {
        alert(db, kind).await;
    }
}

/// `business`'s automatic activation and pause rows and a no-show pick ended at `at`; returns
/// the no-show job's number.
async fn plant_business(db: &mut PgConnection, business: Uuid, at: Timestamp) -> i64 {
    let subject = ("businesses.business", business);
    let active = Change::Status {
        from: "pending",
        to: "active",
    };
    audit_row(db, "businesses.activated", subject, active).await;
    let paused = Change::Status {
        from: "active",
        to: "paused",
    };
    audit_row(db, "businesses.paused", subject, paused).await;
    let job = create(db, at - days(2), false, Actor::System).await;
    let job = job.expect("job");
    sqlx::query(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at, \
         ended_at) VALUES ($1, $2, 1, 'page', 'no_show', $3, $4)",
    )
    .bind(job)
    .bind(business)
    .bind((at - days(2)).to_sqlx())
    .bind(at.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("no-show");
    job_number(db, job).await.expect("number")
}

/// Two complaints about `business` listed for the digest once their boxes closed, the first
/// with the business's reply; returns their job numbers.
async fn plant_complaints(db: &mut PgConnection, business: Uuid, now: Timestamp) -> [i64; 2] {
    let mut numbers = [0; 2];
    let replies = [Some("We came at nine and found the side gate shut."), None];
    for (n, words) in replies.into_iter().enumerate() {
        let job = create(db, now - days(8), false, Actor::System).await;
        let job = job.expect("job");
        let new = NewComplaint {
            business,
            job,
            kind: "no_show",
            words: "nobody came on the day we agreed",
        };
        let got = record_complaint(db, &new, now - days(6)).await;
        let got = got.expect("complaint");
        if let Some(words) = words {
            let done = reply(db, (business, got.id), words, now - days(5)).await;
            done.expect("reply");
        }
        assert!(
            list_closed(db, got.id, now - days(1))
                .await
                .expect("listed")
        );
        numbers[n] = job_number(db, job).await.expect("number");
    }
    numbers
}

/// One row of every kind the week's digest reads, each in the week before `now`.
async fn plant(db: &mut PgConnection, now: Timestamp) -> Planted {
    let at = now - days(1);
    let business = askhava_businesses::api::create(db, false, now - days(60)).await;
    let business = business.expect("business");
    plant_alerts(db, now).await;
    let no_show = plant_business(db, business, at).await;
    let complaints = plant_complaints(db, business, now).await;
    for (action, kind) in [
        (WROTE_AGAIN_ACTION, "messaging.optout_event"),
        (AUTO_ANSWERED_ACTION, "messaging.inbox"),
        (RESTORE_PASSED_ACTION, "recovery.restore"),
    ] {
        audit_row(db, action, (kind, Uuid::new_v4()), Change::Action).await;
    }
    let fee = ReasonAnswer::parse("fee", "").expect("answer");
    let left = record_reason(db, business, (AskedBy::Left, Some(&fee)), at).await;
    assert!(left.expect("reason"));
    let contact = contact(db).await;
    not_covered(db, contact, "pool_service", at).await;
    let help = (Uuid::new_v4(), at, now + days(1));
    open_help_request(db, Asker::Email, help, HelpLimits::default())
        .await
        .expect("help");
    let (number, words) = problem(db, at).await;
    Planted {
        number,
        words,
        no_show,
        complaints,
        contact,
    }
}

/// One digest run at `at`.
async fn run(db: &mut PgConnection, at: Timestamp) -> DigestRun {
    let done = server::digest_email::run_digest(db, &mailer(), at).await;
    done.expect("run")
}

/// The plain text of each queued owner digest email with `template`.
async fn emails(db: &mut PgConnection, template: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT plain FROM messaging.email_outbox WHERE template_id = $1")
        .bind(template)
        .fetch_all(db)
        .await
        .expect("emails")
}

/// The item's line text in `view`, as `<count> [refs]`.
fn item(view: &OwnerDigestView, label: &str) -> String {
    let line = view.digest.lines.iter().find(|l| l.item == label);
    let line = line.expect(label);
    format!("{:?} {}", line.count, line.refs.join(","))
}

/// Whether `w` is shaped like a hyphenated UUID.
fn uuid_shaped(w: &[u8]) -> bool {
    w.iter().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) {
            *b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }
    })
}

/// TD-OPS-08, TD-OPS-07 (OPS-027, D-464): 5 active businesses and a median of 4 quotes is
/// crowded, 4 businesses or a median of 3 is not; OPS-031: 31% NO or OTHER in two consecutive
/// months is flagged, 30% in either is not, and one month alone never is.
#[test]
fn td_ops_08_crowded_and_no_or_other_follow_the_settings() {
    let limits = OwnerFlagLimits::default();
    assert!(is_crowded(5, 40, &limits));
    assert!(!is_crowded(4, 40, &limits));
    assert!(!is_crowded(5, 39, &limits));
    assert!(!is_crowded(5, 30, &limits));
    assert!(is_no_or_other(&[(100, 31), (100, 31)], &limits));
    assert!(!is_no_or_other(&[(100, 30), (100, 31)], &limits));
    assert!(!is_no_or_other(&[(100, 31), (0, 0)], &limits));
    assert!(!is_no_or_other(&[(100, 31)], &limits));
}

/// The sent card of `job` in `trade` at `at` (`quotes.shown_card`).
async fn shown(db: &mut PgConnection, job: Uuid, trade: &str, at: Timestamp) {
    sqlx::query(
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
         photo_ids, category, candidates, emergency, recorded_at, sent_at) \
         VALUES (gen_random_uuid(), $1, '0123456789abcdef0123456789abcdef', 'Fake card', \
         'Fake Town', 'house', '{}', $2, '{}', false, $3, $3)",
    )
    .bind(job)
    .bind(trade)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("card");
}

/// A confirmed firm quote by `business` on `job` received at `at`.
async fn quote(db: &mut PgConnection, job: Uuid, business: Uuid, at: Timestamp) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 10000, false, '', 'Fake quote', false, true, $3)",
    )
    .bind(job)
    .bind(business)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("quote");
}

/// `job`'s move into Connected at `at`, with a fake paid pick by `business` there.
async fn connected(db: &mut PgConnection, job: Uuid, business: Uuid, at: Timestamp) {
    for sql in [
        "INSERT INTO jobs.status_history (job_id, version, from_status, to_status, at) \
         SELECT $1, coalesce(max(version), 0) + 1, 'payment_pending', 'connected', $3 \
         FROM jobs.status_history WHERE job_id = $1",
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'paid', $3)",
    ] {
        let q = sqlx::query(sql).bind(job).bind(business).bind(at.to_sqlx());
        q.execute(&mut *db).await.expect("connected");
    }
}

/// OPS-027, OPS-031: painting (5 active) and roofing (4 active) each get a job whose card went
/// out mid last Arizona month with 4 confirmed quotes (painting's fifth arrives this month) and a
/// job mid the month before with none; every job connected, NO or OTHER on all but roofing's
/// older job.
async fn plant_months(db: &mut PgConnection, now: Timestamp) {
    let first = arizona_time(now)
        .date()
        .first_of_month()
        .to_zoned(arizona());
    let first = first.expect("first").timestamp();
    let mut businesses = Vec::new();
    for _ in 0..5 {
        let b = askhava_businesses::api::create(db, false, now - days(120)).await;
        businesses.push(b.expect("business"));
    }
    sqlx::query(
        "INSERT INTO ops.coverage (scope, code, active, counted_at) \
         VALUES ('trade', 'painting', 5, $1), ('trade', 'roofing', 4, $1) \
         ON CONFLICT (scope, code) DO UPDATE SET active = EXCLUDED.active",
    )
    .bind(now.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("coverage");
    let answers = [Answer::No, Answer::Other, Answer::No, Answer::Done];
    let plan = [
        ("painting", 15),
        ("painting", 45),
        ("roofing", 15),
        ("roofing", 45),
    ];
    for ((trade, back), answer) in plan.into_iter().zip(answers) {
        let sent = first - days(back);
        let job = create(db, sent, false, Actor::System).await.expect("job");
        shown(db, job, trade, sent).await;
        if back == 15 {
            for b in &businesses[..4] {
                quote(db, job, *b, sent + SignedDuration::from_hours(1)).await;
            }
        }
        if trade == "painting" && back == 15 {
            quote(db, job, businesses[4], first).await;
        }
        connected(db, job, businesses[0], sent + SignedDuration::from_hours(3)).await;
        let how = (CheckIn::Day3, answer);
        let done = record_check_in_answer(db, job, how, Actor::System).await;
        done.expect("answer");
    }
}

/// TD-OPS-08, TD-OPS-07 (OPS-027, OPS-031, OPS-034, D-464, D-4311) on PostgreSQL: only the last
/// full Arizona month counts for crowding (painting: 5 active, median 4, 4 confirmed quotes per
/// win; roofing's 4 active is not crowded), NO or OTHER in both of the last two months flags
/// painting but not roofing (one month only), and a homeowner with 3 unpicked requests in 30
/// days is flagged by her count only while 2 are not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_flags_read_the_last_full_months_on_postgres(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    plant_months(&mut db, now).await;
    for n in [3, 2] {
        let home = homeowner(&mut db, Uuid::new_v4(), now - days(20)).await;
        for d in 0..n {
            request(&mut db, home, now - days(2 + d)).await;
        }
    }
    let flags = owner_flags(&mut db, now, &OwnerFlagLimits::default()).await;
    let flags = flags.expect("flags");
    let painting = CrowdedTrade {
        trade: "painting".to_owned(),
        active: 5,
        median_tenths: 40,
        quotes_per_win_tenths: Some(40),
    };
    assert_eq!(flags.crowded, vec![painting]);
    assert_eq!(flags.no_or_other, vec!["painting".to_owned()]);
    assert_eq!(flags.repeat_requests, vec![3]);
}

/// TD-OPS-08 (OPS-034, D-284): 3 lapsed picks in 30 days flag the business for Casey, 2 do not,
/// and the flag changes nothing (the business stays as it was).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_three_lapsed_picks_flag_two_do_not(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let mut ids = Vec::new();
    for lapsed in [3, 2] {
        let b = askhava_businesses::api::create(&mut db, false, now - days(60)).await;
        let b = b.expect("business");
        for n in 0..lapsed {
            let job = create(&mut db, now - days(20), false, Actor::System).await;
            sqlx::query(
                "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, \
                 picked_at) VALUES ($1, $2, 1, 'page', 'expired', $3)",
            )
            .bind(job.expect("job"))
            .bind(b)
            .bind((now - days(10 + n)).to_sqlx())
            .execute(&mut *db)
            .await
            .expect("pick");
        }
        ids.push(b);
    }
    let status = "SELECT status FROM businesses.business WHERE id = $1";
    let before: String = sqlx::query_scalar(status)
        .bind(ids[0])
        .fetch_one(&mut *db)
        .await
        .expect("s");
    let flags = owner_flags(&mut db, now, &OwnerFlagLimits::default()).await;
    let flags = flags.expect("flags");
    assert_eq!(flags.lapsed, vec![(ids[0], 3)]);
    let after: String = sqlx::query_scalar(status)
        .bind(ids[0])
        .fetch_one(&mut *db)
        .await
        .expect("s");
    assert_eq!(before, after, "a flag changes nothing");
}

/// TD-OPS-08, TD-OPS-09, TD-OPS-11 (OPS-030, OPS-033, OPS-041, BIZ-054, CON-016, D-5318,
/// D-5706): with no proof the owner view lists every planted item as counts and job numbers
/// (complaints with whether the business replied, the health and delivery alerts the system
/// acknowledged), the HELP request due in the next 4 days, and no PROBLEM words; with a genuine
/// owner proof it shows the fake PROBLEM words (D-5657).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_owner_view_shows_words_only_with_the_proof(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let planted = plant(&mut db, now).await;
    let at = now + SignedDuration::from_mins(15);
    let view = owner_digest_view(&mut db, None, at, &inputs())
        .await
        .expect("view");
    assert!(view.problem_words.is_empty(), "no words without the proof");
    assert_eq!(view.due_soon.len(), 1, "the HELP request due tomorrow");
    let [replied, silent] = planted.complaints;
    for (label, want) in [
        ("PROBLEM replies", format!("Some(1) #{}", planted.number)),
        ("No-shows", format!("Some(1) #{}", planted.no_show)),
        (
            "Complaints",
            format!("Some(2) #{replied} replied,#{silent} no reply"),
        ),
        ("Self-acknowledged alerts", "Some(2) ".to_owned()),
        (
            "Not covered, by trade",
            "Some(1) pool_service: 1".to_owned(),
        ),
        ("Leaving answers", "Some(1) left: the fee".to_owned()),
        ("Unreadable business texts", "None no reader yet".to_owned()),
    ] {
        assert_eq!(item(&view, label), want, "{label}");
    }
    for label in [
        "Automatic activations",
        "Automatic pauses (no review needed)",
        "License unknown",
        "Dispute filings",
        "Unknown charges refunded",
        "Database volume alerts",
        "Call-backs due",
        "HELP messages answered automatically",
        "Opted-out numbers that wrote again",
    ] {
        assert_eq!(item(&view, label), "Some(1) ", "{label}");
    }
    assert!(item(&view, "Help requests open (reply by)").starts_with("Some(1) "));
    assert!(item(&view, "Test restore").starts_with("Some(1) passed "));
    let proof = session_minter().owner(OWNER);
    let view = owner_digest_view(&mut db, Some(&proof), at, &inputs()).await;
    let view = view.expect("view");
    assert_eq!(view.problem_words.len(), 1);
    assert_eq!(view.problem_words[0].number, planted.number);
    assert_eq!(view.problem_words[0].words, planted.words);
}

/// What the weekly email must never hold: the PROBLEM words, the vault contact's ID and fields,
/// any UUID or any phone-shaped run of digits (PRIV-028, D-219).
fn assert_no_private(body: &str, planted: &Planted) {
    let id = planted.contact.to_string();
    for secret in [
        planted.words.as_str(),
        "latch",
        id.as_str(),
        "Zelda",
        "Canary",
        "555-0152",
        "5550152",
        "example.com",
    ] {
        assert!(!body.contains(secret), "{secret}");
    }
    assert!(!body.as_bytes().windows(36).any(uuid_shaped), "no ID");
    let digits = body
        .split(|c: char| !c.is_ascii_digit())
        .map(str::len)
        .max();
    assert!(digits.unwrap_or(0) < 7, "no phone-shaped number");
}

/// TD-OPS-08, TD-OPS-09 (OPS-030, OPS-033, D-219): the weekly email holds every item as counts
/// and job numbers with the one owner-portal link and no PROBLEM words, vault contact or ID; a
/// repeat run queues nothing; unopened 3 days, no reminder; 4 days, exactly one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_weekly_email_and_one_reminder_after_4_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let planted = plant(&mut db, now).await;
    let at = now + SignedDuration::from_mins(15);
    assert_eq!(run(&mut db, at).await, DigestRun::Sent);
    let sent = emails(&mut db, DIGEST_TEMPLATE).await;
    assert_eq!(sent.len(), 1);
    let body = &sent[0];
    let [replied, silent] = planted.complaints;
    for item in [
        "Automatic activations: 1".to_owned(),
        "Self-acknowledged alerts: 2".to_owned(),
        format!("No-shows: 1 (#{})", planted.no_show),
        "Automatic pauses (no review needed): 1".to_owned(),
        "License unknown: 1".to_owned(),
        "Opt-outs (of those texted): 2 (homeowners 0 of 0, businesses 0 of 0)".to_owned(),
        format!("Complaints: 2 (#{replied} replied, #{silent} no reply)"),
        format!("PROBLEM replies: 1 (#{})", planted.number),
        "Dispute filings: 1".to_owned(),
        "Unknown charges refunded: 1".to_owned(),
        "Unreadable business texts: not counted (no reader yet)".to_owned(),
        "Call-backs due: 1".to_owned(),
        "Help requests open (reply by): 1".to_owned(),
        "HELP messages answered automatically: 1".to_owned(),
        "Test restore: 1 (passed ".to_owned(),
        "Database volume alerts: 1".to_owned(),
        "Opted-out numbers that wrote again: 1".to_owned(),
        "Not covered, by trade: 1 (pool_service: 1)".to_owned(),
        "Leaving answers: 1 (left: the fee)".to_owned(),
        OWNER_DIGEST_ROUTE.to_owned(),
    ] {
        assert!(body.contains(&item), "{item}");
    }
    assert_no_private(body, &planted);
    assert_eq!(run(&mut db, at).await, DigestRun::Nothing);
    assert_eq!(run(&mut db, at + days(3)).await, DigestRun::Nothing);
    assert_eq!(run(&mut db, at + days(4)).await, DigestRun::Reminded);
    assert_eq!(run(&mut db, at + days(5)).await, DigestRun::Nothing);
    assert_eq!(
        emails(&mut db, REMINDER_TEMPLATE).await.len(),
        1,
        "one reminder"
    );
}

/// TD-OPS-09 (OPS-033): a digest Casey opened within 4 days is never re-sent; the next week's
/// digest goes as usual.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_an_opened_digest_gets_no_reminder(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let at = Timestamp::now();
    assert_eq!(run(&mut db, at).await, DigestRun::Sent);
    let row = latest_digest(&mut db).await.expect("read").expect("digest");
    let opened = digest_opened(&mut db, row.id, at + days(1)).await;
    assert!(opened.expect("opened"));
    for later in [4, 5, 6] {
        assert_eq!(run(&mut db, at + days(later)).await, DigestRun::Nothing);
    }
    assert!(emails(&mut db, REMINDER_TEMPLATE).await.is_empty());
    assert_eq!(run(&mut db, at + days(7)).await, DigestRun::Sent);
}
