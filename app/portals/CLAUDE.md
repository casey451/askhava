# askhava-portals: portals and the shared web security

## Purpose

The layer-8 crate for the homeowner, business and owner portals (`docs/architecture.md` section 3). T051 built the shared web security every browser-facing page uses: sessions and their `__Host-` cookies, CSRF tokens, the cross-site refusal, the SEC-012 headers, `no-store` for contact and photo pages, the realm proofs for vault reads and the SEC-048 security events. T052 added the homeowner's quote page (D-1233 to D-1236); T055 the pick review step and the pick result page (D-1248 to D-1252). Not here yet: sign-in for any realm (later portals tasks), action links (T148), the pick code step and the stored page pick (T187, D-1248), the emergency switch on the review step (D-1249), the quote page's close button, STOP's still-open line, card facts and names, Spanish (D-1236).

## What it owns

- Migration `0058_portals_base_security.sql`: the `portals` schema and grants; `portals.session` (SHA256 of the cookie token, realm, actor ID, created and expiry times) and `portals.security_event` (kind, route template, request ID, actor reference, time, `raised_alert`; append-only for `askhava_app`).
- `src/session.rs`: `SESSION_COOKIE`, `PRE_SESSION_COOKIE`, `PortalsError`, `Realm`, `Session` (`id`, `realm`, `homeowner_proof`, `owner_proof`), `start_session`, `current_session`, `ensure_pre_session`, `csrf_token`, `csrf_matches`.
- `src/security.rs`: `security_headers`, `no_store`, `refuse_cross_site`, `is_cross_site`, `WebSecurity`, `CsrfField`, `ButtonForm`, `CsrfForm`, `SecurityEventKind`, `SecurityEvent`, `record_security_event`, `EVENTS_PER_HOUR_ALERT`, the header values `CONTENT_SECURITY_POLICY`, `STRICT_TRANSPORT_SECURITY`, `PERMISSIONS_POLICY`.
- `src/quote_page.rs` (T052): `quote_page_routes` (`GET /q/{token}`, `QUOTE_PAGE_ROUTE`), `number_quotes`, `Numbered`, `token_expired`, `TOKEN_LIFE_AFTER_CLOSE`; crate-private `PageView` over `templates/quote_page.html`. `static/quote_page.ts`: the 30-second poll and the status line (D-1235).
- `src/pick_flow.rs` (T055): `pick_flow_routes` (`POST /q/{token}` the review step, `POST /q/{token}/pick` the final Pick, `PICK_ROUTE`), `result_page`, `PageLanguage`; crate-private `ReviewView` over `templates/pick_review.html` (review and result).
- `askama.toml`: template folders `templates/` then `../site/templates/`, so every portal page extends `base.html` (D-491).
- `sqlx.toml`: `timestamptz` as jiff timestamps. `src/api.rs`: the public surface, plus a re-export of axum-extra's `CookieJar`.

## Rules it enforces

- SEC-009, D-099: every cookie is `__Host-` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain` (`session::host_cookie`); a form carries `csrf_token` and is read through `CsrfForm`.
- SEC-042, D-480: `start_session` deletes the browser's earlier session and drops the pre-session cookie, so no pre-login cookie survives a login; `refuse_cross_site` refuses a POST, PUT, PATCH or DELETE whose `Sec-Fetch-Site` is not `same-origin`, whose `Origin` is not `https://` on the `Host` (plain `http://` is refused), or that has neither; forms without a session are bound to `__Host-askhava-pre` (`ensure_pre_session`).
- SEC-011: autoescaping on in every template; `scripts/check_rust_rules.py` bans the escapes; `security::tests` renders `<script>` through `base.html` as text.
- SEC-012, SEC-054, D-492: `security_headers` (layered outermost in `server::build_router`) sends the CSP, one-year HSTS with `includeSubDomains` and no `preload`, the Permissions-Policy, `nosniff` and `Referrer-Policy: no-referrer` on every response, replacing any a handler set; a page-specific CSP (Stripe on Confirm, a nonce) needs that layer changed first (D-946).
- SEC-013: the CSP allows the app's own origin only; templates and static files load no other host (Stripe.js on the Confirm page is the one exception, FEE-014, which that page's task adds to its own CSP).
- SEC-034: `no_store` on every page with homeowner contact details or job photos.
- SEC-046: `Session::homeowner_proof` and `owner_proof` mint a `privacy` proof only for a session of that realm; business sessions get none.
- SEC-048, D-943, D-945: `record_security_event` logs one `security event` line (kind, route template, request ID, actor reference) and stores a row; a refusal's actor reference is the signed-in session's row ID, never a phone or name. The event that makes its kind's count over the last hour at least `EVENTS_PER_HOUR_ALERT` (50) raises one High `ops` alert `portals.<kind>_burst` unless an event of that kind in the last hour already did (`raised_alert`), so a steady attack raises at most one per kind per hour. An advisory lock per kind serializes the count.
- PICK-008, D-084, D-1233: `number_quotes` numbers `quotes::api::page_quotes` by `received_at` then business ID; an unavailable quote keeps its number, shows "no longer available" and has no Pick button.
- PICK-009, PICK-012, PICK-015, D-1234: every available quote has a Pick form with "or reply <n> YES" while `JobStatus::takes_quote_edits`; `quote_page.ts` adds new quotes and writes one `role="status"` line per batch, empty at first render (A11Y-033, D-1235).
- PICK-010, SEC-037: the page reads no name, phone or address; `X-Robots-Tag: noindex, nofollow` and `no_store`; malformed, unknown, re-issued or revoked tokens are a bare 404.
- PICK-011: `token_expired` is true from exactly 30 days after the job entered a final status.
- WORD-005, CON-003, CON-028, TXT-016: the fixed page lines live in `templates/quote_page.html`; the still-open line shows while `intake::api::paused_by_homeowner` on a live job.
- PICK-013, PICK-016, PICK-017, D-1248, D-1251: the review step says what happens next (the 6-digit code first), shows "or reply <n> YES" and "Go back" to the card, and puts only the Terms line (`#terms-line`, body text, default underlined link colour) right after the final Pick button; it shows no Confirm end time, $100 cap or key-terms summary. The final Pick re-checks the link and quote and re-renders the review under "Nothing was picked" (`#pick-not-open`, focused) until T187's code step stores the pick.
- PICK-012, PICK-032, D-1251: `takes_pick` lets a pick through in Open, No quotes yet, Quotes ready and Awaiting confirm (a new pick replaces the waiting one), never once the business confirmed.
- A11Y-014, PICK-045, PICK-054: `result_page`'s `h1#pick-result` has `tabindex="-1"` and `autofocus`; it carries the next steps, the acceptance end time when `accept_by` is given, the `data:` vCard "Add Ask Hava to your contacts" (askhava.com only, no texting-number claim) and the after-Done line, never a reward.
- A11Y-030, A11Y-031: forms post with `novalidate`, keep their submit button enabled and lean on the handler's idempotency key; a refused CSRF token is a bare 403.

## Never break

- No cookie without the `__Host-` attributes, and no cookie survives a login: `tests/security/tests/cookies_csrf.rs`.
- A CSRF token works only with its own cookie; a cross-site post is refused even with a good token: `tests/security/tests/cookies_csrf.rs`.
- Every response carries the SEC-012 headers, HSTS has no `preload`: `tests/security/tests/headers.rs`.
- No third-party script, style, image, frame or font host, and the CSP names none: `tests/security/tests/no_third_party.rs`.
- Security events carry no phone, name or path, 50 of one kind in an hour raise one alert, and a steady 50 an hour raises one per hour, not one per event: `cookies_csrf.rs`.
- Quote numbers follow arrival and never move; Pick and "or reply <n> YES" from #1; no sort or filter control: `src/tests/prop_arrival_order.rs`, `tests/integration/tests/quote_page.rs`.
- Token 30-day edge, 404 for re-issued, revoked and malformed tokens, noindex, no contact, cross-site POST refused: `tests/security/tests/quote_token.rs`.
- The quote page's fixed lines, no rating or summary element, empty status region, still-open line: `tests/integration/tests/quote_page.rs`.
- Only the Terms line beside the final Pick, link underlined in the link colour, no $100 or liability words, Go back to the card, no end time on the review step (the result may show it, PICK-045): `src/tests/pick_flow.rs`, `tests/e2e/pick_flow.spec.ts`.
- `refuse_cross_site` is never layered over webhooks (their callers send no `Origin`).

## How to change

- Add a portal page: a template in `templates/` that `{% extends "base.html" %}` and fills `title` and `main`; its router gets `from_fn_with_state(security, refuse_cross_site)`; every POST handler takes `CsrfForm<YourForm>` (with `#[serde(default)] csrf`) and the page puts `csrf_token(&jar)` in a hidden `csrf` field; pages with contact details or photos add `from_fn(no_store)`.
- Sign a person in: after the realm's checks pass, `start_session(db, jar, (realm, actor_id), lifetime, now)` with that realm's lifetime (SEC-005, SEC-008); return the jar with the response.
- Log a sign-in failure, a denial or a webhook signature failure: `record_security_event` with the route template and an actor ID, never the phone or the path.
- Change the CSP or a header: `security.rs` constants, then `headers.rs` and `no_third_party.rs` (`cargo test -p askhava-security --test headers --test no_third_party`).
- Change a quote-page line: `templates/quote_page.html` (the fixed lines change only with the spec), then `cargo test -p askhava-integration --test quote_page` and `python3 scripts/check_copy.py`; status words are the `data-one`/`data-many` attributes, never in `quote_page.ts`.
- Change the pick flow: `src/pick_flow.rs` and `templates/pick_review.html` (PICK-017's two sentences, the Terms line and PICK-054's lines are spec words), then `cargo test -p askhava-portals tests::pick_flow` and `npx playwright test --config tests/e2e/playwright.config.ts tests/e2e/pick_flow.spec.ts`; the code step replaces `final_pick`'s "Nothing was picked" render and calls `result_page` with the Confirm window's end time (T187, T063).
- Add to a quote card: a field on `quotes::api::PageQuote` (`app/quotes/src/api.rs`) or the card read, then the template; check `tsc --noEmit` and `eslint` for `static/quote_page.ts`.
- Add a table or column: a new migration (`-- portals (T###)` first line), regenerate `.sqlx/` (online build with `SQLX_OFFLINE_DIR=.sqlx`).

## Tests that guard it

- `tests/security/tests/headers.rs` (TD-SEC-04): `cargo test -p askhava-security --test headers`.
- `tests/security/tests/cookies_csrf.rs` (TD-SEC-04, TD-SEC-21, TD-SEC-26): `cargo test -p askhava-security --test cookies_csrf`.
- `tests/security/tests/no_third_party.rs` (TD-SEC-04): `cargo test -p askhava-security --test no_third_party`.
- `tests/security/tests/quote_token.rs` (TD-PICK-05, TD-SEC-16): `cargo test -p askhava-security --test quote_token`.
- `tests/integration/tests/quote_page.rs` (TD-PICK-04, TD-PICK-38, TD-A11Y-02): `cargo test -p askhava-integration --test quote_page`.
- `src/tests/prop_arrival_order.rs` (TD-PICK-04, TD-PICK-05): `cargo test -p askhava-portals tests::prop_arrival_order`.
- `src/tests/pick_flow.rs` (TD-PICK-08, TD-PICK-28, TD-PICK-30, TD-A11Y-02): `cargo test -p askhava-portals tests::pick_flow`.
- `tests/e2e/pick_flow.spec.ts` (TD-PICK-08, TD-PICK-24, TD-PICK-28, TD-PICK-30, TD-A11Y-02): `npx playwright test --config tests/e2e/playwright.config.ts tests/e2e/pick_flow.spec.ts` (needs DATABASE_URL).
- `src/security.rs` `tests` (TD-SEC-04, SEC-011) and `src/session.rs` `tests` (TD-SEC-25, SEC-046): `cargo test -p askhava-portals`.
