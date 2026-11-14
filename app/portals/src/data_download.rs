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
//! nothing; each code is bound to its own request by a local challenge with a try cap
//! (`crate::code_challenge`, SEC-055, T346), [`resend_request_code`] texts it again, and all
//! three take the pool and hold no connection across Verify. A delete ([`delete_her_data`]) clears only her own jobs' photos
//! (`intake::api::jobs_of_contact`) and the job text and cards stored under every request of hers
//! (`quotes::api::forget_requests`, T411, PRIV-008, PRIV-011) and blanks those jobs' feedback
//! words (`jobs::api::purge_feedback`, T315, JOB-017, D-2205), in one transaction with its
//! confirmed audit row (R11). The masking also catches a bare domain and a phone number split
//! across lines (T315, D-3265). Each request is due [`ANSWER_WITHIN`] (30 days)
//! after its stored received time ([`DataRequest::due_at`]). The number on file comes only from a
//! realm-checked vault read ([`OnFile`], SEC-046). Requests by email or the help form are T200's
//! (D-493). The routes wait on homeowner sign-in being served (D-1934).

use std::fmt;

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_intake::api::{conversations_of_contact, jobs_of_contact};
use askhava_jobs::api::purge_feedback;
use askhava_ops::api::{Actor, AuditEntry, Change, audit, audit_once, export_audit};
use askhava_picks::api::rules::parse_code;
use askhava_privacy::api::delete_my_data;
use askhava_quotes::api::forget_requests;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::code_challenge::{Purpose, open_challenge, reserve_try, use_challenge};
use crate::homeowner_portal::OnFile;

/// The download's path in the homeowner portal (not served yet, D-1934).
pub const DOWNLOAD_ROUTE: &str = "/h/my-data";

/// The path that opens a delete or copy request (not served yet, D-1934).
pub const DATA_REQUEST_ROUTE: &str = "/h/my-data/request";

/// The path of a request's code step (not served yet, D-1934).
pub const DATA_CODE_ROUTE: &str = "/h/my-data/code";

/// How long Ask Hava has to answer a delete or copy request (PRIV-031).
pub const ANSWER_WITHIN: SignedDuration = SignedDuration::from_hours(30 * 24);

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
    /// SEC-055: the request has no live code (none sent, expired, used, or its tries spent);
    /// Verify was not called and nothing was done; a new code is needed.
    #[error("no live code for this request")]
    NoLiveCode,
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

/// PRIV-031, SEC-055: stores `kind` as received (its audit row, whose time is the received time)
/// and texts a Verify code to the number on file under the request's own challenge
/// ([`send_request_code`]). The row is written first, so a request is on record even if the send
/// fails. No connection is held across Verify.
///
/// # Errors
/// [`RequestError::Database`] or [`RequestError::Verify`] when the code could not be sent.
pub async fn open_request<V: VerifyAdapter>(
    pool: &PgPool,
    (verify, key): (&V, &SecretString),
    on_file: &OnFile,
    (kind, now): (DataRequestKind, Timestamp),
) -> Result<DataRequest, RequestError> {
    let request = {
        let mut db = pool.acquire().await.map_err(database)?;
        let subject = (CONTACT_SUBJECT, on_file.contact());
        let id = audit(&mut db, &entry(on_file, kind.actions().0, subject))
            .await
            .map_err(database)?;
        stored(&mut db, on_file, id)
            .await?
            .ok_or(RequestError::Database)?
    };
    send_request_code(pool, (verify, key), on_file, (request, now)).await?;
    Ok(request)
}

/// PRIV-031, SEC-055: texts a new code for her open request `request_id` (hers, not past its due
/// date at `now`); inside the code's life it reuses the request's challenge, so it gives no more
/// tries. No connection is held across Verify.
///
/// # Errors
/// [`RequestError::NoRequest`], [`RequestError::Database`] or [`RequestError::Verify`].
pub async fn resend_request_code<V: VerifyAdapter>(
    pool: &PgPool,
    (verify, key): (&V, &SecretString),
    on_file: &OnFile,
    (request_id, now): (i64, Timestamp),
) -> Result<DataRequest, RequestError> {
    let request = open_one(pool, on_file, request_id, now).await?;
    send_request_code(pool, (verify, key), on_file, (request, now)).await?;
    Ok(request)
}

/// The challenge purpose and subject of `request` (SEC-055).
fn challenge_of(request: &DataRequest) -> (Purpose, Uuid) {
    let purpose = match request.kind {
        DataRequestKind::Copy => Purpose::DataCopy,
        DataRequestKind::Delete => Purpose::DataDelete,
    };
    (purpose, request_subject(request.id))
}

fn request_subject(request_id: i64) -> Uuid {
    Uuid::from_u128(request_id.unsigned_abs().into())
}

/// SEC-055: opens or reuses `request`'s challenge in its own committed step, then sends the code;
/// the send key is the contact, kind and challenge, so a double submit reuses it (R13).
async fn send_request_code<V: VerifyAdapter>(
    pool: &PgPool,
    (verify, key): (&V, &SecretString),
    on_file: &OnFile,
    (request, now): (DataRequest, Timestamp),
) -> Result<(), RequestError> {
    let challenge = open_challenge(pool, challenge_of(&request), now)
        .await
        .map_err(database)?
        .to_string();
    let contact = on_file.contact().to_string();
    let parts = [
        contact.as_str(),
        request.kind.actions().0,
        challenge.as_str(),
    ];
    let idem = IdempotencyKey::derive(key, "verify.data_request", &parts);
    let phone = on_file.phone().expose_secret();
    verify
        .send_code(&idem, phone, CodeChannel::Sms)
        .await
        .map(|_| ())
        .map_err(|_| RequestError::Verify)
}

/// Her stored request `request_id` while not past its due date at `now`, on a connection held
/// only for the read.
async fn open_one(
    pool: &PgPool,
    on_file: &OnFile,
    request_id: i64,
    now: Timestamp,
) -> Result<DataRequest, RequestError> {
    let mut db = pool.acquire().await.map_err(database)?;
    stored(&mut db, on_file, request_id)
        .await?
        .filter(|request| now <= request.due_at())
        .ok_or(RequestError::NoRequest)
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

/// PRIV-031, PRIV-011, SEC-055: acts on her open request `request_id` only when it is hers, not
/// past its due date at `now`, a try is reserved against the request's own live challenge
/// ([`reserve_try`]; none, and Verify is never called: [`RequestError::NoLiveCode`]) and `code`
/// is the Verify code sent to the number on file; else nothing is written or deleted. The used
/// challenge, the confirmed audit row (one per request) and a delete ([`delete_her_data`]) commit
/// together, so two right checks act once. No connection is held across Verify.
///
/// # Errors
/// [`RequestError`].
pub async fn confirm_request<V: VerifyAdapter>(
    pool: &PgPool,
    verify: &V,
    on_file: &OnFile,
    (request_id, code, now): (i64, &str, Timestamp),
) -> Result<Acted, RequestError> {
    let request = open_one(pool, on_file, request_id, now).await?;
    let code = parse_code(code).ok_or(RequestError::WrongCode)?;
    let reserved = reserve_try(pool, challenge_of(&request), now).await;
    let challenge = reserved
        .map_err(database)?
        .ok_or(RequestError::NoLiveCode)?;
    let phone = on_file.phone().expose_secret();
    match verify.check_code(phone, code.digits()).await {
        Ok(true) => act(pool, on_file, (request, challenge, now)).await,
        Ok(false) => Err(RequestError::WrongCode),
        Err(_) => Err(RequestError::Verify),
    }
}

/// SEC-055, R11: uses `challenge`, writes the confirmed row and, for a delete, deletes her data,
/// in one transaction; a challenge or request already used rolls back as
/// [`RequestError::NoRequest`].
async fn act(
    pool: &PgPool,
    on_file: &OnFile,
    (request, challenge, now): (DataRequest, i64, Timestamp),
) -> Result<Acted, RequestError> {
    let mut tx = pool.begin().await.map_err(database)?;
    if !use_challenge(&mut tx, challenge, now)
        .await
        .map_err(database)?
    {
        return Err(RequestError::NoRequest);
    }
    let subject = (REQUEST_SUBJECT, request_subject(request.id));
    let done = entry(on_file, request.kind.actions().1, subject);
    if !audit_once(&mut tx, &done).await.map_err(database)? {
        return Err(RequestError::NoRequest);
    }
    let acted = match request.kind {
        DataRequestKind::Copy => Acted::Copy,
        DataRequestKind::Delete => {
            let deleted = delete_her_data(&mut tx, on_file.contact()).await;
            Acted::Deleted(deleted.map_err(database)?)
        }
    };
    tx.commit().await.map_err(database)?;
    Ok(acted)
}

/// PRIV-011, PRIV-008, D-3189: "Delete my data" on the caller's transaction: the stored job text
/// and cards of every request of hers (open or closed, `intake::api::conversations_of_contact`;
/// T411), then her feedback words on each of those jobs (`jobs::api::purge_feedback`, audited as
/// her, JOB-017, T315), then `privacy::api::delete_my_data` over her jobs
/// (`intake::api::jobs_of_contact`) and every job those rows named, so a later job's photos go
/// too. Returns the confirmation sentence.
///
/// # Errors
/// [`DeleteError`] naming the step that failed.
pub async fn delete_her_data(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<&'static str, DeleteError> {
    let requests = conversations_of_contact(db, contact)
        .await
        .map_err(|_| DeleteError::Intake)?;
    let mut jobs = jobs_of_contact(db, contact)
        .await
        .map_err(|_| DeleteError::Intake)?;
    let named = forget_requests(db, &requests)
        .await
        .map_err(|_| DeleteError::Quotes)?;
    jobs.extend(named);
    jobs.sort_unstable();
    jobs.dedup();
    for job in &jobs {
        purge_feedback(db, *job, Actor::Homeowner(contact))
            .await
            .map_err(|_| DeleteError::Jobs)?;
    }
    delete_my_data(db, contact, &jobs)
        .await
        .map_err(|_| DeleteError::Privacy)
}

/// Which step of [`delete_her_data`] failed; the transaction then commits nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeleteError {
    /// Reading her requests or jobs failed.
    #[error("reading her requests failed")]
    Intake,
    /// Deleting the stored job text failed.
    #[error("deleting the job text failed")]
    Quotes,
    /// Blanking her feedback words failed.
    #[error("blanking the feedback failed")]
    Jobs,
    /// The vault and photo delete failed.
    #[error("the vault delete failed")]
    Privacy,
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

/// PRIV-031: `words` with every run of 7 or more digits (joined by [`RUN_SEPARATORS`], any
/// whitespace or a Unicode dash), every word holding `@`, every web address and every bare
/// domain masked (T315, T448, D-3265, D-3266, D-3592).
pub(crate) fn mask_contact_details(words: &str) -> String {
    let (mut out, mut run) = (String::new(), String::new());
    for ch in words.chars() {
        if is_run_digit(ch) || joins_run(ch) {
            run.push(ch);
        } else {
            flush_run(&mut out, &mut run);
            out.push(ch);
        }
    }
    flush_run(&mut out, &mut run);
    let (mut masked, mut word) = (String::new(), String::new());
    for ch in out.chars() {
        if ch.is_whitespace() {
            masked.push_str(if is_web(&word) { MASKED } else { &word });
            masked.push(ch);
            word.clear();
        } else {
            word.push(ch);
        }
    }
    masked.push_str(if is_web(&word) { MASKED } else { &word });
    masked
}

/// Whether one word is an email, a web address or a bare domain.
fn is_web(word: &str) -> bool {
    word.contains('@') || word.contains("://") || is_domain(word)
}

/// Where a host ends or a scheme, label or wrapper sits around one (D-3266).
const HOST_CUTS: &[char] = &[
    ':', '/', '?', '#', '\'', '"', '(', ')', '<', '>', '[', ']', '\u{2018}', '\u{2019}',
    '\u{201c}', '\u{201d}',
];

/// PRIV-031, D-3265, D-3266: some piece of the word, cut at a scheme, port, path, query, fragment,
/// apostrophe or wrapping bracket or quote, is two or more dot-separated labels of letters, digits
/// or dashes whose last is 2 to 24 letters; "3.5", "$1,250.00", "Dr.", "e.g." and "p.m." are not.
fn is_domain(word: &str) -> bool {
    word.split(HOST_CUTS).any(is_host)
}

/// Whether `piece`, without wrapping punctuation, is a host name of two or more labels.
fn is_host(piece: &str) -> bool {
    let host = piece.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let labels: Vec<&str> = host.split('.').collect();
    let last = labels.last().copied().unwrap_or_default();
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && (2..=24).contains(&last.len())
        && last.chars().all(|c| c.is_ascii_alphabetic())
}

/// The punctuation that continues a digit run (D-3592); `,`, `:`, `$` and every other character
/// not named here or in [`joins_run`] end one, so money and durations are never joined.
const RUN_SEPARATORS: &[char] = &['(', ')', '+', '-', '.', '/', '_'];

/// PRIV-031, D-3592: whether `ch` may sit inside a digit run: [`RUN_SEPARATORS`], any whitespace
/// (tab, non-breaking space and line breaks included), the dashes U+2010 to U+2015 and U+2212.
fn joins_run(ch: char) -> bool {
    RUN_SEPARATORS.contains(&ch)
        || ch.is_whitespace()
        || ('\u{2010}'..='\u{2015}').contains(&ch)
        || ch == '\u{2212}'
}

/// PRIV-031, D-3592: an ASCII digit or a fullwidth one (U+FF10 to U+FF19).
fn is_run_digit(ch: char) -> bool {
    ch.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&ch)
}

/// PRIV-031: writes `run` to `out`, masked when it holds 7 or more digits, its outer whitespace kept.
fn flush_run(out: &mut String, run: &mut String) {
    if run.chars().filter(|&c| is_run_digit(c)).count() >= 7 {
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
    use sqlx::postgres::PgPoolOptions;
    use sqlx::types::Uuid;
    use sqlx::{PgConnection, PgPool};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Poll;

    use super::{
        Acted, DataRequest, DataRequestKind, HeldItem, RequestError, Section, confirm_request,
        download_file, open_request, resend_request_code,
    };
    use crate::homeowner_portal::OnFile;

    const CODE: &str = "123456";

    /// One shared Verify service: any code it sent to her number passes, whatever it was sent
    /// for (the sign-in code included); counts what reaches it, or hangs (TD-SEC-60).
    #[derive(Default)]
    struct Counting {
        reached: AtomicUsize,
        hang: bool,
    }

    impl VerifyAdapter for Counting {
        async fn send_code(
            &self,
            _: &IdempotencyKey,
            _: &str,
            _: CodeChannel,
        ) -> Result<String, AdapterError> {
            self.reached.fetch_add(1, Ordering::SeqCst);
            if self.hang {
                std::future::pending::<()>().await;
            }
            Ok("VEfake".to_owned())
        }
        async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
            self.reached.fetch_add(1, Ordering::SeqCst);
            if self.hang {
                std::future::pending::<()>().await;
            }
            Ok(code == CODE)
        }
    }

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

    async fn open(pool: &PgPool, kind: DataRequestKind) -> DataRequest {
        let key = SecretString::from("fake-key-for-tests".to_owned());
        let now = Timestamp::now();
        let opened = open_request(pool, (&FakeVerify, &key), &her(), (kind, now)).await;
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

    /// TD-PRIV-15, SEC-055: received as stored and due 30 days later; a wrong code acts on
    /// nothing, the right one acts once and uses the request's code.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_request_waits_for_the_right_code(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        let request = open(&pool, DataRequestKind::Delete).await;
        let due = request.received_at + SignedDuration::from_hours(30 * 24);
        assert_eq!(request.due_at(), due);
        let now = request.received_at;
        for wrong in ["654321", "12", "abcdef"] {
            let try_wrong = (request.id, wrong, now);
            let refused = confirm_request(&pool, &FakeVerify, &her(), try_wrong).await;
            assert_eq!(refused, Err(RequestError::WrongCode));
        }
        assert_eq!(actions(&mut db).await, ["portals.data_delete_requested"]);
        let done = confirm_request(&pool, &FakeVerify, &her(), (request.id, CODE, now)).await;
        assert!(matches!(done, Ok(Acted::Deleted(_))));
        let again = confirm_request(&pool, &FakeVerify, &her(), (request.id, CODE, now)).await;
        assert_eq!(again, Err(RequestError::NoLiveCode));
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
        let request = open(&pool, DataRequestKind::Copy).await;
        let now = request.received_at;
        let other = OnFile::new(Uuid::from_u128(8), "+19285550101");
        let late = request.due_at() + SignedDuration::from_secs(1);
        let cases = [
            (&her(), request.id + 1, now),
            (&other, request.id, now),
            (&her(), request.id, late),
        ];
        for (who, id, when) in cases {
            let refused = confirm_request(&pool, &FakeVerify, who, (id, CODE, when)).await;
            assert_eq!(refused, Err(RequestError::NoRequest));
        }
        assert_eq!(actions(&mut db).await, ["portals.data_copy_requested"]);
        let copy = confirm_request(&pool, &FakeVerify, &her(), (request.id, CODE, now)).await;
        assert_eq!(copy, Ok(Acted::Copy));
    }

    async fn challenges(pool: &PgPool) -> Vec<(String, i32, bool)> {
        let rows = sqlx::query!(
            r#"SELECT purpose, tries, used_at IS NOT NULL AS "used!" FROM portals.code_challenge ORDER BY id"#
        )
        .fetch_all(pool)
        .await
        .expect("challenges");
        rows.into_iter()
            .map(|r| (r.purpose, r.tries, r.used))
            .collect()
    }

    /// TD-SEC-60 (SEC-055, PRIV-031): a code that passes Verify for her number (a sign-in code)
    /// confirms no request whose own challenge expired or was used; each is refused before
    /// Verify is called.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_sec_60_a_request_needs_its_own_live_challenge(pool: PgPool) {
        let (verify, key) = (Counting::default(), SecretString::from("k".to_owned()));
        let t0 = Timestamp::now();
        let delete = (DataRequestKind::Delete, t0);
        let request = open_request(&pool, (&verify, &key), &her(), delete).await;
        let late = (
            request.expect("open").id,
            CODE,
            t0 + SignedDuration::from_secs(600),
        );
        let refused = confirm_request(&pool, &verify, &her(), late).await;
        assert_eq!(refused, Err(RequestError::NoLiveCode));
        let copy = (DataRequestKind::Copy, t0);
        let request = open_request(&pool, (&verify, &key), &her(), copy).await;
        let right = (request.expect("open").id, CODE, t0);
        let copied = confirm_request(&pool, &verify, &her(), right).await;
        assert_eq!(copied, Ok(Acted::Copy));
        let used = confirm_request(&pool, &verify, &her(), right).await;
        assert_eq!(used, Err(RequestError::NoLiveCode));
        assert_eq!(verify.reached.load(Ordering::SeqCst), 3);
        let rows = [
            ("data_delete".to_owned(), 0, false),
            ("data_copy".to_owned(), 1, true),
        ];
        assert_eq!(challenges(&pool).await, rows);
    }

    /// TD-SEC-60 (SEC-055): a re-send inside the code's life reuses the request's challenge and
    /// gives no more tries; the sixth check is refused before Verify; a re-send after expiry
    /// opens a new challenge, whose right code acts.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_sec_60_a_request_resend_gives_no_more_tries(pool: PgPool) {
        let (verify, key) = (Counting::default(), SecretString::from("k".to_owned()));
        let t0 = Timestamp::now();
        let delete = (DataRequestKind::Delete, t0);
        let id = open_request(&pool, (&verify, &key), &her(), delete)
            .await
            .expect("open")
            .id;
        for n in 0..6 {
            let when = t0 + SignedDuration::from_secs(n * 60);
            let again = resend_request_code(&pool, (&verify, &key), &her(), (id, when)).await;
            assert!(again.is_ok());
            let wrong = confirm_request(&pool, &verify, &her(), (id, "654321", t0)).await;
            let refused = [RequestError::WrongCode, RequestError::NoLiveCode][usize::from(n == 5)];
            assert_eq!(wrong, Err(refused));
        }
        assert_eq!(verify.reached.load(Ordering::SeqCst), 1 + 6 + 5);
        let fresh = t0 + SignedDuration::from_secs(600);
        assert!(
            resend_request_code(&pool, (&verify, &key), &her(), (id, fresh))
                .await
                .is_ok()
        );
        let done = confirm_request(&pool, &verify, &her(), (id, CODE, fresh)).await;
        assert!(matches!(done, Ok(Acted::Deleted(_))));
        let rows = [
            ("data_delete".to_owned(), 5, false),
            ("data_delete".to_owned(), 1, true),
        ];
        assert_eq!(challenges(&pool).await, rows);
    }

    /// TD-SEC-60 (AVAIL-01, D-2349): with Verify hanging, an open, a re-send and a check of a
    /// request leave a pool of one connection free.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_sec_60_request_steps_hold_no_connection_across_verify(pool: PgPool) {
        let one = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(2));
        let one = one
            .connect_with((*pool.connect_options()).clone())
            .await
            .expect("pool");
        let (key, t0, she) = (SecretString::from("k".to_owned()), Timestamp::now(), her());
        let id = open(&one, DataRequestKind::Copy).await.id;
        let hung = Counting {
            hang: true,
            ..Counting::default()
        };
        let delete = (DataRequestKind::Delete, t0);
        let mut steps: [Pin<Box<dyn Future<Output = bool> + '_>>; 3] = [
            Box::pin(async {
                open_request(&one, (&hung, &key), &she, delete)
                    .await
                    .is_ok()
            }),
            Box::pin(async {
                let resent = resend_request_code(&one, (&hung, &key), &she, (id, t0));
                resent.await.is_ok()
            }),
            Box::pin(async {
                confirm_request(&one, &hung, &she, (id, CODE, t0))
                    .await
                    .is_ok()
            }),
        ];
        std::future::poll_fn(|cx| {
            for step in &mut steps {
                assert!(step.as_mut().poll(cx).is_pending(), "a step ended");
            }
            match hung.reached.load(Ordering::SeqCst) {
                3 => Poll::Ready(()),
                _ => Poll::Pending,
            }
        })
        .await;
        let mut db = one
            .acquire()
            .await
            .expect("no connection is held across Verify");
        assert_eq!(actions(&mut db).await.len(), 2);
    }
}
