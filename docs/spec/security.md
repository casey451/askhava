# Spec: Security (OWASP ASVS Level 2)

Prefix: `SEC` · Modules: all · Format and checks: [index.md](index.md)

- **SEC-001** No key or secret appears in code, logs, fixtures, screenshots, PR text or plain environment variables, and gitleaks fails the build on any hit. Source: `BUILD_BRIEF.md:62`, `BUILD_BRIEF.md:297`, `BUILD_BRIEF.md:298`.
- **SEC-002** All configuration comes from environment variables validated at startup, and the app refuses to start if a live key appears in a test environment or a test key in live. Source: `BUILD_BRIEF.md:521`.
- **SEC-003** Test and live run as separate projects with separate keys, databases, webhook endpoints and phone numbers, and the live project has auto-deploy off. Source: `BUILD_BRIEF.md:299`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:305`.
- **SEC-004** Business portal sign-in is per person (the owner and each team member) and uses a single-use link emailed to that person's address that expires in 15 minutes, plus a one-time code on a new device sent by SMS to that person's phone (or the owner's phone for a member with none), with 30-day sessions, and removing a team member ends that member's sessions and unlinks every phone that member linked (D-014, D-191, D-260). Source: Casey Oct 28, 2026: "do whatever research you need to to make the most ideal changes" on the three-angle fix plan (`docs/decisions.md:294`, `docs/decisions.md:348`), Casey Oct 27, 2026: "Make all the changes you recommend" on the business-side fix plan (`docs/decisions.md:272`), `BUILD_BRIEF.md:315`.
- **SEC-005** Owner portal sign-in uses an argon2id password plus a passkey or TOTP, with sessions of at most 12 hours (D-014). Source: `BUILD_BRIEF.md:316`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:375`.
- **SEC-006** The owner portal is the only portal with pause, refund and release powers, a release there only completes or re-sends one for a pick already confirmed and paid or a recorded free win, and every owner action is audit-logged (D-074). Source: `BUILD_BRIEF.md:316`.
- **SEC-007** Owner recovery needs at least two registered passkeys or authenticators plus offline one-time recovery codes, and there is no email-only recovery. Source: `BUILD_BRIEF.md:317`.
- **SEC-008** Homeowner portal sign-in uses the phone number plus a texted 6-digit code, with 30-day sessions per device (D-014). Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:373`.
- **SEC-009** Cookies are `Secure`, `HttpOnly` and `SameSite=Lax` and rotate on login, and every form carries a CSRF token. Source: `BUILD_BRIEF.md:318`.
- **SEC-010** Every input is validated by a pydantic schema. Source: `BUILD_BRIEF.md:321`.
- **SEC-011** Jinja autoescaping is always on, and homeowner and business text is always rendered as text, never as HTML. Source: `BUILD_BRIEF.md:322`.
- **SEC-012** Responses send a CSP with no inline script (nonces where needed), HSTS, `frame-ancestors 'none'`, a locked-down `Permissions-Policy` and `X-Content-Type-Options: nosniff`, and token pages send `Referrer-Policy: no-referrer`. Source: `BUILD_BRIEF.md:323`, `BUILD_BRIEF.md:324`, `BUILD_BRIEF.md:325`, `BUILD_BRIEF.md:326`, `BUILD_BRIEF.md:327`, `BUILD_BRIEF.md:328`, `BUILD_BRIEF.md:329`.
- **SEC-013** Pages load no third-party trackers or ad scripts, and fonts are self-hosted. Source: `BUILD_BRIEF.md:330`.
- **SEC-014** Rate limits apply per phone, per IP and per business to request intake, code sends, sign-in links and quote submissions, including 5 new requests per phone per hour and 50 sign-ups a day. Source: `BUILD_BRIEF.md:331`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:426`.
- **SEC-015** A burst of 100 fake requests in an hour is blocked and alerted. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:426`.
- **SEC-016** Twilio webhooks are verified with `X-Twilio-Signature`, Stripe webhooks with `Stripe-Signature` and a 5-minute tolerance, and an unverified call gets a 403 and is logged. Source: `BUILD_BRIEF.md:333`, `BUILD_BRIEF.md:334`, `BUILD_BRIEF.md:335`.
- **SEC-017** Every webhook is processed exactly once by event ID, so the same event arriving 100 times changes nothing after the first. Source: `BUILD_BRIEF.md:335`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:183`.
- **SEC-018** Every outside action carries an idempotency key derived from what it identifies. Source: `BUILD_BRIEF.md:149`.
- **SEC-019** Every route has authorization tests, and one homeowner's token can never read another job. Source: `BUILD_BRIEF.md:378`, `BUILD_BRIEF.md:379`.
- **SEC-020** Brute-force tests on codes, signature-forgery tests on webhooks and replay tests on idempotency are in the `Ci` security step. Source: `BUILD_BRIEF.md:380`, `BUILD_BRIEF.md:477`.
- **SEC-021** pip-audit and bandit run on every PR, the lock file is committed, and the Docker base image is pinned by digest. Source: `BUILD_BRIEF.md:366`, `BUILD_BRIEF.md:367`, `BUILD_BRIEF.md:369`.
- **SEC-022** The threat model in `docs/threat-model.md` is reviewed by Opus at K4 and in the final audit. Source: `BUILD_BRIEF.md:382`.
- **SEC-023** The ZAP baseline scan runs in Phase 2. Source: `BUILD_BRIEF.md:381`.
