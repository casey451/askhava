//! The emailed single-use link that confirms a delete or copy request made in the signed-in
//! portal for a number on the suppression list (PRIV-031, TXT-018, SEC-014, SEC-026, SEC-034,
//! SEC-037; T804, D-5281, D-5332, D-5339, D-5340).
//!
//! A listed number gets no Verify code, so [`send_request_link`] stores the request as received
//! (`data_download`'s link-path sibling of `open_request`), stores the link's 128-bit salt in
//! `portals.data_request_link` (migration 0217) and queues the link email to her confirmed update
//! address through `messaging::api::queue_data_request_link`; when she has none (or it is not
//! confirmed) everything rolls back and the answer is [`LinkSent::NoConfirmedAddress`]. While an
//! earlier link of the same kind of hers is live (unused, within [`DATA_LINK_LIFE`]) a repeat
//! answers as sent and stores nothing. The caller runs the send limits first (SEC-014).
//!
//! The link is [`DATA_LINK_ROUTE`]: `{object}` is the request's subject UUID and `{token}` is
//! SEC-037's `link_token` (`LinkKind::Action`) over it and the stored salt, recomputed and
//! compared as `LinkToken`'s hash equality (never stored, never logged). [`open_data_link`] (the
//! `GET`) reads only and uses nothing up (SEC-026). [`confirm_data_link`] (the `POST`) sets
//! `used_at` with one guarded `UPDATE` and acts through `data_download::act_once` in one
//! transaction, so the link, a double submit or a code act once; a repeat within the link's life
//! answers the same result and acts nothing. [`data_link_page`] renders every answer; a copy's
//! answer holds no data (D-5340): her file is served only to her session.

use askama::Template;
use askhava_core::api::{IdSecret, LinkKind, LinkSalt, LinkToken, brand, link_token};
use askhava_messaging::api::{
    DataLinkKind, Language, LinkEmail, PostalAddress, queue_data_request_link, texts_on,
};
use askhava_privacy::api::{DELETE_CONFIRMATION, Vault};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::data_download::{
    Acted, DataRequest, DataRequestKind, RequestError, act_once, open_link_request, received,
    request_subject, stored,
};
use crate::homeowner_portal::{HOMEOWNER_SIGNIN_ROUTE, OnFile};

/// The link's path, under the help prefix so it opens with no session (D-5332).
pub const DATA_LINK_ROUTE: &str = "/help/my-data/{object}/{token}";

/// How long a link works after it was sent (PRIV-031, D-5281).
pub const DATA_LINK_LIFE: SignedDuration = SignedDuration::from_mins(15);

/// The link's path up to `{object}`.
const LINK_PREFIX: &str = "/help/my-data/";

/// What [`send_request_link`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSent {
    /// The link email is queued, or an earlier live link of hers of the same kind stands.
    Sent,
    /// She has no confirmed update address: nothing was stored or sent.
    NoConfirmedAddress,
}

/// What a link's `GET` shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPage {
    /// A live link: the confirm page for this kind of request.
    Confirm(DataRequestKind),
    /// An unknown, malformed, used or expired link: the one generic page.
    Ended,
}

/// Every answer [`data_link_page`] renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkScreen {
    /// The confirm page of a live link (or the generic one), as [`open_data_link`] found it.
    Opened(LinkPage),
    /// The request was acted on (now or by an earlier press of the same link).
    Done(Acted),
}

fn database<E>(_: E) -> RequestError {
    RequestError::Database
}

/// The ID secret as SEC-037's link key; a weak one fails as a database error, never a value.
fn link_key(secret: &SecretString) -> Result<IdSecret, RequestError> {
    IdSecret::new(1, secret.clone()).map_err(database)
}

/// A link sent at or before this instant has run out at `now`.
fn started(now: Timestamp) -> Timestamp {
    now.saturating_sub(DATA_LINK_LIFE).unwrap_or(Timestamp::MIN)
}

/// CON-035, PRIV-031: whether her number is on the suppression list (texts off), so her portal
/// request is confirmed by link, not by code.
///
/// # Errors
/// [`RequestError::Database`].
pub async fn number_listed(
    db: &mut PgConnection,
    vault: &Vault,
    on_file: &OnFile,
) -> Result<bool, RequestError> {
    let on = texts_on(db, vault, on_file.phone().expose_secret()).await;
    on.map(|on| !on).map_err(database)
}

/// PRIV-031, TXT-018, D-5339, D-5340: in one transaction (under a lock on her contact, so two
/// presses store one link), answers [`LinkSent::Sent`] when a link of hers of `kind` is still
/// live; else stores the request as received, its link's salt and the link email to her confirmed
/// update address in `language`. With no confirmed address everything rolls back. Sends no
/// Verify code; the caller has run the SEC-014 send limits.
///
/// # Errors
/// [`RequestError::Database`].
pub async fn send_request_link(
    pool: &PgPool,
    (secret, postal): (&SecretString, &PostalAddress),
    on_file: &OnFile,
    (kind, language, now): (DataRequestKind, Language, Timestamp),
) -> Result<LinkSent, RequestError> {
    let ids = link_key(secret)?;
    let mut tx = pool.begin().await.map_err(database)?;
    lock_contact(&mut tx, on_file.contact()).await?;
    if live_link(&mut tx, on_file.contact(), kind, now).await? {
        return Ok(LinkSent::Sent);
    }
    let request = open_link_request(&mut tx, on_file, kind).await?;
    let link = store_link(&mut tx, &ids, request.id, now).await?;
    let what = (link_kind(kind), request_subject(request.id), link.as_str());
    let to = (on_file.contact(), language);
    match queue_data_request_link(&mut tx, (secret, postal), to, what, now).await {
        Ok(LinkEmail::Queued(_)) => {
            tx.commit().await.map_err(database)?;
            Ok(LinkSent::Sent)
        }
        Ok(LinkEmail::NoConfirmedAddress) => Ok(LinkSent::NoConfirmedAddress),
        Err(_) => Err(RequestError::Database),
    }
}

fn link_kind(kind: DataRequestKind) -> DataLinkKind {
    match kind {
        DataRequestKind::Copy => DataLinkKind::Copy,
        DataRequestKind::Delete => DataLinkKind::Delete,
    }
}

/// Holds a transaction lock on `contact`'s link sends.
async fn lock_contact(db: &mut PgConnection, contact: Uuid) -> Result<(), RequestError> {
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended('portals.data_request_link:' || $1::uuid::text, 0))) AS lock"#,
        contact,
    )
    .fetch_one(&mut *db)
    .await
    .map(|_| ())
    .map_err(database)
}

/// Whether an unused link sent within [`DATA_LINK_LIFE`] of `now` confirms a request of
/// `contact`'s of `kind`.
async fn live_link(
    db: &mut PgConnection,
    contact: Uuid,
    kind: DataRequestKind,
    now: Timestamp,
) -> Result<bool, RequestError> {
    let live = sqlx::query_scalar!(
        r#"SELECT request_id FROM portals.data_request_link
           WHERE used_at IS NULL AND sent_at > $1 ORDER BY request_id"#,
        started(now).to_sqlx(),
    )
    .fetch_all(&mut *db)
    .await
    .map_err(database)?;
    for id in live {
        if stored(db, contact, id)
            .await?
            .is_some_and(|r| r.kind == kind)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// SEC-037: stores a fresh salt for request `request_id` sent at `now` and returns the link.
async fn store_link(
    db: &mut PgConnection,
    ids: &IdSecret,
    request_id: i64,
    now: Timestamp,
) -> Result<String, RequestError> {
    let salt = LinkSalt::generate().map_err(database)?;
    sqlx::query!(
        r#"INSERT INTO portals.data_request_link (request_id, salt, sent_at) VALUES ($1, $2, $3)"#,
        request_id,
        salt.as_bytes().as_slice(),
        now.to_sqlx(),
    )
    .execute(&mut *db)
    .await
    .map_err(database)?;
    let object = request_subject(request_id);
    let token = link_token(ids, LinkKind::Action, object, &salt);
    let domain = brand().domain();
    Ok(format!(
        "https://{domain}{LINK_PREFIX}{object}/{}",
        token.as_str()
    ))
}

/// A stored link whose token matched, with its request and the homeowner who made it.
struct Link {
    request: DataRequest,
    contact: Uuid,
    sent_at: Timestamp,
    used: bool,
}

/// SEC-037: the stored link `(object, token)` names, when both parse and the token recomputed
/// from the stored salt matches (compared as hashes); `None` for anything else.
async fn found(
    db: &mut PgConnection,
    ids: &IdSecret,
    (object, token): (&str, &str),
) -> Result<Option<Link>, RequestError> {
    let (Ok(object), Ok(token)) = (Uuid::parse_str(object), LinkToken::parse(token)) else {
        return Ok(None);
    };
    let Ok(request_id) = i64::try_from(object.as_u128()) else {
        return Ok(None);
    };
    let row = sqlx::query!(
        r#"SELECT salt, sent_at, used_at FROM portals.data_request_link WHERE request_id = $1"#,
        request_id,
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let salt = LinkSalt::from_bytes(&row.salt).map_err(database)?;
    if link_token(ids, LinkKind::Action, object, &salt) != token {
        return Ok(None);
    }
    let made = received(db, request_id).await?;
    Ok(made.map(|(request, contact)| Link {
        request,
        contact,
        sent_at: row.sent_at.to_jiff(),
        used: row.used_at.is_some(),
    }))
}

/// SEC-026, PRIV-031: what the link's `GET` shows at `now`; it reads only, so opening the link
/// (a mail scanner or a preview) changes nothing and uses nothing up.
///
/// # Errors
/// [`RequestError::Database`].
pub async fn open_data_link(
    pool: &PgPool,
    secret: &SecretString,
    path: (&str, &str),
    now: Timestamp,
) -> Result<LinkPage, RequestError> {
    let ids = link_key(secret)?;
    let mut db = pool.acquire().await.map_err(database)?;
    let link = found(&mut db, &ids, path).await?;
    let live = link.filter(|l| !l.used && l.sent_at > started(now));
    Ok(live.map_or(LinkPage::Ended, |l| LinkPage::Confirm(l.request.kind)))
}

/// SEC-026, PRIV-031, PRIV-011, R11: the link's `POST` (after the CSRF check): in one
/// transaction, uses a live link with one guarded `UPDATE` and acts on its request
/// (`act_once`); a link this press or an earlier one used within its life answers the same
/// result and acts nothing again. `None` for an unknown, malformed or expired link.
///
/// # Errors
/// [`RequestError::Database`].
pub async fn confirm_data_link(
    pool: &PgPool,
    secret: &SecretString,
    path: (&str, &str),
    now: Timestamp,
) -> Result<Option<Acted>, RequestError> {
    let ids = link_key(secret)?;
    let mut tx = pool.begin().await.map_err(database)?;
    let Some(link) = found(&mut tx, &ids, path).await? else {
        return Ok(None);
    };
    if link.sent_at <= started(now) {
        return Ok(None);
    }
    let used = sqlx::query!(
        r#"UPDATE portals.data_request_link SET used_at = $2
           WHERE request_id = $1 AND used_at IS NULL AND sent_at > $3"#,
        link.request.id,
        now.to_sqlx(),
        started(now).to_sqlx(),
    )
    .execute(&mut *tx)
    .await
    .map_err(database)?;
    let same = same_result(link.request.kind);
    if used.rows_affected() != 1 {
        return Ok(Some(same));
    }
    let acted = act_once(&mut tx, link.contact, link.request).await?;
    tx.commit().await.map_err(database)?;
    Ok(Some(acted.unwrap_or(same)))
}

/// The answer a request of `kind` gave when it was acted on.
fn same_result(kind: DataRequestKind) -> Acted {
    match kind {
        DataRequestKind::Copy => Acted::Copy,
        DataRequestKind::Delete => Acted::Deleted(DELETE_CONFIRMATION),
    }
}

/// What the page shows for one [`LinkScreen`].
struct View {
    title: &'static str,
    lines: Vec<&'static str>,
    button: bool,
    button_label: &'static str,
    sign_in: bool,
}

#[derive(Template)]
#[template(path = "data_request_link.html")]
struct LinkPageView<'a> {
    csrf: &'a str,
    sign_in: &'static str,
    view: View,
}

/// The confirm page's words for `kind`.
fn confirm_view(kind: DataRequestKind) -> View {
    let (title, lines, button_label) = match kind {
        DataRequestKind::Copy => (
            "Confirm your copy request",
            vec![
                "You asked for a copy of your data.",
                "Press the button to confirm. Then sign in to download your file.",
            ],
            "Get my copy",
        ),
        DataRequestKind::Delete => (
            "Confirm your delete request",
            vec![
                "You asked us to delete your data.",
                "Press the button to confirm. This can't be undone.",
            ],
            "Delete my data",
        ),
    };
    View {
        title,
        lines,
        button: true,
        button_label,
        sign_in: false,
    }
}

fn view_of(screen: LinkScreen) -> View {
    let (title, lines, sign_in) = match screen {
        LinkScreen::Opened(LinkPage::Confirm(kind)) => return confirm_view(kind),
        LinkScreen::Opened(LinkPage::Ended) => (
            "This link no longer works",
            vec![
                "This link has expired or was already used.",
                "Sign in to make your request again.",
            ],
            true,
        ),
        LinkScreen::Done(Acted::Copy) => (
            "Copy request confirmed",
            vec!["Your copy is ready. Sign in to download it."],
            true,
        ),
        LinkScreen::Done(Acted::Deleted(sentence)) => {
            ("Your data is deleted", vec![sentence], false)
        }
    };
    View {
        title,
        lines,
        button: false,
        button_label: "",
        sign_in,
    }
}

/// PRIV-031, SEC-026, SEC-009: the link page for `screen`; the confirm page's one button posts
/// `csrf` to the link's own path; the generic and copy pages link to homeowner sign-in.
///
/// # Errors
/// The template's render error.
pub fn data_link_page(screen: LinkScreen, csrf: &str) -> Result<String, askama::Error> {
    let view = view_of(screen);
    LinkPageView {
        csrf,
        sign_in: HOMEOWNER_SIGNIN_ROUTE,
        view,
    }
    .render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generic_page_says_the_link_ended_and_links_to_sign_in() {
        let page = data_link_page(LinkScreen::Opened(LinkPage::Ended), "t").expect("render");
        assert!(page.contains("This link has expired or was already used."));
        assert!(page.contains(HOMEOWNER_SIGNIN_ROUTE));
        assert!(!page.contains("<form"));
    }

    #[test]
    fn the_confirm_page_posts_the_csrf_token() {
        let screen = LinkScreen::Opened(LinkPage::Confirm(DataRequestKind::Delete));
        let page = data_link_page(screen, "tok123").expect("render");
        assert!(page.contains("name=\"csrf\" value=\"tok123\""));
        assert!(page.contains("method=\"post\""));
    }

    #[test]
    fn a_copy_answer_holds_no_data_only_sign_in() {
        let page = data_link_page(LinkScreen::Done(Acted::Copy), "t").expect("render");
        assert!(page.contains("Your copy is ready. Sign in to download it."));
        assert!(page.contains(HOMEOWNER_SIGNIN_ROUTE));
    }

    #[test]
    fn a_repeat_gives_the_same_result() {
        assert_eq!(same_result(DataRequestKind::Copy), Acted::Copy);
        let deleted = Acted::Deleted(DELETE_CONFIRMATION);
        assert_eq!(same_result(DataRequestKind::Delete), deleted);
    }
}
