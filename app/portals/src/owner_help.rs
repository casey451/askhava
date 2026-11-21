//! The owner help page and its screened reply box (OPS-036, TD-OPS-23, SEC-005, D-3883, D-3890,
//! D-4726). `GET` [`OWNER_HELP_ROUTE`] lists the open help requests with their reply-by times
//! and, for `?request=<id>`, that request's kept messages; `POST` [`OWNER_HELP_REPLY_ROUTE`]
//! sends the owner's typed reply and `POST` [`OWNER_HELP_CLOSE_ROUTE`] closes or reopens the
//! request (IDs in the form body, never the path). Every route reads `owner_principal` first.
//! A reply goes only by text, only to a homeowner asker `intake::api::may_text` allows; every
//! other asker is refused with "by email for now" and nothing is stored or queued (email
//! replies are T740's). The typed text must pass `quotes::api::final_gate` unchanged once
//! links on the brand's own domain are set aside. The form's one-time `press` ID is the
//! reply's `body_ref`, so a double submit sends once.

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, brand};
use askhava_intake::api::may_text;
use askhava_intake::api::rules::wanted_language;
use askhava_messaging::api::{Language, NewText, Relation, enqueue};
use askhava_ops::api::{
    Asker, HelpRequest, add_owner_reply, help_request, open_help_requests, set_help_closed,
};
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, OwnerSessionProof, SessionMint, keep_problem_words,
    problem_words, scrub,
};
use askhava_quotes::api::final_gate;
use axum::Router;
use axum::extract::{FromRef, Query, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum_extra::extract::CookieJar;
use jiff::{Timestamp, tz};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::owner_auth::owner_principal;
use crate::owner_step_up_page::owner_signed_out;
use crate::security::{CsrfField, CsrfForm, WebSecurity, no_store, refuse_cross_site};
use crate::session::{csrf_token, current_session};

/// The help page: the open requests and, with `?request=`, one request.
pub const OWNER_HELP_ROUTE: &str = "/owner/help";
/// The reply box's post.
pub const OWNER_HELP_REPLY_ROUTE: &str = "/owner/help/reply";
/// The close and reopen post.
pub const OWNER_HELP_CLOSE_ROUTE: &str = "/owner/help/close";
/// The outbox `template_id` of an owner reply, outside the registry (D-3883).
pub const OWNER_REPLY_TEMPLATE: &str = "owner_reply";
/// OPS-036: the longest reply the owner may type, in characters.
pub const OWNER_REPLY_MAX: usize = 300;

/// Why a reply was not sent (OPS-036, D-4726); each stores and queues nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyRefusal {
    /// Nothing typed.
    Empty,
    /// Over [`OWNER_REPLY_MAX`] characters.
    TooLong,
    /// The CARD-008 gate would change it: a phone, email, address or outside link.
    Screened,
    /// A business, a help email, or a homeowner who cannot be texted: email is T740's.
    EmailOnly,
    /// The server runs without the texting part (no `ID_SECRET`).
    NoTexting,
    /// No such request, or a form without its IDs.
    Unknown,
    /// A database failure.
    Failed,
}

impl ReplyRefusal {
    /// The line the page shows.
    #[must_use]
    pub fn line(self) -> String {
        let domain = brand().domain();
        match self {
            Self::Empty => "Type a reply first.".to_owned(),
            Self::TooLong => format!("That's over {OWNER_REPLY_MAX} characters. Shorten it."),
            Self::Screened => format!(
                "Take out the phone number, email, address or link. A {domain} link is fine."
            ),
            Self::EmailOnly => "This reply has to go by email for now.".into(),
            Self::NoTexting => "Texting isn't set up yet.".into(),
            Self::Unknown | Self::Failed => "That request wasn't found.".into(),
        }
    }
}

/// True for a link on the brand's own domain: `https://` or bare, `www.` or not, with a path
/// of letters, `/` and `-` only, so no number rides in on it (OPS-036).
fn own_link(word: &str) -> bool {
    let word = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
    let word = word.to_lowercase();
    let rest = word.strip_prefix("https://").unwrap_or(&word);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    host == brand().domain()
        && path
            .chars()
            .all(|c| c.is_ascii_alphabetic() || "/-".contains(c))
}

/// OPS-036: the typed reply, trimmed, when it is 1 to 300 characters and the CARD-008 gate
/// (`final_gate`, as PICK-007) leaves it unchanged once the brand's own links are set aside.
///
/// # Errors
///
/// [`ReplyRefusal::Empty`], [`ReplyRefusal::TooLong`] or [`ReplyRefusal::Screened`].
pub fn screen_reply(typed: &str) -> Result<&str, ReplyRefusal> {
    let text = typed.trim();
    if text.is_empty() {
        return Err(ReplyRefusal::Empty);
    }
    if text.chars().count() > OWNER_REPLY_MAX {
        return Err(ReplyRefusal::TooLong);
    }
    let rest: Vec<&str> = text.split_whitespace().filter(|w| !own_link(w)).collect();
    let rest = rest.join(" ");
    if final_gate(&rest)
        .split_whitespace()
        .eq(rest.split_whitespace())
    {
        Ok(text)
    } else {
        Err(ReplyRefusal::Screened)
    }
}

/// OPS-036, WORD-021: the text as sent, "<brand>: Reply from the <brand> owner: <text>" or its
/// Spanish opening; the brand from `core::api::brand()`, no personal name.
#[must_use]
pub fn reply_body(language: Language, text: &str) -> String {
    match language {
        Language::En => {
            let short = brand().short_name();
            format!("{short}: Reply from the {short} owner: {text}")
        }
        Language::Es => {
            let short = brand().es().short_name();
            format!("{short}: Respuesta del dueño de {short}: {text}")
        }
    }
}

/// The language of her newest kept message on `one`; English when none is kept (D-4726).
async fn language_of(
    db: &mut PgConnection,
    proof: Option<&OwnerSessionProof>,
    one: &HelpRequest,
) -> Result<Language, ReplyRefusal> {
    for (body_ref, _, _) in one.messages.iter().rev().filter(|m| !m.1) {
        if let Some(words) = kept_words(db, proof, *body_ref).await? {
            return Ok(wanted_language(&words, None));
        }
    }
    Ok(Language::En)
}

/// PRIV-006: the kept words of `body_ref`, read only under the owner's proof; `None` without
/// one (a router built with no session mint) or once the 30-day cut removed them.
async fn kept_words(
    db: &mut PgConnection,
    proof: Option<&OwnerSessionProof>,
    body_ref: Uuid,
) -> Result<Option<String>, ReplyRefusal> {
    let Some(proof) = proof else {
        return Ok(None);
    };
    let kept = problem_words(db, proof, body_ref).await;
    Ok(kept.map_err(|_| ReplyRefusal::Failed)?.map(|k| k.words))
}

/// OPS-036, D-4726: sends `typed` on `request` in one transaction: the screen, the homeowner
/// and `may_text` checks, the scrubbed words kept under `press` (the `body_ref`; a repeat
/// press ends here, sent once), `add_owner_reply`, then the text queued under quiet hours.
///
/// # Errors
///
/// The [`ReplyRefusal`]; nothing is stored or queued then.
pub async fn send_owner_reply(
    db: &mut PgConnection,
    (proof, secret): (Option<&OwnerSessionProof>, Option<&SecretString>),
    (request, press, typed): (Uuid, Uuid, &str),
    now: Timestamp,
) -> Result<(), ReplyRefusal> {
    let failed = |_| ReplyRefusal::Failed;
    let text = screen_reply(typed)?;
    let mut tx = db.begin().await.map_err(failed)?;
    let one = help_request(&mut tx, request).await;
    let one = one.map_err(|_| ReplyRefusal::Failed)?;
    let one = one.ok_or(ReplyRefusal::Unknown)?;
    let contact = one.asker_id.ok_or(ReplyRefusal::EmailOnly)?;
    if one.asker != Asker::Homeowner(contact).code() {
        return Err(ReplyRefusal::EmailOnly);
    }
    if !may_text(&mut tx, contact)
        .await
        .map_err(|_| ReplyRefusal::Failed)?
    {
        return Err(ReplyRefusal::EmailOnly);
    }
    let secret = secret.ok_or(ReplyRefusal::NoTexting)?;
    let body = reply_body(language_of(&mut tx, proof, &one).await?, text);
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let words = scrub(text, &known, &mut NameMemory::default());
    let kept = keep_problem_words(&mut tx, press, request, &words, false).await;
    if !kept.map_err(|_| ReplyRefusal::Failed)? {
        return Ok(());
    }
    let added = add_owner_reply(&mut tx, request, press, now).await;
    added.map_err(|_| ReplyRefusal::Failed)?;
    let text = NewText {
        template_id: OWNER_REPLY_TEMPLATE,
        subject: &[request, press],
        recipient: contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let queued = enqueue(&mut tx, secret, &text).await;
    queued.map_err(|_| ReplyRefusal::Failed)?;
    tx.commit().await.map_err(failed)
}

/// One message as the page shows it; `words` is `None` once the 30-day cut removed them.
struct MessageRow {
    from_owner: bool,
    at: String,
    words: Option<String>,
}

/// The one request the page shows.
struct OneView {
    id: Uuid,
    asker: String,
    due: String,
    closed: bool,
    messages: Vec<MessageRow>,
}

#[derive(Template)]
#[template(path = "help_request.html")]
struct HelpPage<'a> {
    csrf: &'a str,
    routes: [&'static str; 3],
    open: Vec<(Uuid, String)>,
    one: Option<OneView>,
    line: Option<String>,
    failed: bool,
    typed: &'a str,
    press: Uuid,
    max: usize,
}

fn arizona(at: Timestamp) -> String {
    at.to_zoned(tz::TimeZone::fixed(tz::offset(-7)))
        .strftime("%b %-d, %-I:%M %p")
        .to_string()
}

/// `request` with its kept words, read under the owner's proof (PRIV-006).
async fn one_view(
    db: &mut PgConnection,
    proof: Option<&OwnerSessionProof>,
    request: Uuid,
) -> Result<Option<OneView>, ReplyRefusal> {
    let one = help_request(db, request).await;
    let Some(one) = one.map_err(|_| ReplyRefusal::Failed)? else {
        return Ok(None);
    };
    let mut messages = Vec::new();
    for (body_ref, from_owner, at) in one.messages {
        let words = kept_words(db, proof, body_ref).await?;
        messages.push(MessageRow {
            from_owner,
            at: arizona(at),
            words,
        });
    }
    Ok(Some(OneView {
        id: one.id,
        asker: one.asker,
        due: arizona(one.due_at),
        closed: one.closed_at.is_some(),
        messages,
    }))
}

/// The page's state: the routes' pool, clock, session mint and, with texting, the `ID_SECRET`.
#[derive(Clone)]
struct Help {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    mint: Option<&'static SessionMint>,
    id_secret: Option<SecretString>,
}

impl FromRef<Help> for WebSecurity {
    fn from_ref(help: &Help) -> Self {
        help.security.clone()
    }
}

/// The page's query: the one request to show (D-3890, SEC-010).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HelpQuery {
    #[serde(default)]
    request: String,
}

/// The posted IDs and text; a missing field is empty (D-3890: no path parameter).
#[derive(serde::Deserialize)]
struct HelpForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    request: String,
    #[serde(default)]
    press: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    reopen: String,
}

impl CsrfField for HelpForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// OPS-036, SEC-005, SEC-006: the three owner help routes, each behind `owner_principal`,
/// `no-store` and the cross-site refusal; `id_secret` is the webhooks' `ID_SECRET`, without
/// which the reply box refuses with one fixed line; `mint` is the process's session mint,
/// without which (a test crate may not name it) no kept words are read or shown.
pub fn owner_help_routes(
    pool: PgPool,
    clock: Arc<dyn Clock>,
    mint: Option<&'static SessionMint>,
    id_secret: Option<SecretString>,
) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let help = Help {
        pool,
        clock,
        security: security.clone(),
        mint,
        id_secret,
    };
    Router::new()
        .route(OWNER_HELP_ROUTE, get(show))
        .route(OWNER_HELP_REPLY_ROUTE, post(reply))
        .route(OWNER_HELP_CLOSE_ROUTE, post(close))
        .with_state(help)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

fn failed() -> Box<Response> {
    tracing::warn!("owner help failed");
    Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

type Door = (
    sqlx::pool::PoolConnection<sqlx::Postgres>,
    Option<OwnerSessionProof>,
    Timestamp,
);

/// SEC-005, SEC-006, SEC-046: the live owner's connection, read proof (none without a mint)
/// and `now`, else the owner door's 401, which shows nothing.
async fn owner(help: &Help, jar: &CookieJar) -> Result<Door, Box<Response>> {
    let now = help.clock.now();
    let mut db = help.pool.acquire().await.map_err(|_| failed())?;
    let principal = owner_principal(&mut db, jar, now).await;
    if principal.map_err(|_| failed())?.is_none() {
        return Err(Box::new(owner_signed_out()));
    }
    let Some(mint) = help.mint else {
        return Ok((db, None, now));
    };
    let session = current_session(&mut db, jar, now).await;
    let proof = session.map_err(|_| failed())?;
    let proof = proof.and_then(|s| s.owner_proof(mint));
    match proof {
        Some(proof) => Ok((db, Some(proof), now)),
        None => Err(Box::new(owner_signed_out())),
    }
}

/// The page for `request` with `notice` (`Err` a refusal, a 400, its text kept in the box).
async fn page(
    (db, proof): (&mut PgConnection, Option<&OwnerSessionProof>),
    jar: &CookieJar,
    request: Option<Uuid>,
    (notice, typed): (Option<Result<&str, ReplyRefusal>>, &str),
) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return *failed();
    };
    let Ok(open) = open_help_requests(db).await else {
        return *failed();
    };
    let one = match request {
        Some(id) => one_view(db, proof, id).await,
        None => Ok(None),
    };
    let Ok(one) = one else {
        return *failed();
    };
    let refused = matches!(notice, Some(Err(_)));
    let view = HelpPage {
        csrf: &csrf,
        routes: [
            OWNER_HELP_ROUTE,
            OWNER_HELP_REPLY_ROUTE,
            OWNER_HELP_CLOSE_ROUTE,
        ],
        open: open
            .into_iter()
            .map(|(id, due)| (id, arizona(due)))
            .collect(),
        one,
        line: notice.map(|n| n.map_or_else(ReplyRefusal::line, str::to_owned)),
        failed: refused,
        typed: if refused { typed } else { "" },
        press: Uuid::new_v4(),
        max: OWNER_REPLY_MAX,
    };
    let status = if refused {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    view.render()
        .map_or_else(|_| *failed(), |html| (status, Html(html)).into_response())
}

/// `GET` [`OWNER_HELP_ROUTE`] (OPS-036, SEC-005): the open requests and `?request=`'s messages.
async fn show(
    State(help): State<Help>,
    jar: CookieJar,
    Query(query): Query<HelpQuery>,
) -> Response {
    let (mut db, proof, _) = match owner(&help, &jar).await {
        Ok(door) => door,
        Err(refused) => return *refused,
    };
    let request = query.request.parse().ok();
    page((&mut db, proof.as_ref()), &jar, request, (None, "")).await
}

/// `POST` [`OWNER_HELP_REPLY_ROUTE`] (OPS-036, D-4726): [`send_owner_reply`], then the page.
async fn reply(
    State(help): State<Help>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<HelpForm>,
) -> Response {
    let (mut db, proof, now) = match owner(&help, &jar).await {
        Ok(door) => door,
        Err(refused) => return *refused,
    };
    let request = form.request.parse().ok();
    let sent = match (request, form.press.parse()) {
        (Some(request), Ok(press)) => {
            let keys = (proof.as_ref(), help.id_secret.as_ref());
            send_owner_reply(&mut db, keys, (request, press, &form.text), now).await
        }
        _ => Err(ReplyRefusal::Unknown),
    };
    if sent == Err(ReplyRefusal::Failed) {
        return *failed();
    }
    let notice = sent.map(|()| "Reply saved. It goes out by text between 8 a.m. and 8 p.m.");
    page(
        (&mut db, proof.as_ref()),
        &jar,
        request,
        (Some(notice), &form.text),
    )
    .await
}

/// `POST` [`OWNER_HELP_CLOSE_ROUTE`] (OPS-036): closes the request, or reopens it with `reopen`.
async fn close(
    State(help): State<Help>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<HelpForm>,
) -> Response {
    let (mut db, proof, now) = match owner(&help, &jar).await {
        Ok(door) => door,
        Err(refused) => return *refused,
    };
    let Ok(request) = form.request.parse() else {
        return page(
            (&mut db, proof.as_ref()),
            &jar,
            None,
            (Some(Err(ReplyRefusal::Unknown)), ""),
        )
        .await;
    };
    let closing = form.reopen.is_empty();
    let notice = match set_help_closed(&mut db, request, (closing, now)).await {
        Ok(true) if closing => "Request closed.",
        Ok(true) => "Request reopened.",
        Ok(false) => "Already done. Nothing changed.",
        Err(_) => return *failed(),
    };
    page(
        (&mut db, proof.as_ref()),
        &jar,
        Some(request),
        (Some(Ok(notice)), ""),
    )
    .await
}
