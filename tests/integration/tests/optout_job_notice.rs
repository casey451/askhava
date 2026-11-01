//! STOP from a number with a live job (TD-CON-21: CON-028, CON-012; TD-CON-06's flow, JOB-020,
//! CON-029). Fake numbers, IDs and text only.
//!
//! `messaging` lists the number and queues the bare CON-009 confirmation; `intake::on_opt_out`
//! then changes no job status, texts nothing (the "Your request <n> is still open" line is never in
//! a text) and enqueues the CON-028 event once. The line by email is TD-TXT-05's (T094) and on the
//! quote page TD-PICK-38's (T052).

use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, OPTED_OUT_LIVE_KIND, Opened, TextIn, begin_job, on_opt_out, paused_by_homeowner,
    receive_text,
};
use askhava_jobs::api::current;
use askhava_messaging::api::{
    CONFIRMATION_TEMPLATE, InboundText, Outcome, ProviderKeyword, Relation, on_inbound_text,
};
use askhava_privacy::api::Vault;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const PHONE: &str = "+19285550147";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: Uuid, words: &str, minute: i64, opt_out: bool) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x2100 + minute.unsigned_abs() as u128),
        contact,
        inbound: Inbound {
            words,
            opt_out,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// The templates and bodies queued for `contact`.
async fn texts(db: &mut Db, contact: Uuid) -> Vec<(String, String)> {
    sqlx::query_as("SELECT template_id, body FROM messaging.outbox WHERE recipient_id = $1")
        .bind(contact)
        .fetch_all(&mut **db)
        .await
        .expect("outbox")
}

async fn events(db: &mut Db) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(OPTED_OUT_LIVE_KIND)
        .fetch_one(&mut **db)
        .await
        .expect("tasks")
}

/// The `intake.paused_by_homeowner` audit rows for request `id`.
async fn pause_audits(db: &mut Db, id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log \
         WHERE action = 'intake.paused_by_homeowner' AND subject_id = $1",
    )
    .bind(id)
    .fetch_one(&mut **db)
    .await
    .expect("audit")
}

/// TD-CON-21 (CON-028, CON-012, JOB-020, CON-029): STOP or bare CANCEL from a number with a live
/// job gets only the bare confirmation text; the job keeps its status, no "still open" line is
/// texted, and the CON-028 event is enqueued once per opt-out text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_21_stop_with_a_live_job_gets_the_bare_confirmation_only(mut db: Db) {
    for (n, word) in [(1_u128, "STOP"), (2, "cancel")] {
        let her = Uuid::from_u128(0x2_1000 + n);
        let first = text(
            her,
            "my kitchen sink is leaking under the cabinet",
            0,
            false,
        );
        let handled = receive_text(&mut db, &secret(), &first, Settings::default()).await;
        let id = handled.expect("open").conversation.expect("request");
        let start = JobStart {
            conversation: id,
            words: first.inbound.words,
            emergency: false,
            trade: "plumbing",
            at: at(1),
        };
        let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
            panic!("a new job");
        };
        let before = current(&mut db, job).await.expect("job").status;
        let queued = texts(&mut db, her).await.len();
        let stop = InboundText {
            inbox_id: Uuid::from_u128(0x2_2000 + n),
            contact: her,
            relation: Relation::OpenRequest,
            phone: &format!("{}{n}", &PHONE[..PHONE.len() - 1]),
            text: word,
            provider: ProviderKeyword::None,
            emergency: false,
        };
        let listed = on_inbound_text(&mut db, (&secret(), &vault()), &stop, at(10)).await;
        assert!(
            matches!(
                listed,
                Ok(Outcome::OptedOut {
                    confirmation: Some(_),
                    ..
                })
            ),
            "{word}"
        );
        let mut msg = text(her, word, 10, true);
        msg.inbox_id = stop.inbox_id;
        let done = on_opt_out(&mut db, &msg)
            .await
            .expect("intake")
            .expect("live");
        assert_eq!(done.jobs, vec![job], "{word}");
        assert!(done.task.is_some(), "{word}");
        assert_eq!(
            on_opt_out(&mut db, &msg)
                .await
                .expect("again")
                .and_then(|d| d.task),
            None
        );
        assert_eq!(
            current(&mut db, job).await.expect("job").status,
            before,
            "{word}"
        );
        let sent = texts(&mut db, her).await;
        assert_eq!(sent.len(), queued + 1, "{word}: one text after the opt-out");
        let confirmations = sent.iter().filter(|t| t.0 == CONFIRMATION_TEMPLATE).count();
        assert_eq!(confirmations, 1, "{word}");
        for (_, body) in &sent {
            assert!(!body.contains("still open"), "{word}: {body}");
        }
        assert_eq!(events(&mut db).await, i64::try_from(n).expect("n"));
        assert_eq!(
            paused_by_homeowner(&mut db, job).await,
            Ok(word == "cancel")
        );
        let audits = pause_audits(&mut db, id).await;
        assert_eq!(
            audits,
            i64::from(word == "cancel"),
            "{word}: one audit row, not one per run"
        );
    }
}
