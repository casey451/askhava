//! Masked-line closing, the staff-phone gate, the LINK request, email-preferred homeowners and the
//! Ask Hava number's greeting on a fresh database (TD-CALL-05: CALL-009, CALL-010, CALL-011,
//! CALL-012, CALL-018; TD-CALL-07: CALL-012, CALL-020; TD-CALL-08's approval gate: CALL-012;
//! TD-BIZ-34's removal case: CALL-012; D-078, D-191, D-227, D-260, D-288, D-311, D-1694 to
//! D-1697). Who may link a phone and the YES and Approve paths are TD-BIZ-16's (T148); the
//! approval state is seeded here. Fake numbers (555-01xx and the 500-555 test range) and IDs only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{
    Acting, Roles, SenderLink, TeamError, add_member, add_owner, create, link_phone, remove_member,
};
use askhava_calls::api::{
    Answer, Attestation, CallTurn, CloseReason, Connected, IncomingCall, LINK_TEXT_TO, Party,
    PhoneStanding, RoutedLine, Step, answer_call, assign_number, close_expired, close_line,
    code_call_twiml, expired, gate, greeting_en, greeting_es, greeting_twiml, job_pin, last4,
    phone_works, request_link, route, safety_script,
};
use askhava_core::api::{Settings, brand};
use askhava_messaging::api::{OutboxError, UpdateChannel, sender_add, set_update_channel};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const HER_PHONE: &str = "+19285550171";
const STAFF_A: &str = "+19285550172";
const STAFF_B: &str = "+19285550173";
const BLOCKED: &str = "+19285550177";
/// 10:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn ts(at: &str) -> Timestamp {
    at.parse().expect("instant")
}

/// A pool of one fake number, so every line is on it.
fn pool() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x59), &key(0x69), &url).expect("vault")
}

/// The composition's sender-directory link (D-963), with the test vault.
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
                _ => TeamError::Database,
            })
    }
}

/// A business with its owner (mailbox 0xa0), a "Confirm and billing" member (0xa1) and a
/// "Cards and quotes" member (0xa2); returns the business, the owner's and the billing member's
/// acting IDs.
async fn business(db: &mut PgConnection) -> (Uuid, Acting, Acting) {
    let at = ts(DAY);
    let b = create(db, false, at).await.expect("business");
    let owner = add_owner(db, b, Uuid::from_u128(0xa0), at)
        .await
        .expect("owner");
    let by = Acting {
        business: b,
        member: owner,
    };
    let billing = Roles::new(false, true).expect("roles");
    let m = add_member(db, by, Uuid::from_u128(0xa1), billing, at)
        .await
        .expect("member");
    let cards = Roles::new(true, false).expect("roles");
    add_member(db, by, Uuid::from_u128(0xa2), cards, at)
        .await
        .expect("member");
    (
        b,
        by,
        Acting {
            business: b,
            member: m,
        },
    )
}

/// Her fake contact.
async fn her(db: &mut PgConnection) -> Uuid {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER_PHONE),
        email: s("testa@example.com"),
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// Opens a line for a new job of `homeowner` with `business`; returns the job and the line.
async fn line(db: &mut PgConnection, (homeowner, business): (Uuid, Uuid)) -> (Uuid, Uuid) {
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner,
        business,
    };
    let l = assign_number(db, &pool(), job, ts(DAY))
        .await
        .expect("assign");
    (job.job, l.id())
}

async fn count(db: &mut PgConnection, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(db)
        .await
        .expect("count")
}

/// Every audit action and task kind written so far, in order: a close may add only its own.
async fn written(db: &mut PgConnection) -> (Vec<String>, Vec<String>) {
    let actions = sqlx::query_scalar("SELECT action FROM ops.audit_log ORDER BY id")
        .fetch_all(&mut *db)
        .await
        .expect("audit");
    let kinds = sqlx::query_scalar("SELECT kind FROM core.task ORDER BY id")
        .fetch_all(&mut *db)
        .await
        .expect("tasks");
    (actions, kinds)
}

async fn notices(db: &mut PgConnection, line: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'line' = $1",
    )
    .bind(line.to_string())
    .fetch_all(db)
    .await
    .expect("notices")
}

/// One turn of a call from `caller` to the pool number, with no PIN entered yet.
async fn call(db: &mut PgConnection, caller: &str) -> Step {
    let caller = SecretString::from(caller.to_owned());
    let incoming = IncomingCall {
        pool_number: "+15005550100",
        caller: &caller,
    };
    let turn = CallTurn {
        attestation: Attestation::A,
        tries_call: 0,
        tries_today: 0,
        entered: None,
    };
    match answer_call(db, &vault(), &incoming, turn)
        .await
        .expect("answer")
    {
        Answer::Step(step) => step,
        Answer::ReadBack(..) => panic!("no read-back without a wrong PIN"),
    }
}

const LINE_AUDIT: &str = "SELECT count(*) FROM ops.audit_log WHERE subject_id::text = $1 \
     AND action LIKE 'calls.line_closed_%'";
const KEYED: &str = "SELECT count(*) FROM core.task WHERE dedupe_key = $1";
const PHONES: &str = "SELECT count(*) FROM businesses.staff_phone WHERE business_id::text = $1";
const OPEN: &str = "SELECT count(*) FROM calls.number_pool WHERE id::text = $1 \
     AND closed_at IS NULL";

/// TD-CALL-05 (CALL-009, CALL-010): a line closes on a cancel, a no-show and a complaint, each
/// with one audit row; only the cancel tells the business, to the owner and the billing member
/// with the job number, and no close records a strike; a second close does nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_05_a_line_closes_on_cancel_no_show_or_complaint(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _, _) = business(&mut db).await;
    let h = her(&mut db).await;
    for reason in [
        CloseReason::Cancelled,
        CloseReason::NoShow,
        CloseReason::Complaint,
    ] {
        let (job, id) = line(&mut db, (h, b)).await;
        let (mut actions, mut kinds) = written(&mut db).await;
        let closed = close_line(&mut db, (job, 1042), reason, ts(DAY))
            .await
            .expect("close");
        assert_eq!(closed, Some(id));
        actions.push(reason.action().to_owned());
        kinds.push("ops.offdb_stream".to_owned());
        if reason == CloseReason::Cancelled {
            kinds.push("messaging.business_notice".to_owned());
        }
        assert_eq!(
            written(&mut db).await,
            (actions, kinds),
            "a close writes only its audit row (and its off-database copy) and a cancel's notice: no strike, no other task"
        );
        assert_eq!(count(&mut db, OPEN, &id.to_string()).await, 0);
        assert_eq!(count(&mut db, LINE_AUDIT, &id.to_string()).await, 1);
        assert_eq!(
            close_line(&mut db, (job, 1042), reason, ts(DAY)).await,
            Ok(None)
        );
        let sent = notices(&mut db, id).await;
        if reason == CloseReason::Cancelled {
            assert_eq!(sent.len(), 1, "a cancel tells the business once");
            assert_eq!(sent[0]["template"], "line_closed_cancelled");
            assert_eq!(sent[0]["values"]["job"], 1042);
            let to = [Uuid::from_u128(0xa0), Uuid::from_u128(0xa1)].map(|u| u.to_string());
            assert_eq!(sent[0]["to"], serde_json::json!(to));
        } else {
            assert!(sent.is_empty(), "{reason:?} queues no cancel notice");
        }
    }
}

/// TD-CALL-05 (CALL-009): a line closes 30 days after it opened, not a second before; the sweep
/// is safe to run twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_05_a_line_closes_after_30_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _, _) = business(&mut db).await;
    let h = her(&mut db).await;
    let (_, id) = line(&mut db, (h, b)).await;
    let thirty = ts(DAY) + SignedDuration::from_hours(30 * 24);
    let before = thirty - SignedDuration::from_secs(1);
    assert!(!expired(ts(DAY), before) && expired(ts(DAY), thirty));
    assert_eq!(close_expired(&mut db, before).await, Ok(0));
    assert_eq!(count(&mut db, OPEN, &id.to_string()).await, 1);
    assert_eq!(close_expired(&mut db, thirty).await, Ok(1));
    assert_eq!(close_expired(&mut db, thirty).await, Ok(0));
    assert_eq!(count(&mut db, OPEN, &id.to_string()).await, 0);
    assert_eq!(count(&mut db, LINE_AUDIT, &id.to_string()).await, 1);
}

/// TD-CALL-05 (CALL-011, D-078): a blocked call from an unlinked staff phone hears the PIN prompt;
/// the LINK notice is emailed to the owner and the billing member, never the cards-only member,
/// and texted only to the main phone, with the last 4 digits and the line's PIN, never the full
/// number; once per line, with no digits in its dedupe key.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_05_a_blocked_staff_phone_hears_the_pin_prompt_and_link_names_its_last_4(
    pool: PgPool,
) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _, _) = business(&mut db).await;
    let h = her(&mut db).await;
    let (_, id) = line(&mut db, (h, b)).await;
    assert_eq!(call(&mut db, BLOCKED).await, Step::AskPin);
    let blocked = SecretString::from(BLOCKED.to_owned());
    assert_eq!(request_link(&mut db, id, &blocked, ts(DAY)).await, Ok(true));
    assert_eq!(
        request_link(&mut db, id, &blocked, ts(DAY)).await,
        Ok(false)
    );
    let other = SecretString::from(STAFF_A.to_owned());
    assert_eq!(request_link(&mut db, id, &other, ts(DAY)).await, Ok(false));
    let sent = notices(&mut db, id).await;
    assert_eq!(sent.len(), 1, "one LINK notice per line");
    assert_eq!(sent[0]["text_to"], LINK_TEXT_TO);
    let key = format!("messaging.business_notice.link_this_phone.{id}");
    assert_eq!(
        count(&mut db, KEYED, &key).await,
        1,
        "the key holds IDs only"
    );
    assert_eq!(sent[0]["template"], "link_this_phone");
    assert_eq!(sent[0]["values"]["number"], "0177");
    assert_eq!(sent[0]["values"]["job"], job_pin(id).digits());
    let to = [Uuid::from_u128(0xa0), Uuid::from_u128(0xa1)].map(|u| u.to_string());
    assert_eq!(sent[0]["to"], serde_json::json!(to));
    assert!(
        !sent[0].to_string().contains("5550177"),
        "never the full number"
    );
    assert_eq!(last4(&SecretString::from("+123".to_owned())), None);
    let short = SecretString::from("123".to_owned());
    assert_eq!(request_link(&mut db, id, &short, ts(DAY)).await, Ok(false));
    assert_eq!(
        close_line(&mut db, (Uuid::nil(), 1), CloseReason::Complaint, ts(DAY)).await,
        Ok(None)
    );
    assert_eq!(
        close_expired(&mut db, ts(DAY) + SignedDuration::from_hours(720)).await,
        Ok(1)
    );
    assert_eq!(
        request_link(&mut db, id, &blocked, ts(DAY)).await,
        Ok(false)
    );
}

/// TD-CALL-05 (CALL-018): a homeowner who chose email contact still gets a masked line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_05_an_email_preferred_homeowner_still_gets_a_line(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, _, _) = business(&mut db).await;
    let h = her(&mut db).await;
    set_update_channel(&mut db, h, UpdateChannel::Email, ts(DAY))
        .await
        .expect("channel");
    let (_, id) = line(&mut db, (h, b)).await;
    assert_eq!(count(&mut db, OPEN, &id.to_string()).await, 1);
    assert_eq!(call(&mut db, HER_PHONE).await, Step::Connect(id));
}

/// TD-CALL-07 and TD-BIZ-34 (CALL-012, D-260): removing a member who linked two phones unlinks
/// both, and a later call from either hears the PIN prompt. Whether a member-linked phone
/// connects before that needs the owner's approval (BIZ-033, `gate`), which is TD-CALL-08's case,
/// so this checks only that both were linked and then unlinked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_07_a_removed_members_phones_hear_the_pin_prompt(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (b, owner, member) = business(&mut db).await;
    let dir = Directory(vault());
    for phone in [STAFF_A, STAFF_B] {
        let n = SecretString::from(phone.to_owned());
        link_phone(&mut db, &dir, (member, false), &n, ts(DAY))
            .await
            .expect("link");
    }
    let h = her(&mut db).await;
    line(&mut db, (h, b)).await;
    assert_eq!(count(&mut db, PHONES, &b.to_string()).await, 2);
    remove_member(&mut db, owner, member.member, ts(DAY))
        .await
        .expect("remove");
    assert_eq!(count(&mut db, PHONES, &b.to_string()).await, 0);
    for phone in [STAFF_A, STAFF_B] {
        assert_eq!(call(&mut db, phone).await, Step::AskPin);
    }
}

/// TD-CALL-07 (CALL-020, D-227): the Ask Hava number plays the greeting in English then Spanish
/// and hangs up: no voicemail, no recording, no forward.
#[test]
fn td_call_07_the_ask_hava_number_plays_the_greeting_and_hangs_up() {
    let twiml = greeting_twiml();
    let en = twiml.find(&greeting_en()).expect("English");
    let es = twiml.find(&greeting_es()).expect("Spanish");
    assert!(en < es && twiml.ends_with("<Hangup/></Response>"));
    assert!(greeting_en().starts_with("This is Ask Hava LLC. We work by text."));
    assert!(greeting_en().ends_with("If this is an emergency, hang up and call 911."));
    for verb in [
        "<Record",
        "<Dial",
        "<Gather",
        "<Redirect",
        "<Enqueue",
        "record=",
        "voicemail",
    ] {
        assert!(!twiml.contains(verb), "{verb}");
    }
}

/// TD-WORD-35 (WORD-001, D-1932): under the embedded brand the greetings, the code call and the
/// safety call say today's words byte for byte, and each opens with the brand file's legal or
/// spoken legal name.
#[test]
fn td_word_35_the_voice_lines_come_from_the_brand_and_are_unchanged() {
    assert_eq!(
        greeting_en(),
        "This is Ask Hava LLC. We work by text. To start a request, text this number. For help, \
         text HELP. If this is an emergency, hang up and call 911."
    );
    assert_eq!(
        greeting_es(),
        "Habla Ask Hava LLC. Trabajamos por mensaje de texto. Para hacer una solicitud, mande un \
         texto a este número. Para ayuda, mande AYUDA. Si es una emergencia, cuelgue y llame al 911."
    );
    assert!(greeting_en().starts_with(&format!("This is {}.", brand().legal_name())));
    assert!(greeting_es().starts_with(&format!("Habla {}.", brand().es().legal_name())));
    let pool = pool().numbers()[0].clone();
    let script = safety_script("+19285550100", &pool).expect("script");
    assert_eq!(
        script,
        "This is Ask Hava L L C. You sent us a message about a danger. Leave the building now \
         and call 9 1 1. Ask Hava is not an emergency service. To call us back, dial \
         9 2 8, 5 5 5, 0 1 0 0. Press 9 to stop all calls from Ask Hava."
    );
    assert!(script.starts_with(&format!("This is {}.", brand().spoken_legal_name())));
    let code = SecretString::from("1234".to_owned());
    let call = code_call_twiml(&pool, "+19285550100", &code, "/again").expect("code call");
    assert!(call.expose_secret().contains(&format!(
        "This is {}. Your code is 1, 2, 3, 4.",
        brand().legal_name()
    )));
}

/// TD-CALL-08 (CALL-012, BIZ-033, D-288, D-311): with the approval state seeded, a phone a
/// non-owner linked neither places nor takes a masked call as the business until the owner
/// approved it; an owner-linked phone works at once.
#[test]
fn td_call_08_a_member_linked_phone_works_only_after_approval() {
    let (h, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let lines = [RoutedLine {
        line: Uuid::from_u128(4_321),
        homeowner: h,
        business: b,
    }];
    let turn = CallTurn {
        attestation: Attestation::A,
        tries_call: 0,
        tries_today: 0,
        entered: None,
    };
    let seeded = |linked_by_owner, owner_approved| PhoneStanding {
        linked_by_owner,
        owner_approved,
    };
    let waiting = seeded(false, false);
    assert!(!phone_works(waiting));
    assert_eq!(gate(Party::Business(b), waiting), Party::Unlinked);
    assert_eq!(
        route(gate(Party::Business(b), waiting), &lines, turn),
        Step::AskPin
    );
    for works in [seeded(false, true), seeded(true, false)] {
        assert!(phone_works(works));
        let party = gate(Party::Business(b), works);
        assert_eq!(route(party, &lines, turn), Step::Connect(lines[0].line));
    }
    assert_eq!(gate(Party::Homeowner(h), waiting), Party::Homeowner(h));
}
