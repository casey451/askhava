//! An opt-out closes nothing (TD-CON-06: CON-012, CON-029, JOB-020, D-005, D-129).
//!
//! The property layer: every opt-out word, in any case and with any trailing punctuation, never
//! reads as a close or a cancel intent, and only CANCEL or CANCELAR alone is the CON-029 pause
//! word. The module layer: an opt-out from a number with a live job leaves the job's status and the
//! request as they were and records the event. That no text follows and the CON-028 event are
//! `tests/integration/tests/optout_job_notice.rs` (TD-CON-21).

use askhava_jobs::api::current;
use askhava_messaging::api::{APP_OPT_OUT_WORDS, PROVIDER_OPT_OUT_WORDS, SPANISH_OPT_OUT_WORDS};
use jiff::Timestamp;
use proptest::prelude::*;
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::rules::{CloseReading, Inbound, Settings, close_reading};
use crate::api::{JobStart, Opened, TextIn, begin_job, on_opt_out, receive_text, sole_cancel};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn words() -> Vec<&'static str> {
    let mut all: Vec<&str> = PROVIDER_OPT_OUT_WORDS.to_vec();
    all.extend(SPANISH_OPT_OUT_WORDS);
    all.extend(APP_OPT_OUT_WORDS);
    all
}

/// An opt-out word in a random case with optional trailing punctuation.
fn opt_out() -> impl Strategy<Value = (String, &'static str)> {
    let punct = prop::sample::select(vec!["", ".", "!", " ", "?!"]);
    (prop::sample::select(words()), any::<u64>(), punct).prop_map(|(word, bits, end)| {
        let cased: String = (0..)
            .zip(word.chars())
            .map(|(i, c)| {
                if bits >> (i % 64) & 1 == 1 {
                    c.to_ascii_uppercase()
                } else {
                    c
                }
            })
            .collect();
        (format!("{cased}{end}"), word)
    })
}

proptest! {
    /// TD-CON-06 (JOB-020, CON-012, D-005): no opt-out word is a close or a cancel intent, so bare
    /// "cancel" closes nothing; only CANCEL or CANCELAR alone pauses (CON-029).
    #[test]
    fn td_con_06_an_opt_out_word_never_closes((text, word) in opt_out()) {
        let reading = close_reading(&text, &[]);
        prop_assert!(
            !matches!(reading, CloseReading::Close(_) | CloseReading::Intent),
            "{}",
            text
        );
        prop_assert_eq!(sole_cancel(&text), word == "cancel" || word == "cancelar");
        let longer = format!("{text} my request");
        prop_assert!(!sole_cancel(&longer));
    }
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

/// A fake text from contact `contact` at `minute`; every text after the first is an opt-out.
fn text(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x50_0000 + contact * 100 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: minute > 1,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: Timestamp::from_second(1_793_000_000 + minute * 60).expect("time"),
    }
}

/// TD-CON-06 (CON-012, CON-029, JOB-020, D-005): every opt-out word from a number with a live job
/// leaves the job row as it was and the request open, and records the event;
/// only CANCEL and CANCELAR mark the job paused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_06_an_opt_out_changes_no_job_status(mut db: Db) {
    for (contact, word) in (0x6001..).zip(words()) {
        let first = text(contact, "my kitchen sink is leaking under the cabinet", 0);
        let key = secret();
        let handled = receive_text(&mut db, &key, &first, Settings::default()).await;
        let id = handled.expect("open").conversation.expect("request");
        let start = JobStart {
            conversation: id,
            words: "my kitchen sink is leaking under the cabinet",
            emergency: false,
            trade: "plumbing",
            at: first.received_at,
        };
        let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
            panic!("a new job");
        };
        let before = current(&mut db, job).await.expect("job");
        let done = on_opt_out(&mut db, &text(contact, word, 5))
            .await
            .expect("opt out");
        let done = done.expect("a live job");
        assert_eq!(
            (done.conversation, done.jobs.clone()),
            (id, vec![job]),
            "{word}"
        );
        assert_eq!(
            done.paused,
            word == "cancel" || word == "cancelar",
            "{word}"
        );
        assert!(done.task.is_some(), "{word}");
        let after = current(&mut db, job).await.expect("job");
        assert_eq!(before, after, "{word}");
        let open = crate::api::conversation(&mut db, id).await.expect("read");
        assert!(open.expect("row").closed_at.is_none(), "{word}");
        let paused = crate::api::paused_by_homeowner(&mut db, job).await;
        assert_eq!(paused, Ok(done.paused), "{word}");
    }
}
