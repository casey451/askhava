//! The homeowner portal: sign-in by phone and texted code, her own record, the texts-off switch
//! and the forms of "Download my data" (SEC-008, PRIV-012, PRIV-021, PRIV-031, SEC-046, CON-035,
//! A11Y-032; T087, D-1934 to D-1938).
//!
//! [`send_sign_in_code`] texts a Verify code only to a number on file and answers the same either
//! way; [`finish_sign_in`] starts the 30-day homeowner session only after Verify says the code is
//! right (a wrong code is an `auth_failed` event). Her record is read only through [`OnFile::read`],
//! which takes her session's `HomeownerSessionProof` and opens her own contact alone (SEC-046).
//! [`turn_texts_off`] puts her number on the one suppression list at once, logged with channel
//! `portal` and the time, and [`TEXTS_OFF_LINE`] confirms it on screen, never by text (CON-035).
//! [`homeowner_page`] renders every screen of `templates/homeowner_home.html`, which asks for no
//! SSN, driver's license or tax ID image and has no file field (PRIV-012). The Verify send caps are
//! T089's (PICK-055, D-493), so no route here is served yet (D-1934); address and language changes
//! wait on a vault update and a stored language (D-1935).

use askama::Template;
use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_picks::api::rules::parse_code;
use askhava_privacy::api::{Field, HomeownerSessionProof, Vault, find_by_phone, read_own_contact};
use axum_extra::extract::cookie::CookieJar;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::data_download::DataRequestKind;
use crate::security::{SecurityEvent, SecurityEventKind, record_security_event};
use crate::session::{Realm, start_session};

/// The portal home's path (not served yet, D-1934).
pub const HOMEOWNER_HOME_ROUTE: &str = "/h";

/// The sign-in page's path (not served yet, D-1934).
pub const HOMEOWNER_SIGNIN_ROUTE: &str = "/h/sign-in";

/// The texts-off switch's path (not served yet, D-1934).
pub const TEXTS_OFF_ROUTE: &str = "/h/texts-off";

/// A homeowner session lasts 30 days on its device (SEC-008).
pub const HOMEOWNER_SESSION_LIFE: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The on-screen confirmation of the texts-off switch (CON-035: never by text).
pub const TEXTS_OFF_LINE: &str = "Texts are off. We won't text you again unless you text START.";

/// Her contact ID and the number on file, from her own realm-checked read (SEC-046). `Debug`
/// prints no phone (PRIV-016).
#[derive(Debug)]
pub struct OnFile {
    contact: Uuid,
    phone: SecretString,
}

impl OnFile {
    /// PRIV-021, SEC-046: reads the number on file of the contact `proof` names, and nothing else.
    ///
    /// # Errors
    /// `None` when the contact has no phone; a [`askhava_privacy::api::VaultError`] when the read
    /// is refused or fails.
    pub async fn read(
        db: &mut PgConnection,
        vault: &Vault,
        proof: &HomeownerSessionProof,
    ) -> Result<Option<Self>, askhava_privacy::api::VaultError> {
        let own = read_own_contact(db, vault, proof, &[Field::Phone]).await?;
        Ok(own.phone.map(|phone| Self {
            contact: own.id,
            phone,
        }))
    }

    #[cfg(test)]
    pub(crate) fn new(contact: Uuid, phone: &str) -> Self {
        let phone = SecretString::from(phone.to_owned());
        Self { contact, phone }
    }

    /// Her vault contact ID.
    #[must_use]
    pub fn contact(&self) -> Uuid {
        self.contact
    }

    pub(crate) fn phone(&self) -> &SecretString {
        &self.phone
    }
}

/// Why a sign-in or switch did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HomeownerError {
    /// A wrong or unreadable code, or a number not on file; the page says only "try again".
    #[error("sign-in refused")]
    Refused,
    /// Verify could not send or check the code.
    #[error("the code could not be sent or checked")]
    Verify,
    /// A database or vault call failed.
    #[error("homeowner portal database call failed")]
    Database,
}

/// SEC-008: texts a Verify code to `phone` when it is a homeowner number on file; a number not on
/// file gets nothing and the same answer, so the page never says whether it is on file.
///
/// # Errors
/// [`HomeownerError::Verify`] or [`HomeownerError::Database`].
pub async fn send_sign_in_code<V: VerifyAdapter>(
    db: &mut PgConnection,
    (vault, verify, key): (&Vault, &V, &SecretString),
    phone: &str,
    now: Timestamp,
) -> Result<(), HomeownerError> {
    let Ok(Some(contact)) = find_by_phone(db, vault, phone).await else {
        return Ok(());
    };
    let ids = [contact.to_string(), now.to_string()];
    let idem = IdempotencyKey::derive(key, "verify.homeowner_signin", &[&ids[0], &ids[1]]);
    verify
        .send_code(&idem, phone, CodeChannel::Sms)
        .await
        .map(|_| ())
        .map_err(|_| HomeownerError::Verify)
}

/// SEC-008, SEC-042: starts the 30-day homeowner session on her contact only after Verify says
/// `code` is the one sent to `phone`; anything else is [`HomeownerError::Refused`], logged as an
/// `auth_failed` event with the contact's ID (never the phone) when the number is on file.
///
/// # Errors
/// [`HomeownerError`].
pub async fn finish_sign_in<V: VerifyAdapter>(
    db: &mut PgConnection,
    (vault, verify): (&Vault, &V),
    (jar, phone, code): (CookieJar, &str, &str),
    now: Timestamp,
) -> Result<CookieJar, HomeownerError> {
    let Ok(Some(contact)) = find_by_phone(db, vault, phone).await else {
        return Err(HomeownerError::Refused);
    };
    let right = match parse_code(code) {
        Some(code) => verify.check_code(phone, code.digits()).await,
        None => Ok(false),
    };
    match right {
        Ok(true) => {}
        Ok(false) => return Err(refuse(db, contact, now).await),
        Err(_) => return Err(HomeownerError::Verify),
    }
    let login = (Realm::Homeowner, contact);
    start_session(db, jar, login, HOMEOWNER_SESSION_LIFE, now)
        .await
        .map_err(|_| HomeownerError::Database)
}

async fn refuse(db: &mut PgConnection, contact: Uuid, now: Timestamp) -> HomeownerError {
    let event = SecurityEvent {
        kind: SecurityEventKind::AuthFailed,
        route: HOMEOWNER_SIGNIN_ROUTE,
        request_id: None,
        actor_ref: Some(contact),
    };
    match record_security_event(db, &event, now).await {
        Ok(_) => HomeownerError::Refused,
        Err(_) => HomeownerError::Database,
    }
}

/// PRIV-021, CON-035, TD-CON-28: puts her number on the suppression list at once, logged with
/// channel `portal` and `now`; the caller shows [`TEXTS_OFF_LINE`] and sends no text. Returns
/// whether the number was newly listed.
///
/// # Errors
/// [`HomeownerError::Database`].
pub async fn turn_texts_off(
    db: &mut PgConnection,
    vault: &Vault,
    on_file: &OnFile,
    now: Timestamp,
) -> Result<bool, HomeownerError> {
    let how = (RevokeChannel::Portal, "portal:switch");
    suppress(db, vault, on_file.phone.expose_secret(), how, now)
        .await
        .map_err(|_| HomeownerError::Database)
}

/// What the portal home shows of her own record (PRIV-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeFacts {
    /// The last four digits of the number on file.
    pub phone_last4: String,
    /// Her property address, as read from her own record.
    pub address: Option<String>,
    /// Texts come in Spanish.
    pub spanish: bool,
    /// Her number is not on the suppression list.
    pub texts_on: bool,
}

/// One screen of the homeowner portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalScreen {
    /// The phone field; `failed` after a refused code.
    SignIn {
        /// Show the error summary.
        failed: bool,
    },
    /// The code field after a send (A11Y-032).
    Code {
        /// Show the error summary.
        failed: bool,
    },
    /// The home with her record and switches.
    Home(HomeFacts),
    /// The code step of a delete or copy request (PRIV-031).
    Request {
        /// Copy or delete.
        kind: DataRequestKind,
        /// Show the error summary.
        failed: bool,
    },
}

#[derive(Template)]
#[template(path = "homeowner_home.html")]
struct HomeView<'a> {
    screen: &'a str,
    failed: bool,
    facts: Option<&'a HomeFacts>,
    delete: bool,
    texts_off_line: &'a str,
    csrf: &'a str,
}

/// PRIV-012, PRIV-021, PRIV-031, A11Y-030, A11Y-032: renders `screen` with the CSRF token `csrf`.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn homeowner_page(screen: &PortalScreen, csrf: &str) -> Result<String, askama::Error> {
    let (name, failed, facts, delete) = match screen {
        PortalScreen::SignIn { failed } => ("signin", *failed, None, false),
        PortalScreen::Code { failed } => ("code", *failed, None, false),
        PortalScreen::Home(facts) => ("home", false, Some(facts), false),
        PortalScreen::Request { kind, failed } => {
            ("request", *failed, None, *kind == DataRequestKind::Delete)
        }
    };
    let view = HomeView {
        screen: name,
        failed,
        facts,
        delete,
        texts_off_line: TEXTS_OFF_LINE,
        csrf,
    };
    view.render()
}

#[cfg(test)]
mod tests {
    //! TD-PRIV-05, TD-CON-28, TD-A11Y-02, TD-SEC-25 (homeowner sign-in): the pages, the switch
    //! and the code-gated sign-in. Fake data only.

    use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
    use askhava_privacy::api::{NewContact, Vault, store_contact};
    use axum_extra::extract::cookie::CookieJar;
    use jiff::Timestamp;
    use secrecy::SecretString;
    use sqlx::PgPool;

    use super::{
        HomeFacts, HomeownerError, OnFile, PortalScreen, finish_sign_in, homeowner_page,
        turn_texts_off,
    };
    use crate::data_download::DataRequestKind;
    use crate::session::{Realm, current_session};

    const PHONE: &str = "+19285550100";
    const TEMPLATE: &str = include_str!("../templates/homeowner_home.html");

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
            Ok(code == "123456")
        }
    }

    fn vault() -> Vault {
        let key = |k: &str| SecretString::from(format!("1:{k}"));
        let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
        let (seal, index) = ("UlJS".repeat(10) + "UlI=", "YmJi".repeat(10) + "YmI=");
        Vault::new(&key(&seal), &key(&index), &url).expect("vault")
    }

    fn facts(texts_on: bool) -> HomeFacts {
        HomeFacts {
            phone_last4: "0100".to_owned(),
            address: Some("12 Example Way".to_owned()),
            spanish: false,
            texts_on,
        }
    }

    /// TD-PRIV-05 (PRIV-012): no field for an SSN, a driver's license or a tax ID image.
    #[test]
    fn no_page_asks_for_an_ssn_license_or_tax_id() {
        let lower = TEMPLATE.to_lowercase();
        for banned in [
            "ssn",
            "social security",
            "driver",
            "license",
            "tax id",
            "type=\"file\"",
        ] {
            assert!(!lower.contains(banned), "{banned}");
        }
    }

    /// TD-A11Y-02 (A11Y-030, A11Y-032): one paste-friendly one-time-code field; a failed submit
    /// is titled "Error: " with a focused summary first.
    #[test]
    fn code_fields_take_a_pasted_code_and_errors_lead() {
        let request = PortalScreen::Request {
            kind: DataRequestKind::Delete,
            failed: true,
        };
        for screen in [PortalScreen::Code { failed: true }, request] {
            let page = homeowner_page(&screen, "t").expect("page");
            assert_eq!(page.matches("autocomplete=\"one-time-code\"").count(), 1);
            assert!(page.contains("inputmode=\"numeric\""));
            assert!(page.contains("<title>Error: ") && page.contains("role=\"alert\""));
            assert!(!page.contains("disabled") && !page.contains("maxlength"));
        }
    }

    /// TD-PRIV-05 (PRIV-021, PRIV-031): the home shows her record, the switch while texts are on
    /// and the download; with texts off it says how to turn them back on.
    #[test]
    fn the_home_shows_her_record_and_switch() {
        let on = homeowner_page(&PortalScreen::Home(facts(true)), "t").expect("page");
        assert!(on.contains("12 Example Way") && on.contains("0100"));
        assert!(on.contains("action=\"/h/texts-off\"") && on.contains("Download my data"));
        let off = homeowner_page(&PortalScreen::Home(facts(false)), "t").expect("page");
        assert!(!off.contains("action=\"/h/texts-off\"") && off.contains("START"));
    }

    /// TD-CON-28 (CON-035): the switch lists her number at once, once.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn texts_off_lists_the_number_at_once(pool: PgPool) {
        let (mut db, vault) = (pool.acquire().await.expect("db"), vault());
        let phone = Some(SecretString::from(PHONE.to_owned()));
        let new = NewContact {
            phone,
            ..NewContact::default()
        };
        let id = store_contact(&mut db, &vault, &new).await.expect("contact");
        let her = OnFile::new(id, PHONE);
        let now = Timestamp::from_second(1_800_000_000).expect("time");
        assert_eq!(turn_texts_off(&mut db, &vault, &her, now).await, Ok(true));
        assert_eq!(turn_texts_off(&mut db, &vault, &her, now).await, Ok(false));
    }

    /// SEC-008: only the right code signs her in, to her own contact in the homeowner realm.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn only_the_right_code_signs_her_in(pool: PgPool) {
        let (mut db, vault) = (pool.acquire().await.expect("db"), vault());
        let phone = Some(SecretString::from(PHONE.to_owned()));
        let new = NewContact {
            phone,
            ..NewContact::default()
        };
        store_contact(&mut db, &vault, &new).await.expect("contact");
        let now = Timestamp::from_second(1_800_000_000).expect("time");
        let parts = (&vault, &FakeVerify);
        for (phone, code) in [(PHONE, "654321"), (PHONE, "12"), ("+19285550199", "123456")] {
            let refused = finish_sign_in(&mut db, parts, (CookieJar::new(), phone, code), now);
            assert_eq!(refused.await.err(), Some(HomeownerError::Refused));
        }
        let signed = finish_sign_in(&mut db, parts, (CookieJar::new(), PHONE, "123456"), now);
        let jar = signed.await.expect("signed in");
        let session = current_session(&mut db, &jar, now).await.expect("db");
        assert_eq!(session.map(|s| s.realm()), Some(Realm::Homeowner));
    }
}
