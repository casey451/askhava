//! "Download my data" and the code-checked delete or copy request of the homeowner portal
//! (PRIV-031, D-492; T087, D-1936).
//!
//! [`download_file`] builds the file a signed-in homeowner downloads: her own requests, quotes,
//! messages and consent records still held, as JSON, keeping only the items whose contact is hers.
//! A delete or copy request is held until the Verify code sent to the number on file is typed
//! back: [`open_request`] records the request (an `ops` audit row, its received time) and texts the
//! code; [`confirm_request`] acts only on a correct code, a wrong one acting on nothing. Every
//! request is due [`ANSWER_WITHIN`] (30 days) after it was received ([`DataRequest::due_at`]).
//! The number on file comes only from a realm-checked vault read ([`OnFile`], SEC-046). Requests
//! by email or the help form are T200's (D-493). The routes wait on homeowner sign-in being served
//! (D-1934).

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_picks::api::rules::parse_code;
use askhava_privacy::api::delete_my_data;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::homeowner_portal::OnFile;

/// The download's path in the homeowner portal (not served yet, D-1934).
pub const DOWNLOAD_ROUTE: &str = "/h/my-data";

/// The delete or copy request's path (not served yet, D-1934).
pub const DATA_REQUEST_ROUTE: &str = "/h/my-data/request";

/// How long Ask Hava has to answer a delete or copy request (PRIV-031).
pub const ANSWER_WITHIN: SignedDuration = SignedDuration::from_hours(30 * 24);

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
}

/// A request as received: its kind and received time (PRIV-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRequest {
    /// Copy or delete.
    pub kind: DataRequestKind,
    /// When it was received.
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

fn entry(on_file: &OnFile, action: &'static str) -> AuditEntry {
    AuditEntry {
        actor: Actor::Homeowner(on_file.contact()),
        action,
        subject_kind: "privacy.contact",
        subject_id: on_file.contact(),
        change: Change::Action,
    }
}

/// PRIV-031: records `kind` as received at `now` and texts a Verify code to the number on file.
/// The audit row is written first, so a request is on record even if the send fails.
///
/// # Errors
/// [`RequestError::Database`] or [`RequestError::Verify`] when the code could not be sent.
pub async fn open_request<V: VerifyAdapter>(
    db: &mut PgConnection,
    (verify, key): (&V, &SecretString),
    on_file: &OnFile,
    (kind, now): (DataRequestKind, Timestamp),
) -> Result<DataRequest, RequestError> {
    let received = entry(on_file, kind.actions().0);
    audit(db, &received)
        .await
        .map_err(|_| RequestError::Database)?;
    let (contact, at) = (on_file.contact().to_string(), now.to_string());
    let idem = IdempotencyKey::derive(key, "verify.data_request", &[&contact, &at]);
    let phone = on_file.phone().expose_secret();
    verify
        .send_code(&idem, phone, CodeChannel::Sms)
        .await
        .map_err(|_| RequestError::Verify)?;
    Ok(DataRequest {
        kind,
        received_at: now,
    })
}

/// PRIV-031, PRIV-011: acts on `kind` only when `code` is the Verify code sent to the number on
/// file; a wrong code or a failed check writes nothing and deletes nothing. A delete clears her
/// vault entry and the photos of `jobs` (`privacy::api::delete_my_data`); both write a confirmed
/// audit row.
///
/// # Errors
/// [`RequestError`].
pub async fn confirm_request<V: VerifyAdapter>(
    db: &mut PgConnection,
    verify: &V,
    (on_file, jobs): (&OnFile, &[Uuid]),
    (kind, code): (DataRequestKind, &str),
) -> Result<Acted, RequestError> {
    let code = parse_code(code).ok_or(RequestError::WrongCode)?;
    let phone = on_file.phone().expose_secret();
    match verify.check_code(phone, code.digits()).await {
        Ok(true) => {}
        Ok(false) => return Err(RequestError::WrongCode),
        Err(_) => return Err(RequestError::Verify),
    }
    let acted = match kind {
        DataRequestKind::Copy => Acted::Copy,
        DataRequestKind::Delete => delete_my_data(db, on_file.contact(), jobs)
            .await
            .map(Acted::Deleted)
            .map_err(|_| RequestError::Database)?,
    };
    audit(db, &entry(on_file, kind.actions().1))
        .await
        .map_err(|_| RequestError::Database)?;
    Ok(acted)
}

/// The four parts of the download (PRIV-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Her requests.
    Requests,
    /// The quotes on her requests: business name, quote and time only, never a business's phone,
    /// email or address.
    Quotes,
    /// Her messages still held (bodies are cut at 30 days, TXT-020).
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
}

/// One held record the composition read for the download, with the contact it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// PRIV-031: the "Download my data" file for the homeowner `on_file` names, as JSON with the four
/// sections; an item of any other contact is left out.
#[must_use]
pub fn download_file(on_file: &OnFile, items: &[HeldItem]) -> String {
    let mut file = Map::new();
    for section in Section::ALL {
        let held: Vec<Value> = items
            .iter()
            .filter(|i| i.contact == on_file.contact() && i.section == section)
            .map(|i| json!({ "at": i.at.to_string(), "words": i.words }))
            .collect();
        file.insert(section.key().to_owned(), Value::Array(held));
    }
    Value::Object(file).to_string()
}

#[cfg(test)]
mod tests {
    //! TD-PRIV-15 (PRIV-031): the file holds only her records; a request waits for the right code
    //! and is due 30 days after it was received. Fake data only.

    use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
    use askhava_ops::api::export_audit;
    use jiff::Timestamp;
    use secrecy::SecretString;
    use sqlx::types::Uuid;
    use sqlx::{PgConnection, PgPool};

    use super::{
        Acted, DataRequestKind, HeldItem, RequestError, Section, confirm_request, download_file,
        open_request,
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

    /// TD-PRIV-15: received and due 30 days later; a wrong code acts on nothing, the right one
    /// deletes.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_request_waits_for_the_right_code(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        let key = SecretString::from("fake-key-for-tests".to_owned());
        let opened = (DataRequestKind::Delete, at(1_800_000_000));
        let request = open_request(&mut db, (&FakeVerify, &key), &her(), opened)
            .await
            .expect("open");
        assert_eq!(request.due_at(), at(1_800_000_000 + 30 * 86_400));
        let parts = (&her(), &[][..]);
        for wrong in ["654321", "12", "abcdef"] {
            let kind = (DataRequestKind::Delete, wrong);
            let refused = confirm_request(&mut db, &FakeVerify, parts, kind).await;
            assert_eq!(refused, Err(RequestError::WrongCode));
        }
        assert_eq!(actions(&mut db).await, ["portals.data_delete_requested"]);
        let copy = confirm_request(&mut db, &FakeVerify, parts, (DataRequestKind::Copy, CODE));
        assert_eq!(copy.await, Ok(Acted::Copy));
        let delete = (DataRequestKind::Delete, CODE);
        let done = confirm_request(&mut db, &FakeVerify, parts, delete).await;
        assert!(matches!(done, Ok(Acted::Deleted(_))));
        let last = actions(&mut db).await.pop();
        assert_eq!(last.as_deref(), Some("portals.data_delete_confirmed"));
    }
}
