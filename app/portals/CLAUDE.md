# askhava-portals: portals and the shared web security

## Purpose

The layer-8 crate for the homeowner, business and owner portals (`docs/architecture.md` section 3). T051 built the shared web security every browser-facing page uses: sessions and their `__Host-` cookies, CSRF tokens, the cross-site refusal, the SEC-012 headers, `no-store` for contact and photo pages, the realm proofs for vault reads and the SEC-048 security events. Not here yet: sign-in for any realm (later portals tasks), the quote page (T052), action links (T148), any page template.

## What it owns

- Migration `0058_portals_base_security.sql`: the `portals` schema and grants; `portals.session` (SHA256 of the cookie token, realm, actor ID, created and expiry times) and `portals.security_event` (kind, route template, request ID, actor reference, time; append-only for `askhava_app`).
- `src/session.rs`: `SESSION_COOKIE`, `PRE_SESSION_COOKIE`, `PortalsError`, `Realm`, `Session` (`id`, `realm`, `homeowner_proof`, `owner_proof`), `start_session`, `current_session`, `ensure_pre_session`, `csrf_token`, `csrf_matches`.
- `src/security.rs`: `security_headers`, `no_store`, `refuse_cross_site`, `is_cross_site`, `WebSecurity`, `CsrfField`, `ButtonForm`, `CsrfForm`, `SecurityEventKind`, `SecurityEvent`, `record_security_event`, `EVENTS_PER_HOUR_ALERT`, the header values `CONTENT_SECURITY_POLICY`, `STRICT_TRANSPORT_SECURITY`, `PERMISSIONS_POLICY`.
- `askama.toml`: template folders `templates/` then `../site/templates/`, so every portal page extends `base.html` (D-491).
- `sqlx.toml`: `timestamptz` as jiff timestamps. `src/api.rs`: the public surface, plus a re-export of axum-extra's `CookieJar`.

## Rules it enforces

- SEC-009, D-099: every cookie is `__Host-` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain` (`session::host_cookie`); a form carries `csrf_token` and is read through `CsrfForm`.
- SEC-042, D-480: `start_session` deletes the browser's earlier session and drops the pre-session cookie, so no pre-login cookie survives a login; `refuse_cross_site` refuses a POST, PUT, PATCH or DELETE whose `Sec-Fetch-Site` is not `same-origin`, whose `Origin` host is not the `Host`, or that has neither; forms without a session are bound to `__Host-askhava-pre` (`ensure_pre_session`).
- SEC-011: autoescaping on in every template; `scripts/check_rust_rules.py` bans the escapes; `security::tests` renders `<script>` through `base.html` as text.
- SEC-012, SEC-054, D-492: `security_headers` (layered outermost in `server::build_router`) sends the CSP, one-year HSTS with `includeSubDomains` and no `preload`, the Permissions-Policy, `nosniff` and `Referrer-Policy: no-referrer` on every response, replacing any a handler set.
- SEC-013: the CSP allows the app's own origin only; templates and static files load no other host (Stripe.js on the Confirm page is the one exception, FEE-014, which that page's task adds to its own CSP).
- SEC-034: `no_store` on every page with homeowner contact details or job photos.
- SEC-046: `Session::homeowner_proof` and `owner_proof` mint a `privacy` proof only for a session of that realm; business sessions get none.
- SEC-048, D-943: `record_security_event` logs one `security event` line (kind, route template, request ID, actor reference) and stores a row; the event that makes its kind's count over the last hour `EVENTS_PER_HOUR_ALERT` (50) raises one High `ops` alert `portals.<kind>_burst`. An advisory lock per kind serializes the count.
- A11Y-030, A11Y-031: forms post with `novalidate`, keep their submit button enabled and lean on the handler's idempotency key; a refused CSRF token is a bare 403.

## Never break

- No cookie without the `__Host-` attributes, and no cookie survives a login: `tests/security/tests/cookies_csrf.rs`.
- A CSRF token works only with its own cookie; a cross-site post is refused even with a good token: `tests/security/tests/cookies_csrf.rs`.
- Every response carries the SEC-012 headers, HSTS has no `preload`: `tests/security/tests/headers.rs`.
- No third-party script, style, image, frame or font host, and the CSP names none: `tests/security/tests/no_third_party.rs`.
- Security events carry no phone, name or path, and 50 of one kind in an hour raise one alert: `cookies_csrf.rs`.
- `refuse_cross_site` is never layered over webhooks (their callers send no `Origin`).

## How to change

- Add a portal page: a template in `templates/` that `{% extends "base.html" %}` and fills `title` and `main`; its router gets `from_fn_with_state(security, refuse_cross_site)`; every POST handler takes `CsrfForm<YourForm>` (with `#[serde(default)] csrf`) and the page puts `csrf_token(&jar)` in a hidden `csrf` field; pages with contact details or photos add `from_fn(no_store)`.
- Sign a person in: after the realm's checks pass, `start_session(db, jar, (realm, actor_id), lifetime, now)` with that realm's lifetime (SEC-005, SEC-008); return the jar with the response.
- Log a sign-in failure, a denial or a webhook signature failure: `record_security_event` with the route template and an actor ID, never the phone or the path.
- Change the CSP or a header: `security.rs` constants, then `headers.rs` and `no_third_party.rs` (`cargo test -p askhava-security --test headers --test no_third_party`).
- Add a table or column: a new migration (`-- portals (T###)` first line), regenerate `.sqlx/` (online build with `SQLX_OFFLINE_DIR=.sqlx`).

## Tests that guard it

- `tests/security/tests/headers.rs` (TD-SEC-04): `cargo test -p askhava-security --test headers`.
- `tests/security/tests/cookies_csrf.rs` (TD-SEC-04, TD-SEC-21, TD-SEC-26): `cargo test -p askhava-security --test cookies_csrf`.
- `tests/security/tests/no_third_party.rs` (TD-SEC-04): `cargo test -p askhava-security --test no_third_party`.
- `src/security.rs` `tests` (TD-SEC-04, SEC-011) and `src/session.rs` `tests` (TD-SEC-25, SEC-046): `cargo test -p askhava-portals`.
