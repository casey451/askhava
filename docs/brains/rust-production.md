# Brain: production Rust for Ask Hava (axum, askama, sqlx, tokio)

> **Purpose:** the working rules an AI session needs to write production Rust in this repo without guessing; every rule carries its source.
> **Who reads it:** every build task that touches `app/` or `tests/` Rust (T009 onward), and the reviewer of each such PR; the worker loop, `core`, `privacy`, `payments`, `portals` and `server` tasks most of all.
> **How to use it:** read it before writing code; cite the principle number (for example "R7") in the PR body for each rule your change relies on; run the checklist in section 5 before hand-in.
> **Last verified:** 2026-10-29 (sources fetched that day; crate versions are the ones pinned in `docs/architecture.md` section 11).
> **Never overrides** `docs/spec/` or `docs/architecture.md`. If this file disagrees with either, the spec and architecture win, and you report the disagreement in your PR and `docs/for-casey-review.md`; never edit code to match this file against them.

Marks: **[inferred]** means a reasoned practice no fetched source states in these words. "Arch" is `docs/architecture.md`.

## 1. Who this draws on

- **Rust API Guidelines** (Rust library team) — the canonical checklist for newtypes, errors, Debug, private fields: https://rust-lang.github.io/api-guidelines/checklist.html
- **Alexis King, "Parse, don't validate" (2019)** — the type-driven input rule the newtypes follow: https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/
- **Luca Palmieri, *Zero To Production in Rust*** (ran "tens of Rust production services" at TrueLayer) — service structure, error handling chapter: https://www.zero2prod.com/ and https://www.lpalmieri.com/posts/error-handling-rust
- **Jon Gjengset, *Rust for Rustaceans*** (No Starch; AWS engineer, former MIT CSAIL PhD student, "Crust of Rust" streams) — API design and type-level guarantees: https://nostarch.com/blog/jon-gjengset-talks-rust
- **Tokio team, "Graceful Shutdown"** — the three parts of shutdown: https://tokio.rs/tokio/topics/shutdown
- **Alice Ryhl (Tokio maintainer), "Async: What is blocking?"** — the 10–100 µs rule: https://ryhl.io/blog/async-what-is-blocking/
- **axum docs** — infallible handlers, `State`, `serve`: https://www.rustmax.net/api/axum/error_handling/ , https://www.rustmax.net/api/axum/extract/struct.State.html , https://docs.tvix.dev/rust/axum/serve/struct.Serve.html
- **SQLx authors (sqlx-cli docs)** — offline data, `prepare --check`: https://docs.rs/crate/sqlx-cli ; `#[sqlx::test]`: https://docs.rs/sqlx/latest/sqlx/attr.test.html
- **Brandur Leach** (ex-Heroku, ex-Stripe) — Postgres queues, staged jobs, idempotency keys: https://www.brandur.org/postgres-queues , https://brandur.org/job-drain , https://brandur.org/idempotency-keys
- **Stripe API docs** — idempotent requests: https://docs.stripe.com/api/idempotent_requests
- **David Tolnay (thiserror, trybuild)** — https://docs.rs/thiserror , https://docs.rs/crate/trybuild/1.0.120/source/README.md
- **secrecy, tower-http, tracing, proptest, askama_web, jiff-sqlx docs** — listed in section 6.
- **Rust edition guide** — `set_var` is unsafe in 2024: https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html
- **Clippy lint docs** — `unwrap_used`, `allow_attributes_without_reason`: https://rust-lang.github.io/rust-clippy/rust-1.89.0/index.html#unwrap_used

## 2. Principles that bind this codebase

**R1. Parse at the edge into types that cannot hold bad values.** Phone numbers, codes, money, IDs and statuses become newtypes or enums the moment they cross a boundary (form, webhook, row, setting); inner code takes the type, never a `String`. Why: "Newtypes provide static distinctions" (C-NEWTYPE); "Arguments convey meaning through types, not `bool` or `Option`" (C-CUSTOM-TYPE); King: a parser returns a more precise type so the check is never repeated or forgotten. Serves SEC-010 (serde typed structs, `deny_unknown_fields`), SEC-033 (never trust browser values), Arch 4 rule 5.

**R2. Fields are private; constructors are the only way in.** Every struct with an invariant has private fields and a `parse`/`new` that checks it. "Structs have private fields" (C-STRUCT-PRIVATE); "Constructors are static, inherent methods" (C-CTOR). Serves SEC-030, SEC-046 (proof types only their owner can build), AI-004 (`ScrubbedText` only `privacy` builds).

**R3. One `thiserror` enum per crate; HTTP mapping happens once, in `server`.** Lower crates return their own error enum with variants a caller can act on; no crate below `server` knows a status code. Palmieri: pick an enum when "the caller [behaves] differently based on the failure mode"; keep HTTP concerns in the request handler's error type so the logic is reusable from other entry points (here, the worker). thiserror "deliberately does not appear in your public API". No `anyhow` (Arch 2, D-481). Serves SEC-029, Arch 2 (`service.rs`).

**R4. Handlers are infallible; every error becomes a response.** axum requires services with `Infallible` error type, so failures are values converted through `IntoResponse` (axum error_handling docs). The one `impl IntoResponse for AppError` in `server` decides status and the safe page; nothing else writes an error page. Serves SEC-019, WCAG error pages (`docs/spec/accessibility.md`) [inferred link].

**R5. Log an error once, where it is handled.** "Errors should be logged when they are handled"; a function that propagates with `?` does not log (Palmieri). In Ask Hava the handling point is `AppError::into_response` for web and the claim loop for tasks. Serves PRIV-016 (fewer log lines, fewer leaks), SEC-048.

**R6. Personal data and secrets cannot print.** Contact values live in `core::Redacted<T>`, whose `Debug` and `Display` print a marker; secrets are `secrecy::SecretString`, which redacts `Debug`, zeroizes on drop and needs `ExposeSecret` to read (secrecy docs). Error `Display` never interpolates a `Redacted` (Arch 2). "All public types implement `Debug`" (C-DEBUG) still holds; the marker is the Debug. Serves PRIV-016, PRIV-017, SEC-001.

**R7. Capabilities, not checks, guard the dangerous reads.** A function that must only run after proof takes a proof value whose type has private fields; the compiler, not a runtime `if`, stops other crates. `ReleaseMint` is non-`Clone`, made once in `server`, handed to `payments` (Arch 4 rule 4, D-481). A trybuild compile-fail case in a crate with no feature proves it (SEC-030, SEC-046). Gjengset's book treats such type-level guarantees as API design [inferred: no page fetched quotes this].

**R8. SQL is literal and compile-checked.** Only `query!`/`query_as!`/`query_scalar!` with literal SQL; builds run `SQLX_OFFLINE=true` on the committed `.sqlx/`; `cargo sqlx prepare --check` "exits with a nonzero exit status if the data in `.sqlx` is out of date"; test queries need `-- --all-targets --all-features` after the `--` (sqlx-cli docs). Every query names only its own schema. Serves SEC-029, Arch 4 rule 3, D-481.

**R9. Every database test gets a fresh database.** `#[sqlx::test(migrator = ...)]` "runs each test on its own database" with migrations applied (sqlx `attr.test` docs, via Exein's write-up https://blog.exein.io/sqlx_testing-blog-post-by-bogdan/). Append-only tests `SET ROLE askhava_app` so the grant is exercised (D-481). Serves D-092, D-481, test-design isolation.

**R10. Never block the runtime.** Keep "no more than 10 to 100 microseconds between each `.await`"; blocking I/O goes to `tokio::task::spawn_blocking`; heavy CPU (argon2 hashing, image re-encode) goes to `spawn_blocking` or a dedicated thread (Ryhl). Serves OPS-007 (`/healthz` must answer), PRIV-014/PRIV-025 image work.

**R11. Work that leaves the database is queued in the same transaction as the state change.** A state change and its `core.task` row commit together; never call a vendor inside the transaction that changes status. Brandur: enqueue mid-transaction and the job runs too early; enqueue after commit and a crash loses it ("Data is persisted, but the background work doesn't get done"). Here the queue is in the same database, so the insert is simply part of the transaction. Serves JOB-004, JOB-005, JOB-006, Arch 4 rule 2.

**R12. Claim with `SKIP LOCKED`, keep claim transactions short, hold a lease, re-check on run.** PostgreSQL: skipping locked rows "can be used to avoid lock contention with multiple consumers accessing a queue-like table" (select.sgml text, see postgres brain P9). Long transactions pin the vacuum horizon and degrade a hot queue (Brandur 2015; PlanetScale 2026). So: claim and commit, run the handler outside that transaction, finish with a fenced update. Handlers first re-check the current status (JOB-006). Serves JOB-006, Arch 5 task queue.

**R13. Every outside action carries an idempotency key derived from what it identifies.** Stripe "saves the status code and body of the first request made with a given idempotency key", keys up to 255 characters, "Avoid using sensitive data … as idempotency keys" (Stripe docs). Ask Hava derives keys with HMAC-SHA-256 under `ID_SECRET` (Arch 5), so replay after restore reproduces them. Brandur: commit local "atomic phases" before each foreign mutation, store the response, return it on retry. Serves SEC-018, SEC-026, OPS-016, OPS-017.

**R14. Shut down in three steps: decide, tell, wait.** Tokio: decide when (signal), tell tasks, wait for them. Tokio's guide uses `tokio_util`'s `CancellationToken`/`TaskTracker`; `tokio-util` is **not** on `kit/allowed_crates.txt`, so use tokio's own `signal`, `sync::watch` and `task::JoinSet` unless a decision adds it. `web` uses `axum::serve(...).with_graceful_shutdown(signal)`. Serves Arch 5 (drain within grace period, release leases), OPS-033.

**R15. Every request carries an ID into every log line; arguments are not logged by default.** Set `x-request-id` with tower-http `SetRequestIdLayer` *before* `TraceLayer` ("make sure to set request ids before the request reaches `TraceLayer`"), propagate it to the response. `#[instrument]` records all arguments by default with `Debug`, so use `skip_all` and name fields explicitly (tracing-attributes docs). Serves PRIV-016, SEC-048, OPS-011 (web request log).

**R16. Configuration comes in as data, not from the process environment in tests.** Edition 2024 makes `std::env::set_var` unsafe because it "can be unsound … in a multithreaded program" (edition guide), and `unsafe` is forbidden. `Settings::from_iter` takes pairs; binaries pass `std::env::vars()`, tests pass a map (Arch 5). Serves SEC-002, SEC-036, SEC-029.

**R17. Pure rules get property tests; failures are committed.** Proptest generates inputs, shrinks failures, and saves them in `proptest-regressions/`, which "should be committed" (proptest docs, https://proptest-rs.github.io/proptest/). Rules live in `rules.rs` with no I/O (Arch 2). Serves fee, quiet-hours and timer requirements (FEE-*, QH-*, JOB-003), coverage rule (`BUILD_BRIEF.md:481`).

**R18. A suppressed lint carries its reason.** Prefer `#[expect(lint, reason = "...")]` to `#[allow]`; clippy's `allow_attributes_without_reason` "checks for attributes that allow lints without a reason". Serves CLAUDE.md quality rule ("no `#[allow(...)]` without a reason"), `scripts/check_rust_rules.py`.

## 3. Patterns

### 3.1 Newtype, parse-don't-validate (R1, R2)
```rust
/// A US phone number in E.164 form (INT-012). Built only by `parse`.
#[derive(Clone, PartialEq, Eq)]
pub struct Phone(Redacted<String>);
impl Phone {
    /// # Errors
    /// `PhoneError::Invalid` when the input is not a 10-digit NANP number.
    pub fn parse(raw: &str) -> Result<Self, PhoneError> {
        let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
        let d = digits.strip_prefix('1').filter(|d| d.len() == 10).unwrap_or(digits.as_str());
        if d.len() != 10 { return Err(PhoneError::Invalid); }
        Ok(Self(Redacted::new(format!("+1{d}"))))
    }
}
```
Forms: `#[derive(Deserialize)] #[serde(deny_unknown_fields)]` with fields of these types (SEC-010).

### 3.2 thiserror per crate, one IntoResponse (R3, R4, R5)
```rust
// app/picks/src/api.rs
#[derive(Debug, thiserror::Error)]
pub enum PickError {
    #[error("pick {0} is not awaiting confirm")] WrongStatus(PickId),
    #[error("code attempts exhausted")] TooManyAttempts,
    #[error(transparent)] Db(#[from] sqlx::Error),
}
// app/server/src/error.rs: the only place statuses are chosen.
pub enum AppError { Pick(PickError), Auth, NotFound /* ... */ }
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Pick(PickError::WrongStatus(_)) => StatusCode::CONFLICT,
            AppError::Pick(PickError::TooManyAttempts) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Pick(PickError::Db(_)) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Auth => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
        };
        tracing::error!(error = %ErrorKind::from(&self), "request failed"); // kind only, no values
        (status, ErrorPage::for_status(status)).into_response()
    }
}
```
Name every variant in the `match` (no `_ =>`), so a new variant fails to build until mapped [inferred, same reasoning as SEC-031].

### 3.3 `Redacted<T>` and `SecretString` (R6)
```rust
/// Wraps a contact value so logs cannot show it (PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted<T>(T);
impl<T> Redacted<T> {
    pub fn new(v: T) -> Self { Self(v) }
    /// The only way to read the value; grep for `expose(` to audit readers.
    pub fn expose(&self) -> &T { &self.0 }
}
impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("[redacted]") }
}
impl<T> fmt::Display for Redacted<T> { /* same marker */ }
// No Serialize impl. Keys: `secrecy::SecretString`, read with `ExposeSecret::expose_secret`.
```

### 3.4 Capability type with private fields: `ReleaseMint` (R7, SEC-030)
```rust
// app/privacy/src/api.rs
static MINTED: AtomicBool = AtomicBool::new(false);
/// Proof that a pick may release contact fields. No public constructor.
pub struct ReleaseProof { pick: PickId, _p: () }
/// Turns payments' verified evidence into proofs. Not Clone, not Copy.
pub struct ReleaseMint { _p: () }
/// # Panics
/// On a second call in one process (Arch 4 rule 4).
pub fn release_mint() -> ReleaseMint {
    assert!(!MINTED.swap(true, Ordering::SeqCst), "release_mint called twice");
    ReleaseMint { _p: () }
}
impl ReleaseMint {
    pub fn prove(&self, pick: PickId) -> ReleaseProof { ReleaseProof { pick, _p: () } }
}
pub async fn read_for_release(db: &PgPool, proof: &ReleaseProof) -> Result<ReleaseFields, PrivacyError>;
```
`tests/security/ui/forge_release_proof.rs` does `ReleaseProof { pick, _p: () }` and its `.stderr` shows the private-field error. **Open issue to report, not work around:** integration tests that call `build_router()` more than once per test binary would hit the once-per-process panic; the fix belongs in the plan (for example passing the mint into `build_router`), not in removing the assert [inferred].

### 3.5 axum state and extractors
```rust
#[derive(Clone)]
pub struct AppState { db: PgPool, settings: Arc<Settings>, clock: Arc<dyn Clock> }
impl FromRef<AppState> for PgPool { fn from_ref(s: &AppState) -> PgPool { s.db.clone() } }
async fn confirm_page(State(db): State<PgPool>, session: BusinessSession, Path(link): Path<LinkToken>)
    -> Result<ConfirmPage, AppError> { /* ... */ }
```
`State` of the wrong type "results in a compile error" (axum State docs). Session and proof extractors implement `FromRequestParts` and are the only constructors of `HomeownerSessionProof`/`OwnerSessionProof` (SEC-046). GET handlers never change state (SEC-026).

### 3.6 askama_web responses
```rust
#[derive(askama::Template, askama_web::WebTemplate)]
#[template(path = "confirm.html")]
struct ConfirmPage { job_number: JobNumber, fee_label: String, csrf: CsrfToken }
```
Returning the struct gives 200 with an HTML content type (askama_web 0.16.0 docs, feature `axum-0.8`). Autoescape stays on; `|safe` and `escape = "none"` are banned by the template scan (SEC-011). Customer words live in the template, not `.rs` literals (Arch 2).

### 3.7 SQLx `query!`, offline data, `#[sqlx::test]` (R8, R9)
```rust
let row = sqlx::query!(
    r#"UPDATE jobs.job SET status = 'quotes_ready', updated_at = now()
       WHERE id = $1 AND status = 'open' RETURNING id"#, job_id.as_uuid())
    .fetch_optional(&mut *tx).await?;      // None = moved on (JOB-004)
```
`timestamptz` columns map to `jiff_sqlx::Timestamp` wrappers with `.to_jiff()` (jiff-sqlx docs); in `query!` use the type override `"run_at: jiff_sqlx::Timestamp"` [inferred: override syntax is from sqlx `query!` docs, not fetched]. After changing SQL: `cargo sqlx prepare --workspace --check -- --all-targets --all-features` (Arch 2).
```rust
#[sqlx::test(migrator = "askhava_core::MIGRATOR")]
async fn app_role_cannot_update_ledger(pool: PgPool) {
    let mut c = pool.acquire().await.unwrap();
    sqlx::query!("SET ROLE askhava_app").execute(&mut *c).await.unwrap();
    let r = sqlx::query!("UPDATE payments.ledger_entry SET amount_cents = 0").execute(&mut *c).await;
    assert!(r.is_err()); // D-092
}
```
If `query!` cannot describe `SET ROLE`, use a literal through `sqlx::raw_sql` and log that choice in `docs/decisions.md` [inferred]. `unwrap` is allowed in tests; clippy's `allow-unwrap-in-tests` defaults to false, so set it in `clippy.toml` if `unwrap_used` is denied.

### 3.8 `SKIP LOCKED` claim loop with lease and dedupe key (R11, R12, JOB-006)
```sql
-- enqueue, inside the caller's transaction (dedupe key unique; nightly keys carry the date)
INSERT INTO core.task (kind, payload, run_at, dedupe_key) VALUES ($1, $2, $3, $4)
ON CONFLICT (dedupe_key) DO NOTHING;
-- claim, its own short transaction; $4 is the injected clock's "now" (Arch 5), not SQL now()
UPDATE core.task t SET lease_owner = $1, lease_until = $4 + $2::interval, attempts = attempts + 1
WHERE t.id IN (SELECT id FROM core.task
               WHERE done_at IS NULL AND run_at <= $4
                 AND (lease_until IS NULL OR lease_until < $4)
               ORDER BY run_at LIMIT $3 FOR UPDATE SKIP LOCKED)
RETURNING t.id, t.kind, t.payload, t.attempts;
-- finish, fenced so an expired lease cannot overwrite a newer owner
UPDATE core.task SET done_at = $3 WHERE id = $1 AND lease_owner = $2;
```
Column names here are illustrative; T012's migration decides them. Using the injected clock keeps the test-control clock advance (SEC-035) meaningful [inferred].
```rust
loop {
    tokio::select! {
        _ = shutdown.changed() => break,
        _ = tokio::time::sleep(backoff.next()) => {}
    }
    let free = sem.available_permits();
    for task in claim(&db, worker_id, lease, free).await? {
        let permit = sem.clone().acquire_owned().await?;
        set.spawn(async move { let _p = permit; run_with_timeout(task).await });
    }
}
```
Handler order: load the row, re-check status, do nothing if moved on (JOB-006); renew the lease for long handlers; on error set `run_at = now() + backoff`, clear the lease.

### 3.9 Idempotency keys (R13)
```rust
let key = derived_id(&settings.id_secret, &["stripe-charge", pick.as_str(), &attempt_no.to_string()]);
stripe.create_payment_intent(&req, IdempotencyKey(key)).await?;
```
For a POST from an email link (SEC-026): a unique index on `(action_link_id, action)` plus a stored result; a repeat inserts nothing and renders the stored result. Never put a phone, email or name in a key (Stripe docs).

### 3.10 Graceful shutdown, web and worker (R14)
```rust
async fn shutdown_signal() {
    let mut term = tokio::signal::unix::signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
}
axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;   // web
// worker: on signal, tx.send(true); stop claiming; drain `set` with a deadline under the host's
// grace period; then UPDATE core.task SET lease_owner = NULL, lease_until = NULL
// WHERE lease_owner = $me AND done_at IS NULL.
```
Requires tokio's `signal` feature. The host's stop signal and grace period are **verify with the host** [inferred: SIGTERM then kill is the common container contract, not checked for Railway].

### 3.11 Tracing with request IDs and redaction (R15)
```rust
let rid = HeaderName::from_static("x-request-id");     // axum::http, not a new `http` dependency
let app = router.layer(ServiceBuilder::new()
    .layer(SetRequestIdLayer::new(rid.clone(), MakeRequestUuid::default()))
    .layer(TraceLayer::new_for_http().make_span_with(span_with_request_id_and_route_template))
    .layer(PropagateRequestIdLayer::new(rid)));
#[tracing::instrument(skip_all, fields(job = %job_id))]
async fn advance(db: &PgPool, job_id: JobId, contact: &Contact) -> Result<(), JobError> { /* ... */ }
```
Log the route template, never the raw path: link tokens sit in paths (SEC-033). Mark `Authorization` and `Cookie` sensitive with tower-http's `sensitive-headers` feature [inferred from the pinned feature list].

### 3.12 Proptest for pure rules (R17)
```rust
proptest! {
    #[test]
    fn fee_never_exceeds_cap(price in 0u64..10_000_000, monthly in any::<bool>()) {
        let fee = rules::introduction_fee(Cents(price), monthly);
        prop_assert!(fee <= Cents(19_900));          // FEE-001 tiers
    }
}
```

### 3.13 trybuild compile-fail (R7)
```rust
#[test]
fn proofs_cannot_be_forged() { trybuild::TestCases::new().compile_fail("ui/*.rs"); }
```
Each case needs a matching `.stderr`; `TRYBUILD=overwrite` rewrites them, so read the diff (trybuild README). The README warns against compile-fail tests that only pass wrong types; SEC-030 is the exception it does not cover, because the claim under test *is* "this does not compile". `.stderr` text changes with the compiler, so it is tied to `rust-toolchain.toml`.

## 4. Anti-patterns to refuse

- `unwrap()`/`expect()` in handlers, services or the claim loop; startup-only `expect` with a message is acceptable (clippy `unwrap_used` rationale). A panic in a handler is a 500 with no audit row.
- A status as `String`/`&str`, or `==` on a status enum outside `status.rs` (SEC-031).
- SQL built with `format!`, string concatenation or `query()` with a non-literal (SEC-029).
- A vendor call inside a database transaction, or a transaction held across `.await` on the network (R11, R12).
- `std::thread::sleep`, `std::fs`, argon2 or image decode on the async runtime (R10).
- `std::env::set_var` anywhere, `std::env::var` outside the binaries' `main` (R16).
- `#[allow(...)]` without `reason`, or crate-wide `#![allow]` (R18).
- `anyhow`, `Box<dyn Error>` in an `api`, or a `String` error (R3).
- Logging an error at every level it passes through; `{:?}` on a struct holding contact data (R5, R6).
- `#[derive(Debug)]` on a struct with a plain `String` phone or name field (R6).
- `#[instrument]` without `skip_all` on a function taking contact data, tokens or bodies (R15).
- A GET that changes state; an action link used up on open (SEC-026).
- A new crate without a decisions entry and an allow-list change (`tokio-util`, `http`, `anyhow`, `once_cell` all count) (CLAUDE.md quality rule).
- `pub` fields on a proof, `Clone` on `ReleaseMint`, a `pub(crate)` constructor reachable through a re-export (R7).
- Weakening a test, or regenerating `.stderr`/`.sqlx` without reading the diff (CLAUDE.md rule 6).
- `_ =>` arms on your own enums in error mapping (3.2).

## 5. Review checklist (fresh-eyes reviewer)

1. Every new input type parses once at the edge; inner functions take the parsed type (R1).
2. Structs with invariants have private fields and a checked constructor (R2).
3. Each crate's errors are its own `thiserror` enum; no status codes below `server` (R3).
4. `AppError` mapping names every variant; the page shown leaks nothing internal (R4).
5. Each error is logged once, at the handling point, without values (R5).
6. No contact value or secret can reach `Debug`, `Display`, a log field, a span or Sentry (R6, PRIV-016, PRIV-017).
7. Proof and mint types: private fields, no `Clone`, compile-fail case present and run in a crate with no feature (R7, SEC-030, SEC-046).
8. Every query is a literal macro, names only its own schema, and `.sqlx/` was regenerated with `--check` passing (R8).
9. Database tests use `#[sqlx::test(migrator = ...)]`; append-only tests `SET ROLE askhava_app` (R9).
10. No blocking call or heavy CPU on the runtime (R10).
11. State change, audit row and `core.task` insert commit in one transaction; no vendor call inside it (R11, JOB-005).
12. Status updates use "only if still in the expected status" and handle "moved on" (JOB-004).
13. Task handlers re-check status first and are safe to run twice (JOB-006, R12).
14. Claim transactions are short; the finish update is fenced by lease owner (R12).
15. Outside actions carry a derived idempotency key with no personal data (R13, SEC-018).
16. Web and worker stop cleanly on SIGTERM; worker releases leases (R14).
17. Request ID on every log line; routes logged as templates; tokens never logged (R15, SEC-033).
18. No `set_var`; settings tests use `Settings::from_iter` (R16).
19. Pure rules have property tests; `proptest-regressions/` committed (R17).
20. No `#[allow]` without reason; clippy `-D warnings` and `cargo fmt --check` clean (R18).
21. Functions under 50 lines; files open with `//!`; public items have `///` with requirement IDs (`docs/code-docs.md`).
22. GET handlers change nothing; POSTs carry CSRF and are idempotent (SEC-026, SEC-042).
23. No new crate without decision entry and allow-list change.
24. Crate `CLAUDE.md`, `docs/architecture.md` and `docs/decisions.md` updated in the same PR when behavior changed.

## 6. Sources

- Rust API Guidelines checklist: https://rust-lang.github.io/api-guidelines/checklist.html
- Alexis King, Parse, don't validate: https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/
- Luca Palmieri, Zero To Production: https://www.zero2prod.com/ ; error handling chapter: https://www.lpalmieri.com/posts/error-handling-rust
- Jon Gjengset interview (No Starch): https://nostarch.com/blog/jon-gjengset-talks-rust
- Tokio graceful shutdown: https://tokio.rs/tokio/topics/shutdown
- Alice Ryhl, Async: What is blocking?: https://ryhl.io/blog/async-what-is-blocking/
- axum error handling: https://www.rustmax.net/api/axum/error_handling/ ; State: https://www.rustmax.net/api/axum/extract/struct.State.html ; serve: https://docs.tvix.dev/rust/axum/serve/struct.Serve.html
- askama_web 0.16.0: https://docs.rs/askama_web/
- sqlx-cli (offline, prepare --check): https://docs.rs/crate/sqlx-cli ; sqlx test attribute: https://docs.rs/sqlx/latest/sqlx/attr.test.html ; Exein on `#[sqlx::test]`: https://blog.exein.io/sqlx_testing-blog-post-by-bogdan/
- jiff-sqlx: https://docs.rs/jiff-sqlx
- thiserror: https://docs.rs/thiserror ; trybuild README: https://docs.rs/crate/trybuild/1.0.120/source/README.md
- secrecy: https://docs.rs/secrecy/0.8.0/secrecy/ (0.10 keeps the same guarantees [inferred; 0.8 page fetched])
- tower-http request_id: https://docs.rs/tower-http/0.7.0/tower_http/request_id/
- tracing `#[instrument]` docs (source): https://docs.tvix.dev/rust/src/tracing_attributes/lib.rs.html
- proptest: https://docs.rs/crate/proptest/ ; book: https://proptest-rs.github.io/proptest/
- Rust 2024 newly unsafe functions: https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html
- Clippy `unwrap_used`: https://rust-lang.github.io/rust-clippy/rust-1.89.0/index.html#unwrap_used ; `allow_attributes_without_reason`: https://rust-lang.github.io/rust-clippy/rust-1.89.0/index.html#allow_attributes_without_reason
- Brandur Leach: https://www.brandur.org/postgres-queues , https://brandur.org/job-drain , https://brandur.org/idempotency-keys
- PlanetScale, Keeping a Postgres queue healthy (2026): https://www.planetscale.com/blog/keeping-a-postgres-queue-healthy
- PostgreSQL SKIP LOCKED doc text (commit that added it): https://forger.sitiv.fr/open-dsi/postgres/commit/df630b0dd5ea2de52972d456f5978a012436115e.diff
- Stripe idempotent requests: https://docs.stripe.com/api/idempotent_requests
