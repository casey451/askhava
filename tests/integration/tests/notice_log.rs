//! The per-job notice log (TD-BIZ-21; BIZ-045, OPS-037). Fake IDs, addresses and times only.
//!
//! Each card, pick, reminder and release notice shows its send time or "Held by quiet hours until
//! 8:00 a.m.", the fake provider's delivered, deferred, bounced and complaint statuses per address
//! with the "may still be in Spam" line, the "Link first opened" time and the quote and Confirm
//! times; no call outcome, pixel or redirect; no homeowner detail; another business is refused.

use askhava_messaging::api::{
    NewNoticeLog, NoticeKind, Sending, mark_sent, notice_log, record_acted, record_first_open,
    record_notice,
};
use askhava_portals::api::{NoticeLogPage, notice_log_page};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Arizona wall-clock time on November 9, 2026 (UTC-7 all year).
fn az(hour: i8, minute: i8) -> Timestamp {
    date(2026, 11, 9)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

/// A sent fake email whose provider reported `event`; returns its outbox row.
async fn email(db: &mut PgConnection, n: u8, event: &str) -> Uuid {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at) \
         VALUES (repeat($1, 64), 'job_card', gen_random_uuid(), 'business', '{}', 'notice', \
         'sent', $2, 're_log_' || $1, $2, $2) RETURNING id",
    )
    .bind(n.to_string())
    .bind(az(9, 0).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("email");
    sqlx::query(
        "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at) \
         VALUES ('re_log_' || $1, $2, $3, $3)",
    )
    .bind(n.to_string())
    .bind(event)
    .bind(az(9, 1).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("event");
    id
}

/// The fake homeowner on `job`: name, phone and street in the vault, a homeowner row and her
/// conversation on the job, so the page's never-list below can fail.
async fn homeowner_on(db: &mut PgConnection, job: Uuid) {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x19), &key(0x29), &url).expect("vault");
    let new = NewContact {
        name: Some(SecretString::from("Zelda Fakename".to_owned())),
        phone: Some(SecretString::from("(928) 555-0198".to_owned())),
        email: None,
        address: Some(SecretString::from("12 Sentinel Street".to_owned())),
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault, &new).await.expect("contact");
    sqlx::query(
        "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from) \
         VALUES ($1, 'open', $2, $2, $2)",
    )
    .bind(job)
    .bind(az(8, 0).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("job");
    let homeowner: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (id, contact_id, created_at) \
         VALUES (gen_random_uuid(), $1, $2) RETURNING id",
    )
    .bind(contact)
    .bind(az(8, 0).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("homeowner");
    sqlx::query(
        "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at) \
         VALUES ($1, $2, 'en', gen_random_uuid(), 'consent_first', $3, $3)",
    )
    .bind(homeowner)
    .bind(job)
    .bind(az(8, 0).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("conversation");
}

/// TD-BIZ-21 (BIZ-045, OPS-037): the log's lines, kinds, statuses and times.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_21_the_job_page_notice_log(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (business, other, job) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let (main, backup) = (Uuid::new_v4(), Uuid::new_v4());
    homeowner_on(&mut db, job).await;
    for b in [business, other] {
        sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
            .bind(job).bind(b).bind(az(9, 0).to_sqlx()).execute(&mut *db).await.expect("card");
    }
    let log = |kind, mailbox, email| NewNoticeLog {
        business,
        job,
        kind,
        mailbox,
        email,
    };
    let events = [
        "email.delivered",
        "email.delivery_delayed",
        "email.bounced",
        "email.complained",
    ];
    let mut ids = Vec::new();
    for (n, (kind, event)) in [
        NoticeKind::Card,
        NoticeKind::Pick,
        NoticeKind::Reminder,
        NoticeKind::Release,
    ]
    .into_iter()
    .zip(events)
    .enumerate()
    {
        let n = u8::try_from(n).expect("small");
        let e = email(&mut db, n, event).await;
        let at = Sending::Sent(az(9, i8::try_from(n).expect("small")));
        ids.push(
            record_notice(&mut db, &log(kind, main, Some(e)), at)
                .await
                .expect("log"),
        );
    }
    // A later non-status event (an open) never hides the delivered row's status.
    sqlx::query(
        "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at) \
         VALUES ('re_log_0', 'email.opened', $1, $1)",
    )
    .bind(az(9, 40).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("opened");
    let held = record_notice(
        &mut db,
        &log(NoticeKind::Card, backup, None),
        Sending::Held(az(8, 0)),
    )
    .await
    .expect("held");
    assert!(
        record_first_open(&mut db, ids[0], az(9, 30))
            .await
            .expect("open")
    );
    assert!(
        !record_first_open(&mut db, ids[0], az(10, 0))
            .await
            .expect("again")
    );
    assert_eq!(
        record_acted(&mut db, (business, job), &[NoticeKind::Card], az(11, 15))
            .await
            .expect("q"),
        2
    );
    record_acted(&mut db, (business, job), &[NoticeKind::Pick], az(13, 5))
        .await
        .expect("c");
    assert_eq!(
        notice_log(&mut db, business, job)
            .await
            .expect("read")
            .len(),
        5
    );
    assert!(
        notice_log(&mut db, other, job)
            .await
            .expect("other")
            .is_empty()
    );

    let addresses = [(main, "jobs@fakeco.example".to_owned())];
    let page = match notice_log_page(&mut db, business, (job, 1042), &addresses)
        .await
        .expect("page")
    {
        NoticeLogPage::Shown(html) => html,
        NoticeLogPage::NotFound => panic!("own job refused"),
    };
    for line in [
        "Delivered means your mail server accepted it; it may still be in Spam.",
        "Job card",
        "Pick notice",
        "Confirm reminder",
        "Homeowner details ready",
        "Sent Nov 9, 9:00 a.m., to jobs@fakeco.example",
        "Held by quiet hours until Nov 9, 8:00 a.m., to an address you removed",
        "Delivered to your mail server",
        "Delayed by your mail server",
        "Bounced: your mail server refused it",
        "Marked as spam in your inbox",
        "Link first opened Nov 9, 9:30 a.m. (may be a mail scanner)",
        "You quoted Nov 9, 11:15 a.m.",
        "You confirmed Nov 9, 1:05 p.m.",
    ] {
        assert!(page.contains(line), "missing {line}");
    }
    for never in [
        "<img", "call", "Call", "Zelda", "555", "Fakename", "Sentinel", "0198", "redirect",
    ] {
        assert!(!page.contains(never), "page holds {never}");
    }
    assert!(mark_sent(&mut db, held, az(8, 0)).await.expect("sent"));
    assert!(!mark_sent(&mut db, held, az(8, 1)).await.expect("once"));

    let stranger = Uuid::new_v4();
    let refused = notice_log_page(&mut db, stranger, (job, 1042), &addresses)
        .await
        .expect("page");
    assert_eq!(refused, NoticeLogPage::NotFound);
    let empty = notice_log_page(&mut db, other, (job, 1042), &addresses)
        .await
        .expect("page");
    let NoticeLogPage::Shown(empty) = empty else {
        panic!("a card holder is refused");
    };
    assert!(empty.contains("We haven't sent you anything about job 1042 yet."));
    assert!(
        !empty.contains("jobs@fakeco.example"),
        "another business's rows"
    );
}

/// TD-BIZ-21 (BIZ-045): the log is kept as the message log is: `askhava_app` cannot delete it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_21_the_log_is_never_deleted(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let gone = sqlx::query("DELETE FROM messaging.notice_log")
        .execute(&mut *db)
        .await;
    assert!(gone.is_err(), "the app role deleted the notice log");
}
