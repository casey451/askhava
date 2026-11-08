//! "Download my data" and the code-checked delete or copy request of the homeowner portal
//! (PRIV-031, D-492; T087, D-1936, D-1939).
//!
//! [`download_file`] builds the file a signed-in homeowner downloads: her own requests, quotes,
//! messages and consent records still held, as JSON, keeping only the items whose contact is hers
//! and masking any phone number, email address or web address in the quotes and messages, so no
//! business contact detail leaves in it. A delete or copy request is held until the Verify code
//! sent to the number on file is typed back: [`open_request`] stores the request (an `ops` audit
//! row whose ID is the request's and whose time is its received time) and texts a code;
//! [`confirm_request`] acts only on a stored, unanswered request of hers younger than
//! [`ANSWER_WITHIN`] and a correct code, so a code alone (the sign-in one included) acts on
//! nothing. A delete clears only her own jobs' photos (`intake::api::jobs_of_contact`), in one
//! transaction with its confirmed audit row (R11). Each request is due [`ANSWER_WITHIN`] (30 days)
//! after its stored received time ([`DataRequest::due_at`]). The number on file comes only from a
//! realm-checked vault read ([`OnFile`], SEC-046). Requests by email or the help form are T200's
//! (D-493). The routes wait on homeowner sign-in being served (D-1934).

use std::fmt;

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_intake::api::jobs_of_contact;
use askhava_ops::api::{Actor, AuditEntry, Change, audit, audit_once, export_audit};
use askhava_picks::api::rules::parse_code;
use askhava_privacy::api::delete_my_data;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::homeowner_portal::OnFile;

/// The download's path in the homeowner portal (not served yet, D-1934).
pub const DOWNLOAD_ROUTE: &str = "/h/my-data";

/// The path that opens a delete or copy request (not served yet, D-1934).
pub const DATA_REQUEST_ROUTE: &str = "/h/my-data/request";

/// The path of a request's code step (not served yet, D-1934).
pub const DATA_CODE_ROUTE: &str = "/h/my-data/code";

/// How long Ask Hava has to answer a delete or copy request (PRIV-031).
pub const ANSWER_WITHIN: SignedDuration = SignedDuration::from_hours(30 * 24);

/// One Verify send key per contact, kind and ten minutes (the code's life), so a double submit
/// reuses the key (R13).
const SEND_WINDOW_SECONDS: i64 = 600;

/// The audit subject of a request's received row: her vault contact.
const CONTACT_SUBJECT: &str = "privacy.contact";

/// The audit subject of a request's confirmed row: the request, by its received row's ID.
const REQUEST_SUBJECT: &str = "portals.data_request";

/// What a homeowner asked for (PRIV-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRequestKind {
    /// A copy of her data.
    Copy,
    /// Delete her data (PRIV-011).
    Delete,
}

impl DataRequestKind {
    /// The audit actions for this request: received, then acted on.
    fn actions(self) -> (&'static str, &'static str) {
        match self {
            Self::Copy => ("portals.data_copy_requested", "portals.data_copy_confirmed"),
            Self::Delete => (
                "portals.data_delete_requested",
                "portals.data_delete_confirmed",
            ),
        }
    }

    fn from_received(action: &str) -> Option<Self> {
        [Self::Copy, Self::Delete]
            .into_iter()
            .find(|kind| kind.actions().0 == action)
    }
}

/// A request as stored: its ID (the received audit row's), kind and received time (PRIV-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRequest {
    /// The request's ID, carried by its code step's form.
    pub id: i64,
    /// Copy or delete.
    pub kind: DataRequestKind,
    /// When it was received, as stored.
    pub received_at: Timestamp,
}

impl DataRequest {
    /// PRIV-031: the answer is due 30 days after the request was received.
    #[must_use]
    pub fn due_at(&self) -> Timestamp {
        self.received_at
            .saturating_add(ANSWER_WITHIN)
            .unwrap_or(Timestamp::MAX)
    }
}

/// Why a request was not recorded or not acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    /// The code was not the one sent, or not six digits; nothing was done.
    #[error("the code did not match")]
    WrongCode,
    /// No open request of hers has that ID: never made, someone else's, answered or past its
    /// due date; nothing was done.
    #[error("no open request")]
    NoRequest,
    /// Verify could not send or check the code; nothing was done.
    #[error("the code could not be sent or checked")]
    Verify,
    /// A database call failed.
    #[error("data request database call failed")]
    Database,
}

/// What a confirmed request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acted {
    /// The copy may be handed over now ([`download_file`]).
    Copy,
    /// Her data was deleted; the PRIV-011 sentence to show.
    Deleted(&'static str),
}

fn entry(on_file: &OnFile, action: &'static str, subject: (&'static str, Uuid)) -> AuditEntry {
    AuditEntry {
        actor: Actor::Homeowner(on_file.contact()),
        action,
        subject_kind: subject.0,
        subject_id: subject.1,
        change: Change::Action,
    }
}

fn database<E>(_: E) -> RequestError {
    RequestError::Database
}

/// PRIV-031: stores `kind` as received (its audit row, whose time is the received time) and
/// texts a Verify code to the number on file. The row is written first, so a request is on record
/// even if the send fails. The send key is the contact, kind and ten-minute window of `now`.
///
/// # Errors
/// [`RequestError::Database`] or [`RequestError::Verify`] when the code could not be sent.
pub async fn open_request<V: VerifyAdapter>(
    db: &mut PgConnection,
    (verify, key): (&V, &SecretString),
    on_file: &OnFile,
    (kind, now): (DataRequestKind, Timestamp),
) -> Result<DataRequest, RequestError> {
    let subject = (CONTACT_SUBJECT, on_file.contact());
    let id = audit(db, &entry(on_file, kind.actions().0, subject))
        .await
        .map_err(database)?;
    let request = stored(db, on_file, id)
        .await?
        .ok_or(RequestError::Database)?;
    let window = now.as_second().div_euclid(SEND_WINDOW_SECONDS).to_string();
    let contact = on_file.contact().to_string();
    let parts = [contact.as_str(), kind.actions().0, window.as_str()];
    let idem = IdempotencyKey::derive(key, "verify.data_request", &parts);
    let phone = on_file.phone().expose_secret();
    verify
        .send_code(&idem, phone, CodeChannel::Sms)
        .await
        .map_err(|_| RequestError::Verify)?;
    Ok(request)
}

/// The stored request `id` when it is a received row made by and about `on_file`'s contact.
async fn stored(
    db: &mut PgConnection,
    on_file: &OnFile,
    id: i64,
) -> Result<Option<DataRequest>, RequestError> {
    let rows = export_audit(db, id.saturating_sub(1), 1)
        .await
        .map_err(database)?;
    let hers = |row: &askhava_ops::api::AuditRow| {
        row.id == id
            && row.actor_kind == "homeowner"
            && row.actor_id == Some(on_file.contact())
            && row.subject_kind == CONTACT_SUBJECT
            && row.subject_id == on_file.contact()
    };
    Ok(rows.into_iter().find(hers).and_then(|row| {
        let kind = DataRequestKind::from_received(&row.action)?;
        let received_at = row.at;
        Some(DataRequest {
            id,
            kind,
            received_at,
        })
    }))
}

/// PRIV-031, PRIV-011: acts on her open request `request_id` only when it is hers, unanswered and
/// not past its due date at `now`, and `code` is the Verify code sent to the number on file; else
/// nothing is written or deleted. The confirmed audit row (one per request) and a delete of her
/// vault entry and her own jobs' photos (`privacy::api::delete_my_data`) commit together.
///
/// # Errors
/// [`RequestError`].
pub async fn confirm_request<V: VerifyAdapter>(
    db: &mut PgConnection,
    verify: &V,
    on_file: &OnFile,
    (request_id, code, now): (i64, &str, Timestamp),
) -> Result<Acted, RequestError> {
    let request = stored(db, on_file, request_id)
        .await?
        .filter(|request| now <= request.due_at())
        .ok_or(RequestError::NoRequest)?;
    let code = parse_code(code).ok_or(RequestError::WrongCode)?;
    let phone = on_file.phone().expose_secret();
    match verify.check_code(phone, code.digits()).await {
        Ok(true) => {}
        Ok(false) => return Err(RequestError::WrongCode),
        Err(_) => return Err(RequestError::Verify),
    }
    let mut tx = db.begin().await.map_err(database)?;
    let subject = (
        REQUEST_SUBJECT,
        Uuid::from_u128(request_id.unsigned_abs().into()),
    );
    let done = entry(on_file, request.kind.actions().1, subject);
    if !audit_once(&mut tx, &done).await.map_err(database)? {
        return Err(RequestError::NoRequest);
    }
    let acted = match request.kind {
        DataRequestKind::Copy => Acted::Copy,
        DataRequestKind::Delete => {
            let jobs = jobs_of_contact(&mut tx, on_file.contact())
                .await
                .map_err(database)?;
            let deleted = delete_my_data(&mut tx, on_file.contact(), &jobs).await;
            Acted::Deleted(deleted.map_err(database)?)
        }
    };
    tx.commit().await.map_err(database)?;
    Ok(acted)
}

/// The four parts of the download (PRIV-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Her requests.
    Requests,
    /// The quotes on her requests: business name, quote and time only; a business's phone, email
    /// or web address in the words is masked.
    Quotes,
    /// Her messages still held (bodies are cut at 30 days, TXT-020); contact details masked.
    Messages,
    /// Her consent and opt-out records.
    Consent,
}

impl Section {
    const ALL: [Self; 4] = [Self::Requests, Self::Quotes, Self::Messages, Self::Consent];

    fn key(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::Quotes => "quotes",
            Self::Messages => "messages",
            Self::Consent => "consent",
        }
    }

    /// Whether a business's words can appear in this part.
    fn has_business_words(self) -> bool {
        matches!(self, Self::Quotes | Self::Messages)
    }
}

/// One held record the composition read for the download, with the contact it belongs to.
/// `Debug` prints no words (PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct HeldItem {
    /// The vault contact the record belongs to.
    pub contact: Uuid,
    /// Which part of the file it goes in.
    pub section: Section,
    /// When it happened.
    pub at: Timestamp,
    /// What it says, in plain words.
    pub words: String,
}

impl fmt::Debug for HeldItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeldItem")
            .field("contact", &self.contact)
            .field("section", &self.section)
            .field("at", &self.at)
            .field("words", &"[redacted]")
            .finish()
    }
}

/// What stands in for a masked contact detail.
const MASKED: &str = "[removed]";

/// PRIV-031: `words` with every run of 7 or more digits (with spaces, dots, dashes, brackets or a
/// plus between them), every word holding `@` and every web address masked.
fn mask_contact_details(words: &str) -> String {
    let (mut out, mut run) = (String::new(), String::new());
    for ch in words.chars() {
        if ch.is_ascii_digit() || "()+-. ".contains(ch) {
            run.push(ch);
        } else {
            flush_run(&mut out, &mut run);
            out.push(ch);
        }
    }
    flush_run(&mut out, &mut run);
    let web = |w: &str| {
        let lower = w.to_ascii_lowercase();
        w.contains('@') || lower.starts_with("http") || lower.starts_with("www.")
    };
    let masked: Vec<&str> = out
        .split(' ')
        .map(|w| if web(w) { MASKED } else { w })
        .collect();
    masked.join(" ")
}

fn flush_run(out: &mut String, run: &mut String) {
    if run.chars().filter(char::is_ascii_digit).count() >= 7 {
        let lead = run.len() - run.trim_start().len();
        let tail = run.trim_end().len();
        out.push_str(&run[..lead]);
        out.push_str(MASKED);
        out.push_str(&run[tail..]);
    } else {
        out.push_str(run);
    }
    run.clear();
}

/// PRIV-031: the "Download my data" file for the homeowner `on_file` names, as JSON with the four
/// sections; an item of any other contact is left out, and contact details in the quotes and
/// messages are masked, so the file holds no business contact detail.
#[must_use]
pub fn download_file(on_file: &OnFile, items: &[HeldItem]) -> String {
    let mut file = Map::new();
    for section in Section::ALL {
        let words = |i: &HeldItem| {
            if section.has_business_words() {
                mask_contact_details(&i.words)
            } else {
                i.words.clone()
            }
        };
        let held: Vec<Value> = items
            .iter()
            .filter(|i| i.contact == on_file.contact() && i.section == section)
            .map(|i| json!({ "at": i.at.to_string(), "words": words(i) }))
            .collect();
        file.insert(section.key().to_owned(), Value::Array(held));
    }
    Value::Object(file).to_string()
}

#[cfg(test)]
mod tests {
    //! TD-PRIV-15 (PRIV-031): the file holds only her records and no business contact detail; a
    //! request waits for its own right code, is due 30 days after its stored received time and is
    //! acted on once. Fake data only.

    use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
    use askhava_ops::api::export_audit;
    use jiff::{SignedDuration, Timestamp};
    use secrecy::SecretString;
    use sqlx::types::Uuid;
    use sqlx::{PgConnection, PgPool};

    use super::{
        Acted, DataRequest, DataRequestKind, HeldItem, RequestError, Section, confirm_request,
        download_file, open_request,
    };
    use crate::homeowner_portal::OnFile;

    const CODE: &str = "123456";

    struct FakeVerify;

    impl VerifyAdapter for FakeVerify {
        async fn send_code(
            &self,
            _: &IdempotencyKey,
            _: &str,
            _: CodeChannel,
        ) -> Result<String, AdapterError> {
            Ok("VEfake".to_owned())
        }
        async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
            Ok(code == CODE)
        }
    }

    fn her() -> OnFile {
        OnFile::new(Uuid::from_u128(7), "+19285550100")
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("time")
    }

    fn item(contact: u128, section: Section, words: &str) -> HeldItem {
        let (contact, words) = (Uuid::from_u128(contact), words.to_owned());
        HeldItem {
            contact,
            section,
            at: at(1_800_000_000),
            words,
        }
    }

    async fn actions(db: &mut PgConnection) -> Vec<String> {
        let rows = export_audit(db, 0, 100).await.expect("audit");
        rows.into_iter()
            .map(|row| row.action)
            .filter(|a| a.starts_with("portals."))
            .collect()
    }

    async fn open(db: &mut PgConnection, kind: DataRequestKind) -> DataRequest {
        let key = SecretString::from("fake-key-for-tests".to_owned());
        let now = Timestamp::now();
        let opened = open_request(db, (&FakeVerify, &key), &her(), (kind, now)).await;
        opened.expect("open")
    }

    /// TD-PRIV-15: her four sections, no other person's record.
    #[test]
    fn the_file_holds_only_her_records() {
        let items = [
            item(7, Section::Requests, "Leaky kitchen faucet"),
            item(7, Section::Quotes, "Example Plumbing: $180"),
            item(7, Section::Consent, "Texts on"),
            item(8, Section::Messages, "Someone else's words"),
        ];
        let file = download_file(&her(), &items);
        assert!(file.contains("Leaky kitchen faucet") && file.contains("Example Plumbing"));
        assert!(file.contains("\"messages\":[]") && file.contains("Texts on"));
        assert!(!file.contains("Someone else"));
    }

    /// TD-PRIV-15: no business phone, email or web address leaves in the file, and `Debug` of a
    /// held item prints no words.
    #[test]
    fn the_file_holds_no_business_contact_detail() {
        let quote = "Example Plumbing: $180. Call (928) 555-0142 or +1 928.555.0143, \
                     office@plumbing.example, https://plumbing.example or www.plumbing.example";
        let items = [
            item(7, Section::Quotes, quote),
            item(7, Section::Messages, "Text me at 555-0144 tomorrow"),
        ];
        let file = download_file(&her(), &items);
        assert!(file.contains("Example Plumbing: $180. Call [removed] or [removed],"));
        assert!(file.contains("Text me at [removed] tomorrow"));
        for gone in ["555", "@", "http", "www"] {
            assert!(!file.contains(gone), "{gone}");
        }
        assert!(!format!("{:?}", items[0]).contains("Plumbing"));
    }

    /// TD-PRIV-15: received as stored and due 30 days later; a wrong code acts on nothing, the
    /// right one acts once.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_request_waits_for_the_right_code(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        let request = open(&mut db, DataRequestKind::Delete).await;
        let due = request.received_at + SignedDuration::from_hours(30 * 24);
        assert_eq!(request.due_at(), due);
        let now = request.received_at;
        for wrong in ["654321", "12", "abcdef"] {
            let try_wrong = (request.id, wrong, now);
            let refused = confirm_request(&mut db, &FakeVerify, &her(), try_wrong).await;
            assert_eq!(refused, Err(RequestError::WrongCode));
        }
        assert_eq!(actions(&mut db).await, ["portals.data_delete_requested"]);
        let done = confirm_request(&mut db, &FakeVerify, &her(), (request.id, CODE, now)).await;
        assert!(matches!(done, Ok(Acted::Deleted(_))));
        let again = confirm_request(&mut db, &FakeVerify, &her(), (request.id, CODE, now)).await;
        assert_eq!(again, Err(RequestError::NoRequest));
        let last = actions(&mut db).await;
        assert_eq!(
            last,
            [
                "portals.data_delete_requested",
                "portals.data_delete_confirmed"
            ]
        );
    }

    /// TD-PRIV-15: a right code with no open request of hers (none made, someone else's, or past
    /// its due date) acts on nothing; a copy request gives the copy.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_code_without_her_open_request_acts_on_nothing(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        let request = open(&mut db, DataRequestKind::Copy).await;
        let now = request.received_at;
        let other = OnFile::new(Uuid::from_u128(8), "+19285550101");
        let late = request.due_at() + SignedDuration::from_secs(1);
        let cases = [
            (&her(), request.id + 1, now),
            (&other, request.id, now),
            (&her(), request.id, late),
        ];
        for (who, id, when) in cases {
            let refused = confirm_request(&mut db, &FakeVerify, who, (id, CODE, when)).await;
            assert_eq!(refused, Err(RequestError::NoRequest));
        }
        assert_eq!(actions(&mut db).await, ["portals.data_copy_requested"]);
        let copy = confirm_request(&mut db, &FakeVerify, &her(), (request.id, CODE, now)).await;
        assert_eq!(copy, Ok(Acted::Copy));
    }

    async fn request_with_job(db: &mut PgConnection, contact: Uuid, job: Uuid) {
        let homeowner = sqlx::query_scalar!(
            "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, now()) RETURNING id",
            contact,
        )
        .fetch_one(&mut *db)
        .await
        .expect("homeowner");
        sqlx::query!(
            "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, \
             consent_template, consent_sent_at, opened_at) VALUES ($1, $2, 'en', \
             gen_random_uuid(), 'consent_first', now(), now())",
            homeowner,
            job,
        )
        .execute(&mut *db)
        .await
        .expect("conversation");
    }

    /// TD-PRIV-15 (PRIV-011): a delete clears the photos of her own jobs only, never a job the
    /// browser names, so another homeowner's job is not hers to clear.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_delete_reaches_only_her_own_jobs(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        let (mine, theirs) = (Uuid::from_u128(0x71), Uuid::from_u128(0x81));
        request_with_job(&mut db, her().contact(), mine).await;
        request_with_job(&mut db, Uuid::from_u128(8), theirs).await;
        let jobs = askhava_intake::api::jobs_of_contact(&mut db, her().contact()).await;
        assert_eq!(jobs.expect("jobs"), [mine]);
    }
}
