//! The quote page's update choice (TXT-018, CON-035, PICK-029, PRIV-001, A11Y-030, SEC-026,
//! SEC-042; T729, D-4727, D-5340, D-5442).
//!
//! Updates go by text unless she chooses otherwise; she is never asked by text (D-460). While her
//! job is live the quote page shows "Get updates by email instead" and "Get updates by text and
//! email", each a `POST` to [`UPDATES_ROUTE`] that answers with the page asking "What email
//! should we use? We never share it with businesses.". A malformed address is asked once more
//! with the A11Y-030 error (an "Error: " title, a focused summary linking the field,
//! `aria-invalid`, her value kept); a second malformed one keeps text and says so. A well-formed
//! one is sealed as her update address (`privacy::api::store_update_address`, never her
//! contact's conversation email, so no pick code can reach it, PICK-029) and
//! `messaging::api::set_update_channel` records `Email` or `Both` in the same transaction.
//!
//! Until it is confirmed the page says "We sent a confirm link to that email." and shows no
//! button. Only when the page is opened with `?confirm=<token>`, the first update email's link
//! (T730), and that token is `core::api::link_token` (`LinkKind::Action`) over her current,
//! unconfirmed address row and its salt, does it show "Confirm this email", whose `POST` to
//! [`UPDATES_CONFIRM_ROUTE`] sets `confirmed_at` once; opening the link changes nothing
//! (SEC-026), and a `POST` without a matching token sets nothing (D-5340). "Get updates by text
//! only" sets `Text` back; with a bounced address and `Text` the page says so (D-5442). Without
//! keys (`quote_page_routes`) both `POST`s pass the token rules and CSRF, still ask and check the
//! address, and store nothing.

use askhava_core::api::{IdSecret, LinkKind, LinkToken, link_token};
use askhava_intake::api::job_homeowner;
use askhava_messaging::api::{UpdateChannel, set_update_channel, update_channel};
use askhava_privacy::api::{
    UpdateAddress, UpdateStatus, Vault, confirm_update_address, store_update_address, update_status,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};

use crate::quote_page::{Asked, PageError, QuotePage, load, open_job, respond};
use crate::security::{CsrfField, CsrfForm};
use crate::session::csrf_token;

/// The update choice's route (TXT-018): the two choices, the address and "text only".
pub const UPDATES_ROUTE: &str = "/q/{token}/updates";

/// The update address's confirm route (TXT-018, D-5340).
pub const UPDATES_CONFIRM_ROUTE: &str = "/q/{token}/updates/confirm";

/// The line after a second malformed address (TXT-018).
const KEPT_TEXT: &str = "That email still doesn't look right, so your updates stay by text.";

/// The line after a confirm (TXT-018).
const CONFIRMED: &str = "Thanks. That email is confirmed, and your updates will go there.";

/// The vault that seals the address and the ID secret that makes its confirm token (SEC-037).
pub(crate) struct UpdateKeys {
    pub(crate) vault: Vault,
    pub(crate) secret: IdSecret,
}

/// The confirm token of update address `status` (D-5340): what T730 puts in the first email's
/// link, `/q/{token}?confirm=<this>`.
fn confirm_token(secret: &IdSecret, status: &UpdateStatus) -> LinkToken {
    link_token(secret, LinkKind::Action, status.row, &status.salt)
}

/// The address question as the page asks it (TXT-018, A11Y-030). No `Debug`: it holds her typed
/// address (PRIV-016).
#[derive(Default)]
pub(crate) struct Ask {
    /// The chosen channel's form value, `email` or `both`.
    pub(crate) choice: &'static str,
    /// What she typed, kept after an error (A11Y-030).
    pub(crate) value: String,
    /// The address was malformed once, so the error shows and the next miss keeps text.
    pub(crate) error: bool,
}

/// What a request adds to the update section: the question, a note, the `?confirm=` token.
#[derive(Default)]
pub(crate) struct Said {
    pub(crate) ask: Option<Ask>,
    pub(crate) note: Option<&'static str>,
    pub(crate) confirm: Option<LinkToken>,
}

/// The page's update section (TXT-018). No `Debug` (it may hold her typed address).
pub(crate) struct Updates {
    /// `/q/<token>/updates`.
    pub(crate) action: String,
    /// `/q/<token>/updates/confirm`.
    pub(crate) confirm_action: String,
    channel: UpdateChannel,
    status: Option<UpdateStatus>,
    /// A valid confirm token for her current address, carried by the confirm form (D-5340).
    pub(crate) confirm: Option<String>,
    /// The address question, when asked.
    pub(crate) ask: Option<Ask>,
    /// A line about what just happened.
    pub(crate) note: Option<&'static str>,
}

impl Updates {
    /// Updates go by email, alone or with text.
    pub(crate) fn by_email(&self) -> bool {
        self.channel != UpdateChannel::Text
    }

    /// Which channel the page names.
    pub(crate) fn channel_line(&self) -> &'static str {
        match self.channel {
            UpdateChannel::Both => "Your quote updates come by text and email.",
            UpdateChannel::Email => "Your quote updates come by email.",
            UpdateChannel::Text => "Your quote updates come by text.",
        }
    }

    /// Her address waits for its confirm link (D-5340).
    pub(crate) fn waiting(&self) -> bool {
        self.status.is_some_and(|s| !s.confirmed)
    }

    /// The address bounced and updates went back to text (D-5442).
    pub(crate) fn bounced(&self) -> bool {
        self.channel == UpdateChannel::Text && self.status.is_some_and(|s| s.bounced)
    }

    /// The address question shows its A11Y-030 error.
    pub(crate) fn error(&self) -> bool {
        self.ask.as_ref().is_some_and(|a| a.error)
    }
}

/// TXT-018, D-5340, D-5442: the update section of `job`'s page at `token`, or `None` for a job
/// no longer `live` or with no homeowner; the `?confirm=` token is kept only when it is her
/// current, unconfirmed address's.
pub(crate) async fn updates_view(
    db: &mut PgConnection,
    (keys, job, token, live): (Option<&UpdateKeys>, Uuid, &LinkToken, bool),
    said: Said,
) -> Result<Option<Updates>, PageError> {
    if !live {
        return Ok(None);
    }
    let Some(owner) = job_homeowner(db, job).await.map_err(|_| PageError::Read)? else {
        return Ok(None);
    };
    let channel = update_channel(db, owner.contact)
        .await
        .map_err(|_| PageError::Read)?;
    let status = update_status(db, owner.contact)
        .await
        .map_err(|_| PageError::Read)?;
    let valid = |t: &LinkToken| match (keys, status) {
        (Some(keys), Some(s)) => !s.confirmed && confirm_token(&keys.secret, &s) == *t,
        _ => false,
    };
    let confirm = said.confirm.filter(valid).map(|t| t.as_str().to_owned());
    Ok(Some(Updates {
        action: UPDATES_ROUTE.replace("{token}", token.as_str()),
        confirm_action: UPDATES_CONFIRM_ROUTE.replace("{token}", token.as_str()),
        channel,
        status,
        confirm,
        ask: said.ask,
        note: said.note,
    }))
}

/// D-5340: the `confirm=<token>` of the page's query, if well formed.
pub(crate) fn confirm_query(query: Option<&str>) -> Option<LinkToken> {
    let pairs = query.unwrap_or_default().split('&');
    pairs
        .filter_map(|pair| pair.strip_prefix("confirm="))
        .find_map(|raw| LinkToken::parse(raw).ok())
}

/// The posted update choice. No `Debug`: it carries her typed address (PRIV-016). Unknown fields
/// are ignored and every field may be missing, so a bare post shows the page again.
#[derive(serde::Deserialize)]
pub(crate) struct UpdatesForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    choice: String,
    email: Option<String>,
    #[serde(default)]
    tries: String,
}

impl CsrfField for UpdatesForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The posted confirm form: its CSRF token and the confirm token (D-5340).
#[derive(serde::Deserialize)]
pub(crate) struct ConfirmForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    confirm: String,
}

impl CsrfField for ConfirmForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST /q/{token}/updates` (TXT-018, SEC-042): CSRF first (`CsrfForm`), then the token (a bare
/// 404 when it opens nothing), then the choice; the answer is the page.
pub(crate) async fn post_updates(
    State(page): State<QuotePage>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<UpdatesForm>,
) -> Response {
    let Ok(token) = LinkToken::parse(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let said = answer_choice(&page, &token, &form).await;
    show_after(&page, &token, jar, said).await
}

/// `POST /q/{token}/updates/confirm` (TXT-018, SEC-026, SEC-042, D-5340): CSRF, the token, then
/// `confirmed_at` set once, only for her current address's confirm token.
pub(crate) async fn post_confirm(
    State(page): State<QuotePage>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<ConfirmForm>,
) -> Response {
    let Ok(token) = LinkToken::parse(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let said = answer_confirm(&page, &token, &form.confirm).await;
    show_after(&page, &token, jar, said).await
}

/// The page after a `POST`, with what it said; a bare 404 when the token opens nothing.
async fn show_after(
    page: &QuotePage,
    token: &LinkToken,
    jar: CookieJar,
    said: Result<Option<Said>, PageError>,
) -> Response {
    let mut asked = Asked::plain(csrf_token(&jar).unwrap_or_default());
    let shown = match said {
        Ok(Some(said)) => {
            asked.said = said;
            load(page, token, asked).await
        }
        Ok(None) => Ok(None),
        Err(err) => Err(err),
    };
    respond(jar, shown)
}

/// Her contact, while `token` opens a live job with a homeowner; `None` for a token that opens
/// nothing, and `Some(None)` when there is nothing to change.
async fn live_contact(
    db: &mut PgConnection,
    page: &QuotePage,
    token: &LinkToken,
) -> Result<Option<Option<Uuid>>, PageError> {
    let Some((job, status)) = open_job(db, page, token).await? else {
        return Ok(None);
    };
    if status.is_final() {
        return Ok(Some(None));
    }
    let owner = job_homeowner(db, job).await.map_err(|_| PageError::Read)?;
    Ok(Some(owner.map(|o| o.contact)))
}

/// TXT-018: what the choice does. `text` sets text back; `email` or `both` with no address asks,
/// with a malformed one asks again with the error, then keeps text; with a good one stores it and
/// the channel. Without keys nothing is stored.
async fn answer_choice(
    page: &QuotePage,
    token: &LinkToken,
    form: &UpdatesForm,
) -> Result<Option<Said>, PageError> {
    let mut db = page.pool.acquire().await.map_err(|_| PageError::Read)?;
    let Some(contact) = live_contact(&mut db, page, token).await? else {
        return Ok(None);
    };
    let Some(contact) = contact else {
        return Ok(Some(Said::default()));
    };
    let (channel, choice) = match form.choice.as_str() {
        "email" => (UpdateChannel::Email, "email"),
        "both" => (UpdateChannel::Both, "both"),
        "text" => {
            set_text(&mut db, page, contact).await?;
            return Ok(Some(Said::default()));
        }
        _ => return Ok(Some(Said::default())),
    };
    let Some(typed) = form.email.as_deref() else {
        let ask = Ask {
            choice,
            ..Ask::default()
        };
        return Ok(Some(Said {
            ask: Some(ask),
            ..Said::default()
        }));
    };
    let Some(address) = UpdateAddress::parse(typed) else {
        return Ok(Some(malformed(choice, typed, &form.tries)));
    };
    store(&mut db, page, (contact, channel), &address).await?;
    Ok(Some(Said::default()))
}

/// TXT-018, A11Y-030: the first malformed address is asked again with the error and her value
/// kept; a second (`tries` 1) keeps text and says so.
fn malformed(choice: &'static str, typed: &str, tries: &str) -> Said {
    if tries == "1" {
        return Said {
            note: Some(KEPT_TEXT),
            ..Said::default()
        };
    }
    let ask = Ask {
        choice,
        value: typed.to_owned(),
        error: true,
    };
    Said {
        ask: Some(ask),
        ..Said::default()
    }
}

/// TXT-018: "Get updates by text only" sets `Text` (with keys only).
async fn set_text(db: &mut PgConnection, page: &QuotePage, contact: Uuid) -> Result<(), PageError> {
    if page.keys.is_none() {
        return Ok(());
    }
    set_update_channel(db, contact, UpdateChannel::Text, page.clock.now())
        .await
        .map_err(|_| PageError::Update)
}

/// TXT-018, PRIV-001: seals `address` as her update address and sets `channel`, in one
/// transaction (with keys only).
async fn store(
    db: &mut PgConnection,
    page: &QuotePage,
    (contact, channel): (Uuid, UpdateChannel),
    address: &UpdateAddress,
) -> Result<(), PageError> {
    let Some(keys) = page.keys.as_deref() else {
        return Ok(());
    };
    let now = page.clock.now();
    let mut tx = db.begin().await.map_err(|_| PageError::Update)?;
    store_update_address(&mut tx, &keys.vault, contact, address, now)
        .await
        .map_err(|_| PageError::Update)?;
    set_update_channel(&mut tx, contact, channel, now)
        .await
        .map_err(|_| PageError::Update)?;
    tx.commit().await.map_err(|_| PageError::Update)
}

/// TXT-018, D-5340: sets `confirmed_at` once when `posted` is her current, unconfirmed
/// address's confirm token; anything else sets nothing.
async fn answer_confirm(
    page: &QuotePage,
    token: &LinkToken,
    posted: &str,
) -> Result<Option<Said>, PageError> {
    let mut db = page.pool.acquire().await.map_err(|_| PageError::Read)?;
    let Some(contact) = live_contact(&mut db, page, token).await? else {
        return Ok(None);
    };
    let (Some(contact), Some(keys), Ok(posted)) =
        (contact, page.keys.as_deref(), LinkToken::parse(posted))
    else {
        return Ok(Some(Said::default()));
    };
    let status = update_status(&mut db, contact)
        .await
        .map_err(|_| PageError::Read)?;
    let Some(status) = status.filter(|s| confirm_token(&keys.secret, s) == posted) else {
        return Ok(Some(Said::default()));
    };
    let done = confirm_update_address(&mut db, status.row, page.clock.now())
        .await
        .map_err(|_| PageError::Update)?;
    Ok(Some(Said {
        note: done.then_some(CONFIRMED),
        ..Said::default()
    }))
}
