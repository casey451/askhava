# Ask Hava threat model

Written in kit step K4 (`BUILD_BRIEF.md:598`). The checklist is OWASP ASVS Level 2 (`BUILD_BRIEF.md:293`). This model is reviewed by Opus at K4 and again in the final audit (SEC-022; `BUILD_BRIEF.md:382`). **Review status:** written by the K4 worker; the K4 fresh-eyes review is the runner's step (`BUILD_BRIEF.md:249`) and is not recorded here until it has happened.

The architecture it models is `docs/architecture.md`. Requirement IDs (for example `PRIV-005`) are in `docs/spec/`; `D-###` entries are in `docs/decisions.md`. ASVS references are section numbers of **ASVS 5.0.0** (D-099), for example `V6.6` (out-of-band authentication). Citations are `path:LINE`.

---

## 1. Method

STRIDE (Spoofing, Tampering, Repudiation, Information disclosure, Denial of service, Elevation of privilege) applied at each trust boundary in section 3, against the assets in section 2. Each threat names its mitigations as requirement IDs or decisions, the ASVS section it falls under, and what is left over. Residual risks are collected in section 6. Requirement-level ASVS checks are written as tests in K5 test design and later tasks; this file maps at section level.

## 2. Assets

| ID | Asset | Why it matters | Where it lives |
|---|---|---|---|
| A1 | **Contact vault**: homeowner names, phones, emails, addresses, on-site contacts, original photos, raw message text | Privacy promise ("Your number is never shared", `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:191`); Arizona breach law (OPS-020) | `privacy` schema, AES-GCM per field (PRIV-001, PRIV-002, D-093) |
| A2 | **Keys and secrets**: `VAULT_KEY`, `VAULT_INDEX_KEY`, `ID_SECRET`, vendor API keys, webhook signing secrets, session signing material | Whoever holds them reads A1 or acts as Ask Hava | Railway variables per project; network secrets in the build (`BUILD_BRIEF.md:62`–`BUILD_BRIEF.md:65`) |
| A3 | **Money**: saved cards (as Stripe IDs only), charges, refunds, disputes, the ledger | Wrong or double charges; refund loss; ledger drift | Stripe; `payments` schema (FEE-014, FEE-028) |
| A4 | **Phone numbers**: Ask Hava's texting number and its 10DLC standing, the masked-number pool, homeowner and business numbers | A suspended number stops the service (TXT-022); a leaked homeowner number breaks PICK-023 | Twilio; vault; `calls.number_pool` |
| A5 | **Consent and opt-out records**, card authorizations, business E-SIGN consent | Legal evidence for texting and charging (CON-005, CON-019, CON-022, FEE-015) | `intake`, `businesses`, `messaging`, `payments` schemas |
| A6 | **Owner account**: Casey's password, passkeys, TOTP secrets, recovery codes, sessions | The only portal with pause, refund and release powers (SEC-006) | `portals` schema; recovery codes offline (SEC-007) |
| A7 | **Business accounts and sessions**; **quote-page tokens**; **pick codes** | Access to quotes, confirms, charges, released details | `portals`, `jobs` (token hashes), Twilio Verify |
| A8 | **Routing and fee integrity**: license rules, eligibility, fee tiers, the 4-month rule, wording | Legal exposure (licensing law, consumer claims) and money | `catalog`, `licensing`, `quotes`, `payments` |
| A9 | **Audit log** and off-database records | Evidence, replay after restore (OPS-010, OPS-011) | `ops.audit_log`; object storage (D-094) |
| A10 | **Availability**: texting channel, web app, worker timers, database | Every waiting state depends on timers (JOB-003, JOB-006) | Railway; Twilio |
| A11 | **Build pipeline**: repository, CI checks, runner state, dependencies | A poisoned build ships to live | GitHub; `claude/kit-state`; `uv.lock` |

## 3. Actors and trust boundaries

**Actors:** homeowner; business owner and staff; Casey (owner); anonymous internet user; a malicious business (wants contacts without paying, or a lower fee); a malicious texter (spam, SMS pumping, prompt injection); a vendor-impersonating attacker (forged webhooks); the AI model (its output is untrusted); build agents (Claude sessions; trusted to follow §2 of the brief but checked by deny rules and the runner); third-party dependencies.

| ID | Boundary | Crossing it |
|---|---|---|
| TB1 | Internet ↔ `web` | Browsers: site, web chat, quote page, three portals |
| TB2 | Vendors → `web` webhooks | Twilio (texts, status, calls), Stripe, Resend, Lob |
| TB3 | `web`/`worker` → vendor APIs | Twilio, Verify, Stripe, Resend, Lob, license sources |
| TB4 | `ai` → Claude API | Scrubbed text out, schema fields back |
| TB5 | App → PostgreSQL | App role vs migration role; vault ciphertext vs key held in the app |
| TB6 | App → object storage | Off-database stream, nightly export |
| TB7 | Test project ↔ live project | Separate keys, data, numbers and webhooks (SEC-003) |
| TB8 | Build environment ↔ secrets and live systems | Cloud sessions, GitHub Actions, deny rules (`BUILD_BRIEF.md:300`–`BUILD_BRIEF.md:306`) |
| TB9 | Phone network ↔ Twilio | Caller ID and SMS sender spoofing, SIM swap |
| TB10 | Module ↔ module inside the app | Vault plaintext readers (D-093); money only through `payments` |

## 4. Threats and mitigations

### Spoofing

| ID | Threat | Boundary | Mitigations | ASVS | Left over |
|---|---|---|---|---|---|
| S1 | Forged Twilio or Stripe webhook fakes a payment, a reply or a call result | TB2 | `X-Twilio-Signature` and `Stripe-Signature` (5-minute tolerance) checked in the adapter before any module; 403 and a log line on failure; processed once by event ID (SEC-016, SEC-017). The Twilio signature is checked against the configured public base URL, never the request's `Host` header, because the host's proxy rewrites it | V4.1, V12.3 | A leaked signing secret (see I8) |
| S2 | Someone holding a forwarded quote-page link picks a quote | TB1 | The page shows no contact details; a pick needs a 6-digit Verify code sent to the homeowner's own phone (10 minutes, 5 tries, 15-minute cooldown) or, when texting is impossible, to an email the homeowner gave in their own conversation (PICK-010, PICK-013, PICK-029, D-050) | V6.6 | R1 |
| S3 | A spoofed SMS sender sends "2 YES", CLOSE or STOP as a homeowner | TB9 | A pick still needs the business's Confirm and payment before anything is released (PICK-022); STOP only stops texts (CON-012); a closed job can be restarted. "2 YES" picks only with exactly one job in Quotes ready (PICK-031) | V6.6 | R3 |
| S4 | A business signs up as someone else's licensed company | TB1 | License class from the state record, never typed; identity code or letter only to a public-record contact; owner approval plus identity plus license or city-list match (BIZ-002, BIZ-016, D-059, D-060) | V6.4 | R8 |
| S5 | Owner account takeover (phishing, credential stuffing, stolen laptop) | TB1 | argon2id password plus passkey or TOTP; sessions at most 12 hours; at least two second factors plus offline recovery codes; no email-only recovery; rate limits on sign-in (SEC-005, SEC-007, SEC-014, D-098) | V6.2–V6.5, V7.3 | Phishing of a TOTP code (passkeys resist it) |
| S6 | Business portal takeover through the business's email | TB1 | Single-use 15-minute link to the address on file plus an SMS code on a new device (SEC-004); the portal can't release contact details without a confirmed pick (BIZ-014) | V6.3, V6.6 | R2 |
| S7 | Caller-ID spoofing of a linked business phone on a masked line | TB9 | Masked lines exist only for connected jobs; readback of first name and street only to linked phones of a paid pick (CALL-005, D-026); lines close at 30 days or earlier (CALL-009) | V6.6 | R4 |
| S8 | SIM swap of a homeowner's phone | TB9 | The attacker can pick a quote, but details go to the business, not the attacker (PICK-022, PICK-023) | V6.6 | Low |

### Tampering

| ID | Threat | Boundary | Mitigations | ASVS | Left over |
|---|---|---|---|---|---|
| T1 | SQL injection | TB1, TB2 | SQLAlchemy bound parameters only; pydantic validation of every input (SEC-010); bandit in `Ci` (SEC-021) | V1.2, V2.2 | — |
| T2 | Stored or reflected XSS through homeowner or business text | TB1 | Jinja autoescape always on; user text never marked safe (SEC-011); CSP with no inline script, nonces where needed (SEC-012); no third-party scripts (SEC-013) | V1.2, V3.2, V3.4 | — |
| T3 | Cross-site request forgery on Confirm, pick, refund or switch actions | TB1 | CSRF token on every form; `SameSite=Lax`; cookies `__Host-` prefixed (SEC-009, D-099); review step before money or agreement actions (PICK-017, PICK-021) | V3.3, V3.5 | — |
| T4 | Prompt injection in homeowner or business text changes routing, fees, wording or claims | TB4 | The AI gets only scrubbed text, has no tools, returns only schema fields; nothing it returns is executed (AI-005, AI-006); keyword rules give a second opinion and gas words override (AI-011, AI-012); fees come from the quote amount by code (FEE-005); the original wording is always shown and a summary adding a claim is rejected (PICK-003, PICK-004); tests prove routing, fees and disclosures don't change (AI-007) | V2.3 | R6 |
| T5 | A business games the fee tier (amounts just under an edge, a recurring service sent as one-time) | TB1 | Tier from the chosen quote only (FEE-001, FEE-005); the 4-month rule with the per-category one-time list (D-037); edge-clustering flag (FEE-033); mutation tests on the 4-month rule (`BUILD_BRIEF.md:472`) | V2.3 | Flag only, no automatic action |
| T6 | Ledger or audit log edited after the fact | TB5 | Append-only ledger and audit log (FEE-028, OPS-010); the app's database role has no `UPDATE` or `DELETE` on them (D-092); audit copied off the database as it happens (OPS-011); nightly reconcile to Stripe (FEE-029, FEE-030) | V16.4, V15.3 | The migration role can still edit; it is used only by migrations |
| T7 | Races: two picks, two charges, a timer firing after the job moved | TB5 | Row lock plus "only if still in the expected status" (JOB-004); a key per charge attempt (FEE-016); check before every charge (FEE-017); outbox and webhook dedupe (TXT-011, SEC-017); timers re-check status (JOB-006) | V15.4 | — |
| T8 | Replay of a pick code, sign-in link or webhook | TB1, TB2 | Codes bound to their own pick or sign-in and single use; links single use, 15 minutes (SEC-004); webhooks by event ID; replay tests (SEC-020) | V6.6, V4.1 | — |
| T9 | A malicious photo (wrong type, decompression bomb, hidden metadata) | TB1 | Type and size from the file bytes; re-encoded with EXIF and GPS removed; stored privately; short-lived signed links (PRIV-013, PRIV-014); a pixel-count limit before decoding; pillow pinned to a release with no known vulnerabilities (D-096) | V5.2, V5.3, V5.4 | New image-parser vulnerabilities (pip-audit on every PR) |
| T10 | A poisoned dependency or base image | TB8 | Committed lock file with hashes; pip-audit and bandit on every PR; Dependabot; base image pinned by digest (SEC-021); 14-day pinning delay (D-096) | V15.2 | R10 |
| T11 | A build agent changes tests, workflows or protected paths to get green | TB8 | Protected paths only in harness tasks with an Opus review; runner path check; tests changed only through a logged spec change; tree-hash check on merge (`BUILD_BRIEF.md:69`, `BUILD_BRIEF.md:70`, `BUILD_BRIEF.md:248`, `BUILD_BRIEF.md:263`) | V15.1 | — |

### Repudiation

| ID | Threat | Mitigations | ASVS | Left over |
|---|---|---|---|---|
| RP1 | A homeowner denies agreeing to texts | Exact reply, time and consent-prompt version stored; 5-year retention (CON-005, CON-022) | V16.2 | — |
| RP2 | A business disputes a charge | Card authorization version and time (FEE-015); review step before Confirm (PICK-021); call log with answer type and length (CALL-008); dispute evidence from the audit log (FEE-027) | V16.2 | Bank decides disputes |
| RP3 | Disagreement over who did an owner action | Every owner action audit-logged (SEC-006); audit log append-only and off-database (OPS-010) | V16.2, V16.3 | — |

### Information disclosure

| ID | Threat | Boundary | Mitigations | ASVS | Left over |
|---|---|---|---|---|---|
| I1 | Database dump or backup exposes contacts | TB5, TB6 | Field-level AES-GCM with a key outside the database (PRIV-002); blind index for phone lookup; associated data binds ciphertext to its row (D-093); encrypted backups (PRIV-018); nightly export under its own key (D-094) | V11.3, V14.2 | R7 |
| I2 | Contact details typed into a description reach job cards | TB10 | Scrubber before any AI call and the final plain-code gate on every card (AI-002, AI-003, CARD-007, CARD-008, CARD-009); weekly spot-check (OPS-009); business quotes with contact info bounce (PICK-007) | V14.2 | R5 |
| I3 | Contact details released without a paid pick | TB10 | Only `picks` releases, only on paid or free (PICK-022, PRIV-005); owner release only re-sends (D-074); business portal can't release (BIZ-014); readers limited and audited (PRIV-003, D-093) | V8.2, V8.3 | — |
| I4 | Homeowner's phone number reaches a business | TB9 | Masked calls present the pool number; the number is never in a release (PICK-023, CALL-002, PRIV-020) | V14.2 | — |
| I5 | Quote-page token leaks (forwarded, referrer, logs, history) | TB1 | 128-bit token in the path, not the query; no contact details on the page; `Referrer-Policy: no-referrer`; hidden from search engines; tokens redacted from the request log; dead 30 days after close; pick still needs the code (PICK-010, PICK-011, SEC-012) | V14.2, V3.4 | R9 |
| I6 | One homeowner's token or session reads another job; one business reads another's quotes | TB1 | Every query scoped to the signed-in principal; authorization tests on every route; token isolation tests (SEC-019) | V8.2, V8.3 | — |
| I7 | Personal data kept by the AI provider | TB4 | Zero-retention API only, scrubbed text only; no AI call in live until zero retention is signed; stand-in AI during the build (AI-001 to AI-004, AI-008, AI-009) | V14.2 | Provider's own compliance |
| I8 | Secrets leak through code, logs, chat, screenshots or PR text | TB8 | gitleaks and the PII scanner fail the build; deny rules on live key patterns and `.env*`; settings validated at startup (SEC-001, SEC-002; `BUILD_BRIEF.md:300`–`BUILD_BRIEF.md:306`) | V13.3 | R11 |
| I9 | Personal data in logs or Sentry | TB3 | Request IDs only, never phone numbers (PRIV-016); Sentry without default PII plus a scrubber (PRIV-017); message bodies only in the vault (D-093) | V16.2 | — |
| I10 | Test and live data or keys mixed | TB7 | Separate projects, keys, databases, numbers and webhooks; the app refuses to start on a mismatched key (SEC-002, SEC-003); fake data only outside live (PRIV-019) | V13.1 | — |
| I11 | Readback on the masked line tells a stranger a name and street | TB9 | Readback only to linked phones of a paid pick, never to unlinked callers (CALL-005, D-026) | V8.3 | R4 |
| I12 | Verbose errors show internals | TB1 | Generic error pages; details only in Sentry (scrubbed) | V16.5, V13.4 | — |

### Denial of service

| ID | Threat | Boundary | Mitigations | ASVS | Left over |
|---|---|---|---|---|---|
| D1 | Request floods and fake sign-ups | TB1 | Rate limits per phone, IP and business, 5 requests per phone per hour, 50 sign-ups a day; a burst of 100 is blocked and alerted (SEC-014, SEC-015) | V2.4 | — |
| D2 | SMS pumping: web chat or sign-up forms used to make Ask Hava text or call costly numbers | TB1, TB3 | A number typed on the web gets only the consent text until it replies OK (CON-025); rate limits per IP and per number; texts and calls only to US and Canadian area codes (D-100); Verify sends rate-limited with cooldown (PICK-013) | V2.4 | Cost of the consent text itself |
| D3 | Webhook floods | TB2 | Signature check first, cheap 403 (SEC-016) | V4.1 | Host-level limits |
| D4 | The texting number is suspended after complaints | TB9 | Consent first on every path (CON-001); opt-outs honored at once (CON-008); content screening (CARD-010); no second number to dodge a suspension; web-and-email mode (TXT-021, TXT-022) | V2.3 | R12 |
| D5 | A vendor outage stalls jobs | TB3 | Retries with the same keys; email fallback; Stripe unreachable 4 h releases the pick; safe mode (TXT-014, FEE-018, OPS-001) | V16.5 | — |
| D6 | A worker dies holding a task | TB5 | Leases expire and another worker takes the task; timers live in the database (JOB-006; architecture section 5) | V15.4 | — |
| D7 | Database loss | TB5 | Point-in-time recovery; nightly export; replay with derived IDs that sends nothing twice (OPS-012 to OPS-019) | V14.2 | Restore drill proves it in Phase 2 (OPS-014) |

### Elevation of privilege

| ID | Threat | Boundary | Mitigations | ASVS | Left over |
|---|---|---|---|---|---|
| E1 | A business or homeowner reaches owner powers | TB1 | Separate sign-in realm and session for the owner portal; every owner route checks the owner role; route authorization tests (SEC-006, SEC-019) | V8.2, V8.3 | — |
| E2 | Owner release power used to leak contacts | TB10 | Release only re-sends a paid or free release (D-074); audit-logged (SEC-006) | V8.3 | — |
| E3 | AI output treated as instructions | TB4 | No tools; only fixed fields read; never writes texts, moves money or releases details (AI-006, TXT-004) | V2.3 | — |
| E4 | Template or deserialization injection | TB1 | Texts only from fixed templates by ID (TXT-004); no pickle or YAML `load` (only `safe_load`, D-036) | V1.5 | — |
| E5 | A build agent uses live keys or the live project | TB8 | Deny rules; no hosting key for Claude; live auto-deploy off; startup check (`BUILD_BRIEF.md:68`, `BUILD_BRIEF.md:300`–`BUILD_BRIEF.md:306`; OPS-022) | V13.3 | — |

## 5. ASVS 5.0.0 coverage at section level

| ASVS chapter | Where it is met |
|---|---|
| V1 Encoding and Sanitization | T1, T2, E4 |
| V2 Validation and Business Logic | SEC-010; T4, T5; D1, D2 |
| V3 Web Frontend Security | SEC-009, SEC-012, SEC-013; `__Host-` cookies (D-099) |
| V4 API and Web Service | S1, T8, D3 |
| V5 File Handling | T9 |
| V6 Authentication | SEC-004, SEC-005, SEC-007, SEC-008, PICK-013; deviations R1, R2 |
| V7 Session Management | SEC-004, SEC-005, SEC-008, SEC-009 (rotation on login) |
| V8 Authorization | SEC-006, SEC-019; I3, I6, E1 |
| V9 Self-contained Tokens | Not used: sessions are server-side records, quote-page tokens are random values checked against the database |
| V10 OAuth and OIDC | Not used |
| V11 Cryptography | AES-GCM vault, argon2id passwords, HMAC derived IDs and blind index, `secrets` module for tokens (PRIV-002, SEC-005, CARD-017) |
| V12 Secure Communication | HTTPS only with HSTS (SEC-012); vendors over TLS |
| V13 Configuration | SEC-001, SEC-002, SEC-003; deviation R11 |
| V14 Data Protection | PRIV-001 to PRIV-020; retention (D-013, D-079) |
| V15 Secure Coding and Architecture | `docs/architecture.md`; T6, T7, T10 |
| V16 Security Logging and Error Handling | PRIV-016, PRIV-017, OPS-010, OPS-011; I12 |
| V17 WebRTC | Not used |

## 6. Residual risks

| ID | Risk | Why it stays | Owner and watch |
|---|---|---|---|
| R1 | **Homeowners sign in and pick with SMS codes only.** ASVS 6.6.1 (Level 2) wants a stronger method also offered | The brief and v9.3 set this (PICK-013, SEC-008); homeowners are one-off users. A stolen code can pick but releases nothing to the attacker | Accepted deviation (D-099); revisit in the final audit |
| R2 | **Business portal on a known device needs only the emailed link.** ASVS 6.3.3 wants multi-factor or a combination | The brief sets it (SEC-004); the known-device cookie is a second "something you have" | Accepted deviation (D-099) |
| R3 | Spoofed SMS sender can send short commands as a homeowner | SMS has no sender authentication | Impact limited (S3); alerts on unusual CLOSE patterns are a possible later addition |
| R4 | Caller-ID spoofing of a linked business phone on a masked line | Phone network limitation | The `calls` task evaluates Twilio's STIR/SHAKEN result on inbound calls and requires the PIN when attestation is not full; logged as a decision there |
| R5 | The scrubber misses a creative contact detail (spelled out, social handles) | Plain-code rules can't catch everything | Final card gate, quote bounce, weekly spot-check (OPS-009); new patterns added as tests |
| R6 | A new prompt-injection trick changes an AI field | Model behavior changes | Schema limits the damage; keyword second opinion; injection tests kept as a suite (AI-007) |
| R7 | The app process holds `VAULT_KEY`; a full app compromise reads the vault | Field encryption protects dumps and backups, not a live compromise | Least privilege, audit of every read, alerts on unusual read volume |
| R8 | A real business's public-record contact is itself out of date or compromised | Public records lag | Owner approval and flagged sign-ups (D-060) |
| R9 | A quote-page link is a bearer token in a URL (ASVS 14.2.1 is about query strings; this is a path) | Links must work from a text message | No contact details on the page; pick needs the code; no-referrer; expiry |
| R10 | A dependency is compromised before advisories exist | Supply chain | 14-day delay (D-096), hashes, audits on every PR |
| R11 | Railway variables are the secret store, not a dedicated key vault (ASVS 13.3.1) | The brief fixes where keys live (`BUILD_BRIEF.md:62`–`BUILD_BRIEF.md:65`) | Only Casey can read live variables; Claude never holds a hosting key (OPS-022) |
| R12 | Carrier filtering or suspension despite correct consent | Outside Ask Hava's control | Web-and-email mode; no second number (TXT-022) |
| R13 | Photos and contacts persist in backups after deletion until the backups age out (point-in-time window plus the 30-day export) | Backups are immutable by design | Backups encrypted (PRIV-018); stated in the privacy policy by the terms task |
| R14 | One owner, no backup person: an attack during Casey's absence goes unanswered | Changes file decision (OPS-002) | Safe mode after 24 hours (OPS-001) |
