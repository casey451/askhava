//! The request page and its web chat at `/request` (T489; CON-025, WEB-014, WEB-006, AI-026,
//! A11Y-035, SEC-014, PRIV-008; D-3144, D-3189, D-3744, D-3878, D-3882, D-4044, D-4120).
//!
//! [`chat_routes`] serves `GET` and `POST` [`REQUEST_PATH`] (the path the footer's "Get quotes"
//! link uses, never `/chat`, WEB-003). The page shows `intake::api::web_chat_consent_text`, the
//! Spanish draft (`lang="es"`, marked for review) above the English, with no agree button, then,
//! while the AI switch is on (`intake::api::ai_on`), AI-026's line, then one question per
//! server-rendered page with the chat so far above it (A11Y-035): the job, then the mobile number,
//! then the Verify code. Each form carries a fresh message ID that T103's `chat_message` keeps as
//! the PRIV-008 message ID, so a retried `POST` stores nothing twice (D-3189). The typed words are
//! scrubbed by the composition's [`Scrub`] (`privacy::api::scrub`, which `site` may not call) before
//! anything reads them. The typed number goes back only in
//! the code form's hidden field on this `no-store` page; it is never logged, stored here or put in
//! a URL (D-4044). The code step is the composition's [`ChatKeys`] (T103's `send_code` and
//! `check_code` over the vault and Verify, D-963, D-3144); without it (dev and `Ci`) the code step sends and stores nothing and
//! says codes can't be sent right now, answering 200 (D-3744). A new request SEC-014's limit
//! refuses opens nothing and the page says to try again later. The cross-site and CSRF checks are
//! `server`'s layer, which hands the page its token as [`ChatCsrf`] (D-3882). The home request
//! box (T593, D-4098) posts `job` with no `chat` key: the answer is the start screen with her
//! words in the chat box, cut to [`WORDS_MAX_CHARS`], and nothing stored (AI-021, A11Y-035).

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use askama::Template;
use askhava_core::api::Environment;
use askhava_intake::api::rules::{Settings as IntakeSettings, language_from_code};
use askhava_intake::api::{
    ChatAnswer, ChatError, ChatIn, ChatSettings, Checked, chat_message, start_chat,
    web_chat_consent_text,
};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Form, Router};
use jiff::Timestamp;
use rand::TryRng;
use rand::rngs::SysRng;
use secrecy::SecretString;
use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::PgConnection;
use sqlx::types::Uuid;
use sqlx::types::uuid::Builder;

use crate::routes::banner_shown;

/// WEB-003, D-3878: the request page and web chat.
pub const REQUEST_PATH: &str = "/request";

/// The most chat lines a page carries forward.
const MAX_SAID: usize = 20;

/// AI-021, A11Y-035, T593: the most characters the chat box takes (its `maxlength`, and the home
/// request box's); words carried from the home page are cut to it.
pub const WORDS_MAX_CHARS: usize = 1000;

/// SEC-042, D-3882: the page's CSRF token, which `server`'s chat layer puts in the request's
/// extensions; the form posts it as `csrf`. No `Debug`, so it cannot reach a log (R6).
#[derive(Clone)]
pub struct ChatCsrf(pub String);

/// AI-002, PRIV-001: the composition's scrubber (`privacy::api::scrub` with no vault name).
pub type Scrub = fn(&str) -> String;

/// D-963, D-3144, D-3744: the code step, implemented by `server` over T103's `send_code` and
/// `check_code` with the vault, Verify and `ASKHAVA_ID_SECRET`. A full number is never logged.
pub trait ChatKeys: Send + Sync + 'static {
    /// `ASKHAVA_ID_SECRET`, for the text path's outbox keys (SEC-018).
    fn secret(&self) -> &SecretString;
    /// T103's `send_code` for the session of `key`: whether a code went.
    fn send(
        &self,
        db: &mut PgConnection,
        key: &SecretString,
        number: &SecretString,
        now: Timestamp,
    ) -> impl Future<Output = Result<bool, ChatError>> + Send;
    /// T103's `check_code` for the session of `key`, the number and the typed code.
    fn check(
        &self,
        db: &mut PgConnection,
        key: &SecretString,
        typed: (&SecretString, &str),
        at: (ChatSettings, Timestamp),
    ) -> impl Future<Output = Result<Checked, ChatError>> + Send;
}

/// The one question a page asks (A11Y-035).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Message,
    Number,
    Code,
}

/// A line the page shows above the form, and whether it is an error.
type Notice = (&'static str, bool);

const NO_CODES: Notice = (
    "We can't send codes right now. Please try again later.",
    false,
);
const EMPTY: Notice = ("Please tell us about the job first, then press Send.", true);
const BAD_NUMBER: Notice = ("Please enter a US cell number, like 928-555-0100.", true);
const OPTED_OUT: Notice = (
    "That number has opted out of our texts. Text START to us from that phone, then try again.",
    false,
);
const WRONG_CODE: Notice = (
    "That code doesn't match. Check the text and try again.",
    true,
);
const TOO_MANY: Notice = ("Too many tries. Please wait a bit and try again.", false);
const TRY_LATER: Notice = (
    "We can't take new requests right now. Please try again later.",
    false,
);
const OPEN: Notice = (
    "Thanks, we've got your request. We just sent you a text.",
    false,
);
const TEXTED: Notice = ("Got it. We'll answer by text.", false);
const FAILED: Notice = ("Something went wrong on our side. Please try again.", true);

/// One line of the chat so far: ours or hers.
struct Said {
    ours: bool,
    text: String,
}

/// A posted chat form; every field may be absent (SEC-042's token was checked in `server`).
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Posted {
    csrf: String,
    chat: String,
    message: String,
    g: String,
    said: String,
    step: String,
    words: String,
    number: String,
    code: String,
    /// The home request box's words (T593); with no `chat` key the post only carries them.
    job: Option<String>,
}

/// The page's query: the INT-047 `g` code.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Link {
    g: String,
}

/// The request page (WEB-014, WEB-006, AI-026, A11Y-035).
#[derive(Template)]
#[template(path = "request.html")]
struct RequestPage {
    banner_shown: bool,
    ai_on: bool,
    consent_es: String,
    consent_en: String,
    turn: Turn,
    ask: Ask,
    notice: &'static str,
    error: bool,
    message: String,
    words: String,
    words_max: usize,
}

/// What a page carries from one turn to the next.
struct Turn {
    csrf: String,
    chat: String,
    g: String,
    said: Vec<Said>,
    number: String,
}

impl Turn {
    /// The posted turn, or `None` without a session key.
    fn posted(form: &Posted, csrf: String) -> Option<Self> {
        let chat = form.chat.as_str();
        let hex = chat.bytes().all(|b| b.is_ascii_hexdigit());
        let ok = chat.len() == 32 && hex && chat.bytes().any(|b| b != b'0');
        let said: Vec<Said> = form.said.lines().filter_map(said_line).collect();
        let skip = said.len().saturating_sub(MAX_SAID);
        ok.then(|| Self {
            csrf,
            chat: chat.to_owned(),
            g: form.g.clone(),
            said: said.into_iter().skip(skip).collect(),
            number: form.number.trim().to_owned(),
        })
    }

    /// The chat so far as the one hidden `said` field: a line each, `us ` or `you ` first.
    fn said_field(&self) -> String {
        let line = |s: &Said| format!("{} {}", if s.ours { "us" } else { "you" }, s.text);
        self.said.iter().map(line).collect::<Vec<_>>().join("\n")
    }

    fn key(&self) -> SecretString {
        SecretString::from(self.chat.clone())
    }

    fn add(&mut self, ours: bool, text: &str) {
        self.said.push(Said {
            ours,
            text: text.replace(['\r', '\n'], " "),
        });
    }
}

/// The routes' state: the pool, the AI switch's environment, the intake settings the SMS path
/// uses and the optional code-step keys.
struct Chat<C> {
    pool: PgPool,
    env: Environment,
    intake: IntakeSettings,
    scrub: Scrub,
    keys: Option<Arc<C>>,
}

impl<C> Clone for Chat<C> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            env: self.env,
            intake: self.intake,
            scrub: self.scrub,
            keys: self.keys.clone(),
        }
    }
}

/// CON-025, D-3144, D-3878: `GET` and `POST` [`REQUEST_PATH`] over `pool`, the AI switch read in
/// `env`, `intake` the settings the SMS path uses, `scrub` the scrubber and `keys` the code step
/// (none: no code goes).
pub fn chat_routes<C: ChatKeys>(
    pool: PgPool,
    (env, intake): (Environment, IntakeSettings),
    (scrub, keys): (Scrub, Option<C>),
) -> Router {
    let keys = keys.map(Arc::new);
    let chat = Chat {
        pool,
        env,
        intake,
        scrub,
        keys,
    };
    Router::new()
        .route(REQUEST_PATH, get(page::<C>).post(post::<C>))
        .with_state(chat)
}

/// One `said` line: `us <text>` or `you <text>`; anything else is dropped.
fn said_line(line: &str) -> Option<Said> {
    let (ours, text) = match line.split_once(' ')? {
        ("us", text) => (true, text),
        ("you", text) => (false, text),
        _ => return None,
    };
    let text = text.to_owned();
    Some(Said { ours, text })
}

fn token(csrf: Option<Extension<ChatCsrf>>) -> String {
    csrf.map(|Extension(ChatCsrf(t))| t).unwrap_or_default()
}

/// `GET` [`REQUEST_PATH`]: a new chat, its source the `g` query parameter (INT-047, D-4044).
async fn page<C: ChatKeys>(
    State(chat): State<Chat<C>>,
    csrf: Option<Extension<ChatCsrf>>,
    Query(link): Query<Link>,
) -> Response {
    let turn = Turn {
        csrf: token(csrf),
        chat: fresh_id().simple().to_string(),
        g: link.g,
        said: Vec::new(),
        number: String::new(),
    };
    chat.answer(turn, Ask::Message, None).await
}

/// `POST` [`REQUEST_PATH`]: one turn of the chat; a form with no session key or message ID is a
/// 400 (SEC-042's checks ran in `server`'s layer). The home request box's post (a `job` field and
/// no `chat` key) only carries her words to the start screen (T593).
async fn post<C: ChatKeys>(
    State(chat): State<Chat<C>>,
    csrf: Option<Extension<ChatCsrf>>,
    client: Option<Extension<ConnectInfo<SocketAddr>>>,
    Form(form): Form<Posted>,
) -> Response {
    if let Some(job) = form.job.as_deref().filter(|_| form.chat.is_empty()) {
        return chat.carried(token(csrf), job).await;
    }
    let turn = Turn::posted(&form, token(csrf));
    let id = Uuid::try_parse(&form.message);
    let (Some(mut turn), Ok(id)) = (turn, id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let ip = client.map(|Extension(ConnectInfo(a))| a.ip());
    let Ok(mut db) = chat.pool.acquire().await else {
        return chat.answer(turn, Ask::Message, Some(FAILED)).await;
    };
    let db = &mut *db;
    let step = match form.step.as_str() {
        "number" => chat.number(db, &mut turn).await,
        "code" => chat.code(db, &turn, &form.code, ip).await,
        _ => chat.message(db, &mut turn, id, &form.words).await,
    };
    let (ask, notice) = step.unwrap_or_else(|error| {
        tracing::warn!(%error, "web-chat turn failed");
        (Ask::Message, Some(FAILED))
    });
    chat.answer(turn, ask, notice).await
}

type Step = Result<(Ask, Option<Notice>), ChatError>;

impl<C: ChatKeys> Chat<C> {
    /// INT-002, PRIV-008: her scrubbed words as T103's chat message under message ID `id`.
    async fn message(&self, db: &mut PgConnection, turn: &mut Turn, id: Uuid, raw: &str) -> Step {
        let words = (self.scrub)(raw);
        if words.trim().is_empty() {
            return Ok((Ask::Message, Some(EMPTY)));
        }
        let (key, now) = (turn.key(), Timestamp::now());
        start_chat(
            db,
            &key,
            Some(turn.g.as_str()).filter(|g| !g.is_empty()),
            now,
        )
        .await?;
        turn.add(false, &words);
        let msg = ChatIn {
            message_id: id,
            words: &words,
            received_at: now,
        };
        // Without keys no session passes the code, so the text path never reads this secret.
        let secret = self.keys.as_ref().map(|k| k.secret().clone());
        let secret = secret.unwrap_or_else(|| SecretString::from(String::new()));
        Ok(
            match chat_message(db, &secret, &key, &msg, self.intake).await? {
                ChatAnswer::Shown(_, text) => {
                    turn.add(true, &text);
                    (Ask::Message, None)
                }
                ChatAnswer::AskNumber => (Ask::Number, None),
                ChatAnswer::Texted(_) => (Ask::Message, Some(TEXTED)),
            },
        )
    }

    /// CON-025, D-3744: the typed number gets a Verify code; without keys nothing is sent or
    /// stored.
    async fn number(&self, db: &mut PgConnection, turn: &mut Turn) -> Step {
        let Some(keys) = &self.keys else {
            return Ok((Ask::Number, Some(NO_CODES)));
        };
        let number = SecretString::from(turn.number.clone());
        match keys.send(db, &turn.key(), &number, Timestamp::now()).await {
            Ok(true) => Ok((Ask::Code, None)),
            Ok(false) => Ok((Ask::Message, None)),
            Err(ChatError::BadNumber) => Ok((Ask::Number, Some(BAD_NUMBER))),
            Err(ChatError::OptedOut) => Ok((Ask::Number, Some(OPTED_OUT))),
            Err(ChatError::CodeStep) => Ok((Ask::Number, Some(NO_CODES))),
            Err(error) => Err(error),
        }
    }

    /// CON-025, SEC-014, D-4044: the typed code for the number the code form posted back.
    async fn code(
        &self,
        db: &mut PgConnection,
        turn: &Turn,
        typed: &str,
        ip: Option<IpAddr>,
    ) -> Step {
        let Some(keys) = &self.keys else {
            return Ok((Ask::Number, Some(NO_CODES)));
        };
        let number = SecretString::from(turn.number.clone());
        let settings = ChatSettings {
            language: language_from_code("en"),
            settings: self.intake,
            ip,
        };
        let both = (&number, typed.trim());
        match keys
            .check(db, &turn.key(), both, (settings, Timestamp::now()))
            .await
        {
            Ok(checked) => Ok(after_check(&checked)),
            Err(ChatError::CodeStep) => Ok((Ask::Code, Some(NO_CODES))),
            Err(error) => Err(error),
        }
    }

    /// CON-025, AI-021, A11Y-035, T593: the start screen of a new chat with the home request
    /// box's words in the chat box, cut to [`WORDS_MAX_CHARS`], for her to send. Nothing is
    /// written, queued, sent or logged: the words are only escaped into the page.
    async fn carried(&self, csrf: String, job: &str) -> Response {
        let turn = Turn {
            csrf,
            chat: fresh_id().simple().to_string(),
            g: String::new(),
            said: Vec::new(),
            number: String::new(),
        };
        let words = job.chars().take(WORDS_MAX_CHARS).collect();
        self.render(turn, (Ask::Message, None), words).await
    }

    /// The page for `turn` asking `ask` with `notice` (WEB-014, WEB-006, AI-026, WEB-007).
    async fn answer(&self, turn: Turn, ask: Ask, notice: Option<Notice>) -> Response {
        self.render(turn, (ask, notice), String::new()).await
    }

    /// The page for `turn` asking `ask` with `notice`, `words` already in the chat box.
    async fn render(
        &self,
        mut turn: Turn,
        (ask, notice): (Ask, Option<Notice>),
        words: String,
    ) -> Response {
        if ask != Ask::Code {
            turn.number.clear();
        }
        let spanish = IntakeSettings {
            spanish_conversations_on: true,
            ..self.intake
        };
        let es = web_chat_consent_text(language_from_code("es"), spanish);
        let en = web_chat_consent_text(language_from_code("en"), self.intake);
        let (Ok(consent_es), Ok(consent_en)) = (es, en) else {
            tracing::error!("request page consent text failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let (notice, error) = notice.unwrap_or(("", false));
        let page = RequestPage {
            banner_shown: banner_shown(&self.pool).await,
            ai_on: ai_on(&self.pool, self.env).await,
            consent_es,
            consent_en,
            turn,
            ask,
            notice,
            error,
            message: fresh_id().to_string(),
            words,
            words_max: WORDS_MAX_CHARS,
        };
        match page.render() {
            Ok(html) => Html(html).into_response(),
            Err(error) => {
                tracing::error!(%error, "request page render failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// CON-025, SEC-014, D-1951: what the page asks after T103's code check. A refused new request
/// opens nothing and says to try again later.
fn after_check(checked: &Checked) -> (Ask, Option<Notice>) {
    match checked {
        Checked::Verified(..) => (Ask::Message, Some(OPEN)),
        Checked::Wrong => (Ask::Code, Some(WRONG_CODE)),
        Checked::TooManyTries => (Ask::Number, Some(TOO_MANY)),
        Checked::Refused => (Ask::Message, Some(TRY_LATER)),
    }
}

/// A random v4 UUID from the operating system's source: a chat's session key or a form's message
/// ID. Should the source fail, the nil ID makes the next post a 400 (no key) or keeps one ID.
fn fresh_id() -> Uuid {
    let mut bytes = [0_u8; 16];
    if SysRng.try_fill_bytes(&mut bytes).is_err() {
        tracing::error!("random source failed");
        return Uuid::nil();
    }
    Builder::from_random_bytes(bytes).into_uuid()
}

/// AI-026, D-492: whether the AI switch is on; a failed read hides the line, logged once.
async fn ai_on(pool: &PgPool, env: Environment) -> bool {
    let read = match pool.acquire().await {
        Ok(mut db) => askhava_intake::api::ai_on(&mut db, env).await.ok(),
        Err(_) => None,
    };
    read.unwrap_or_else(|| {
        tracing::warn!("AI switch read failed; the request page shows no AI line");
        false
    })
}

#[cfg(test)]
mod tests {
    //! T489's route checks. Fake data only.

    use askhava_core::api::Environment;
    use askhava_intake::api::rules::Settings as IntakeSettings;
    use askhava_intake::api::{ChatError, ChatSettings, Checked};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use jiff::Timestamp;
    use secrecy::SecretString;
    use sqlx::PgPool;
    use sqlx::postgres::PgConnection;
    use tower::ServiceExt;

    use super::{
        Ask, ChatKeys, NO_CODES, OPEN, Scrub, TOO_MANY, TRY_LATER, WRONG_CODE, after_check,
        chat_routes,
    };

    /// AI-026's sentence, word for word (`docs/spec/ai.md:30`).
    const AI_LINE: &str = "An AI service helps word our questions.";

    /// A code step that must never be called.
    struct NoCalls;

    impl ChatKeys for NoCalls {
        fn secret(&self) -> &SecretString {
            unreachable!("no keys are given")
        }
        async fn send(
            &self,
            _: &mut PgConnection,
            _: &SecretString,
            _: &SecretString,
            _: Timestamp,
        ) -> Result<bool, ChatError> {
            unreachable!("no code is sent without the part")
        }
        async fn check(
            &self,
            _: &mut PgConnection,
            _: &SecretString,
            _: (&SecretString, &str),
            _: (ChatSettings, Timestamp),
        ) -> Result<Checked, ChatError> {
            unreachable!("no code is checked without the part")
        }
    }

    /// A stand-in scrubber that drops digits, so the test's typed number never reaches T103.
    fn no_digits(raw: &str) -> String {
        raw.chars().filter(|c| !c.is_ascii_digit()).collect()
    }

    async fn call(pool: &PgPool, env: Environment, request: Request<Body>) -> (StatusCode, String) {
        let parts = (no_digits as Scrub, None::<NoCalls>);
        let app = chat_routes(pool.clone(), (env, IntakeSettings::default()), parts);
        let response = app.oneshot(request).await.expect("answer");
        let status = response.status();
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    fn get(env: Environment) -> (Environment, Request<Body>) {
        let request = Request::get("/request?g=lake-flyer").body(Body::empty());
        (env, request.expect("request"))
    }

    fn post(body: &str) -> Request<Body> {
        Request::post("/request")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_owned()))
            .expect("request")
    }

    /// AI-026 (TD-CON-10's off case), WEB-014, WEB-006: the AI line shows with the AI mode `Ai`
    /// (dev, no records) and not with it `KeywordOnly` (live, no records); the Spanish consent
    /// draft sits above the English, both above the chat box, with no agree button.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn ai_line_follows_the_switch_and_consent_comes_first(pool: PgPool) {
        let (env, request) = get(Environment::Dev);
        let (status, on) = call(&pool, env, request).await;
        assert_eq!(status, StatusCode::OK);
        let (env, request) = get(Environment::Live);
        let (_, off) = call(&pool, env, request).await;
        assert!(on.contains(AI_LINE) && !off.contains(AI_LINE));
        let es = on.find("<div lang=\"es\"").expect("Spanish draft");
        let en = on.find("data-fixed=\"WEB-014\"").expect("English consent");
        let chat = on.find("<form").expect("chat box");
        assert!(es < en && en < on.find(AI_LINE).expect("AI line") && en < chat);
        assert!(on.contains("name=\"g\" value=\"lake-flyer\""));
        assert_eq!(on.matches("<button").count(), 1, "no agree button");
    }

    /// CON-025, D-3744: without the part the number step sends and stores nothing (the code step,
    /// which alone puts the number in the vault and sends, is never called: [`NoCalls`] would panic)
    /// and says codes can't be sent, answering 200; a form with no session key is a 400.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn number_step_without_the_part_sends_and_stores_nothing(pool: PgPool) {
        let chat = "0123456789abcdef0123456789abcdef";
        let id = "6f1c2a1e-1b7a-4c51-9a8e-2a3d4c5b6a71";
        let first = format!("chat={chat}&message={id}&step=message&words=My+sink+leaks");
        let (status, page) = call(&pool, Environment::Dev, post(&first)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(page.contains("name=\"number\""), "asks for the number");
        assert!(
            page.contains("name=\"said\" value=\"you My sink leaks\""),
            "chat carried"
        );
        let number = format!("chat={chat}&message={id}&step=number&number=928-555-0100");
        let (status, page) = call(&pool, Environment::Dev, post(&number)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(NO_CODES.0.contains("codes right now") && page.contains("codes right now"));
        assert!(!page.contains("928-555-0100"));
        let (status, _) = call(&pool, Environment::Dev, post("step=message")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// SEC-014, CON-025: a refused new request asks again for the job with the try-later line,
    /// and each other check answer asks the right next question.
    #[test]
    fn check_answers_pick_the_next_question() {
        let refused = after_check(&Checked::Refused);
        assert!(refused.0 == Ask::Message && refused.1 == Some(TRY_LATER));
        assert!(TRY_LATER.0.contains("try again later"));
        let open = after_check(&Checked::Verified(sqlx::types::Uuid::nil(), Vec::new()));
        assert!(open.0 == Ask::Message && open.1 == Some(OPEN));
        assert!(after_check(&Checked::Wrong) == (Ask::Code, Some(WRONG_CODE)));
        assert!(after_check(&Checked::TooManyTries) == (Ask::Number, Some(TOO_MANY)));
    }
}
