//! The homeowner job texts' pure parts (TD-JOB-03: JOB-010, JOB-011, JOB-012, TXT-042, TXT-040;
//! D-871, D-1301). Fake IDs, numbers and token only. The database flow (consent, opt-out, quiet
//! hours, the version check, one outbox row) is `tests/integration/tests/homeowner_job_texts.rs`.

use serde_json::json;
use sqlx::types::Uuid;

use crate::api::{
    ALL_QUOTES_IN_TEMPLATE, JOB_TEXT_TEMPLATES, JobText, job_text_body, parse_job_text,
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
