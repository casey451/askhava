//! The homeowner job texts' pure parts (TD-JOB-03: JOB-010, JOB-011, JOB-012, TXT-042, TXT-040;
//! D-871, D-1301). Fake IDs, numbers and token only. The database flow (consent, opt-out, quiet
//! hours, the version check, one outbox row) is `tests/integration/tests/homeowner_job_texts.rs`.

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError, IdSecret};
use askhava_jobs::api::TEXT_KIND;
use askhava_privacy::api::Vault;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::api::{
    ALL_QUOTES_IN_TEMPLATE, JOB_TEXT_TEMPLATES, JobText, JobTextKeys, job_text_body,
    parse_job_text, run_job_text,
};
use askhava_messaging::api::Language;

const JOB: &str = "00000000-0000-0000-0000-00000000c240";
const LINK: &str = "https://askhava.com/q/fake-token-for-tests";

fn event(template: &str, values: serde_json::Value) -> JobText {
    let payload = json!({"job": JOB, "version": 3, "template": template, "values": values});
    parse_job_text(&payload).expect("a job text")
}

fn body(template: &str, language: Language) -> String {
    let values = json!({"job": 1042, "quoted": 2, "count": 3});
    let link = (template == ALL_QUOTES_IN_TEMPLATE).then_some(LINK);
    job_text_body(&event(template, values), language, link).expect("rendered")
}

/// D-871: the payload parses once into the job, its version, a known template and the numbers;
/// an unknown template or a missing field is refused.
#[test]
fn td_job_03_job_text_payload_parses_once() {
    let text = event("closed_no_pick", json!({"job": 1042}));
    assert_eq!(text.job, Uuid::parse_str(JOB).expect("uuid"));
    assert_eq!((text.version, text.template), (3, "closed_no_pick"));
    for template in JOB_TEXT_TEMPLATES {
        assert_eq!(event(template, json!({})).template, template);
    }
    let unknown = json!({"job": JOB, "version": 3, "template": "consent", "values": {}});
    assert_eq!(parse_job_text(&unknown), None);
    assert_eq!(
        parse_job_text(&json!({"job": JOB, "template": "closed_no_pick"})),
        None
    );
}

/// TD-JOB-03, JOB-010, JOB-011, JOB-012: the timeout texts carry the request number, word for
/// word as the spec gives them.
#[test]
fn td_job_03_timeout_texts_name_the_request() {
    assert_eq!(
        body("no_quotes_yet", Language::En),
        "Ask Hava: No quotes yet for request 1042. A few more details or a photo often help a \
         business price it. Reply 1 to add details, 2 to keep waiting, or 3 to close."
    );
    assert_eq!(
        body("closed_no_quotes", Language::En),
        "Ask Hava: Request 1042 is closed because no quotes came in. Text us any time to try again."
    );
    assert_eq!(
        body("closed_no_pick", Language::En),
        "Ask Hava: Request 1042 is closed because no quote was picked. Text us any time to start \
         a new one."
    );
}

/// TD-JOB-03, TXT-042, TXT-040: "All quotes are in for request <n>: <k> of <N> businesses
/// quoted." with her page link; Spanish for a Spanish request; no link, no text.
#[test]
fn td_job_03_all_quotes_in_has_counts_and_her_link() {
    assert_eq!(
        body(ALL_QUOTES_IN_TEMPLATE, Language::En),
        format!(
            "Ask Hava: All quotes are in for request 1042: 2 of 3 businesses quoted. {LINK} Reply \
             LIST to see them here, or CLOSE 1042 if you don't need it."
        )
    );
    let es = body(ALL_QUOTES_IN_TEMPLATE, Language::Es);
    assert!(
        es.contains("cotizaron 2 de 3 negocios") && es.contains(LINK),
        "{es}"
    );
    let text = event(
        ALL_QUOTES_IN_TEMPLATE,
        json!({"job": 1042, "quoted": 2, "count": 3}),
    );
    assert!(job_text_body(&text, Language::En, None).is_err());
}

/// Fake sender keys built at run time: two distinct all-0x00 and all-0x04 vault keys.
fn fake_keys() -> Arc<JobTextKeys> {
    let secret = || SecretString::from("test-only-id-secret-not-a-real-key-000".to_owned());
    let seal = SecretString::from(format!("1:{}=", "A".repeat(43)));
    let index = SecretString::from(format!("1:{}BAQ=", "BAQE".repeat(10)));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&seal, &index, &url).expect("vault");
    let link = IdSecret::new(1, secret()).expect("id");
    Arc::new(JobTextKeys::new(link, secret(), vault))
}

/// D-871, D-1030 (A): the registered handler refuses a payload that does not parse with its
/// fixed sentence, before it touches the database (the pool here never connects).
#[tokio::test]
async fn td_job_03_run_job_text_refuses_a_bad_payload() {
    let pool = PgPool::connect_lazy("postgres://fake@db.invalid/fake").expect("lazy pool");
    let bad = [
        json!({}),
        json!({"job": JOB, "version": 3, "template": "consent", "values": {}}),
        json!({"job": "not-a-uuid", "version": 3, "template": "closed_no_pick"}),
    ];
    for payload in bad {
        let task = Claimed {
            id: 1,
            kind: TEXT_KIND.to_owned(),
            payload,
            attempts: 1,
        };
        let refused = run_job_text(pool.clone(), fake_keys(), task).await;
        assert_eq!(
            refused,
            Err(HandlerError("bad jobs homeowner-text payload"))
        );
    }
}
