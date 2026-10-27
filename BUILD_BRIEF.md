# BUILD_BRIEF.md — Ask Hava: instructions for Claude Code

**Owner:** Casey Solomon · **Written:** October 27, 2026 · **Read this first, every session.**

This file tells Claude Code how to build Ask Hava from start to finish. It sits at the repository root. Everything it refers to is in `sources/`, which is read-only.

**Rule zero: this file decides *how* to build, and `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md` decides *what* to build.** If anything else in the repo contradicts them, they win. If they contradict each other, stop and ask Casey (§16).

---

## 0. Casey's setup before the first session (about 15 minutes, no vendor accounts)

Do these in this order. Protecting `main` first would stop you from adding the files. **Everything here can be done on a phone.** `PHONE_SETUP.md` gives the exact taps for each step.

1. **GitHub:** create a private repository named `ask-hava` and tick "Add a README" so `main` exists.
2. **Claude GitHub App:** install it on that repository (github.com/apps/claude).
3. **Put the files in the repo:**
   - `BUILD_BRIEF.md` at the root;
   - the whole stress-test package under `sources/`.

   Either drag them in on github.com, or tell Claude the repo name and it will attach the repo and push them.
4. **Repository settings:**
   - **Rules → Rulesets → New branch ruleset** for `main`:
     - require a pull request (0 approvals);
     - require status checks, **typing these four names in yourself**: `ci`, `twin`, `security`, `a11y`;
     - require branches to be up to date before merging;
     - block force pushes;
     - restrict deletions.
   - **General:** allow auto-merge; allow squash merging only; automatically delete head branches.
   - **Actions → General:** leave workflow permissions at "Read repository contents" (each workflow asks for what it needs), and tick "Allow GitHub Actions to create and approve pull requests."
5. **Cloud environment** at claude.ai/code: name it `ask-hava-build`, set network access to Trusted (the default), and leave variables and secrets empty for now.
6. **Create the scheduled task** at claude.ai/code/routines, using the exact settings and prompt in Appendix A. Then tap **Run now** once. If the scheduled-task page won't work on your phone, use Appendix C instead: start each cycle by hand from the Claude app until you're at a computer.
7. **Your answers to Claude** always go in GitHub issues that the repo's `notify` workflow opens. They're labeled `owner` and @-mention you, so GitHub notifies you. Reply in the issue from your own GitHub account; the GitHub phone app works. Don't answer build questions in chat, because the build never sees chat.
8. **Backup runner (optional; do it later at a computer, since it needs the command line):** run `claude setup-token` and paste the token into the repo as the GitHub Actions secret `CLAUDE_CODE_OAUTH_TOKEN`. Never paste it into chat. It runs only if the runner turns it on (§7).

That's all until the account checkpoint (§6).

---

## 1. Mission and definition of done

Build the Ask Hava quote service exactly as specified:
- **People who need work done** text or web-chat a request.
- **Every eligible opted-in local business** gets an anonymous job card.
- **The homeowner** sees all eligible quotes on a private page and picks one with a texted code.
- **The picked business** confirms, pays a fixed introduction fee and calls through a masked number.

**Done means all of these are true:**
1. Every numbered requirement in `docs/spec/` has passing tests, traced back to its source line.
2. The twin, unit, integration, contract, end-to-end, security and accessibility suites all pass in CI on `main`.
3. Phase 2 tests pass against the real vendor sandboxes, and the Railway test site runs the full flow.
4. The restore drill passed on Railway, including the switch to the restored database.
5. A fresh Opus audit found no open Critical or High findings.
6. The handover pack is complete (§15).
7. Casey approved Gate B. Gate C (going live) is his alone.

Never report something as done, tested or passing unless a tool run in this repository shows it.

## 2. Rules that can't bend

1. **No sign-ups, purchases, outreach, publishing, live messages, live charges or production deployments without Casey's explicit go.** Never create accounts or accept terms for him.
2. **Never paste, print, log, commit or store secrets.** Keys live only in three places:
   - network secrets (build);
   - the app host's variables (running app);
   - the one GitHub Actions secret `CLAUDE_CODE_OAUTH_TOKEN` (backup runner only).
3. **Never put homeowner names, phones, addresses or photos into an AI tool that keeps data.** The build uses fake data only, and the app uses the zero-retention API with a code scrubber first.
4. **Don't claim something was tested unless it was.**
5. **Never use the live Railway project, live keys or live phone numbers before Gate C.** Deny rules enforce this.
6. **Never weaken a test to make it pass.** A wrong test is changed only through a spec change logged in `docs/decisions.md`.
7. **Protected paths:** a task may change `.github/`, `.claude/`, `runner/`, `stand_ins/`, `sources/` or tests it doesn't own only if it is a harness task (type `harness` in `kit/tasks.json`) with an Opus review. The runner rejects any other diff that touches them.

## 3. Repository layout

```
BUILD_BRIEF.md            this file
PHONE_SETUP.md            Casey's setup steps, phone-friendly
CLAUDE.md                 short session rules (<200 lines), written in K1
sources/                  read-only inputs (the package); fingerprinted in K0
docs/constitution.md      K1
docs/spec/*.md            K2: numbered requirements, one file per area
docs/catalog/             K2b: categories.yaml + worked_examples.yaml (Casey-approved)
docs/decisions.md         decision log (append-only)
docs/architecture.md      K4
docs/threat-model.md      K4
docs/a11y.md              accessibility plan, statement and manual checklist
kit/tasks.json            K6: task graph
kit/prompts/T###.md       K7: one prompt per task
runner/                   the runner (K8)
app/                      application code
tests/                    unit, property, integration, contract, e2e, a11y, security
stand_ins/                fake Twilio, fake Resend, fake Lob, stand-in AI, Stripe mock config
twin/                     the digital twin (updated in stage 1)
.claude/                  settings.json (deny rules), agents/, skills/, hooks/
.github/workflows/        ci, twin, security, a11y, auto-pr, nightly-main, backup-runner
```

**The runner's branch:** runner state, STATUS.md and PROGRESS.md live on the separate branch **`claude/kit-state`** (§7), never on `main`. Writing them to `main` would need a pull request, and that merge would start another runner. Casey reads STATUS.md at `github.com/<owner>/ask-hava/blob/claude/kit-state/STATUS.md`. Every runner issue links to it.

**Branch names:** all branches Claude pushes start with `claude/`: `claude/kit-state` and `claude/task/T###-<slug>`. Scheduled-task runs push to `claude/` branches by default, so no extra permission is needed.

## 4. Stack (decided; don't reopen)

| Need | Choice | Why |
|---|---|---|
| Language | Python 3.12 | The twin and its rules are Python |
| Web | FastAPI + Jinja2 server-rendered pages; small vanilla JS from the prototype; progressive enhancement (works without JS) | Keeps the prototype's accessible HTML; fewer moving parts |
| Database | PostgreSQL 16; SQLAlchemy 2; Alembic migrations (forward-only, each tested) | Matches Railway and the cloud VM (Postgres 16 preinstalled) |
| Background jobs and timers | A Postgres job table polled with `SELECT … FOR UPDATE SKIP LOCKED`, plus a scheduler process | No Redis, one data store, survives restarts |
| Messaging and calls | twilio SDK behind an adapter | Swappable for the stand-in |
| Payments | stripe SDK; SetupIntent with off-session use; webhooks | Card data never touches our servers (PCI SAQ A) |
| Email / letters | Resend and Lob, each behind an adapter | — |
| AI | anthropic SDK, Haiku 5.5, structured output only, behind an adapter | Stand-in during the build |
| Validation | pydantic v2 | — |
| Packaging | uv with a committed lock file | Reproducible installs |
| Tests | pytest, hypothesis (property tests), playwright (Python), axe-core injected into pages, mutmut (mutation tests on money and license rules) | — |
| Quality | ruff (lint and format), mypy --strict, complexity limits in ruff | — |
| Security tooling | bandit, pip-audit, gitleaks (secrets), the custom PII scanner, OWASP ZAP baseline scan (Docker) in Phase 2 | — |
| Monitoring | sentry-sdk with PII scrubbing; `/healthz`; UptimeRobot | — |
| Deploy | Dockerfile; the app host chosen per changes file §3 (Railway Pro if its Backups tab shows point-in-time recovery, otherwise Render), with a separate test project (Phase 2) and live project (Gate C) | Fully separate projects, keys and data. "Railway" below means whichever host was chosen |

- **Versions:** K4 pins exact versions available at that time.
- **New libraries:** any library not on this list needs a `docs/decisions.md` entry with its purpose, license and a pip-audit result.

## 5. Architecture (K4 finalizes; these boundaries are fixed)

**Modules.** Each owns its own files and tables, and no module reaches into another's tables.

| Module | Owns |
|---|---|
| `catalog` | Categories, job types, license rules, fee rules, feature switches |
| `intake` | Inbound SMS and web chat; conversation state; consent; emergency detection; category matching |
| `privacy` | The plain-code scrubber; the contact vault (field-level encrypted); retention and purge jobs |
| `ai` | Adapter: request reading and quote summarizing; strict schemas; never sees unscrubbed text |
| `licensing` | ROC daily lists; live lookup at pick; other agencies; the city license list; handyman rules |
| `businesses` | Sign-up, identity chain, checklist, tags, consent records, pause rules |
| `jobs` | The job lifecycle state machine; every waiting state has a timeout |
| `quotes` | Quote intake and parsing; eligibility rules; arrival order; the original text kept beside the summary |
| `picks` | Pick by code; confirm window; release of contact details; losers notified |
| `payments` | Stripe customers, SetupIntents, charges, refunds, disputes, the ledger, nightly reconcile with auto-match or auto-refund |
| `messaging` | Outbox and inbox (never send twice); templates in English and Spanish; quiet hours; opt-out keywords; delivery tracking |
| `calls` | Masked numbers from a pool, PIN routing, call logs (never recorded) |
| `portals` | Homeowner page, business portal, owner portal |
| `site` | Public website built from the prototype |
| `ops` | Alerts, pause switches, safe mode, status banner, web-and-email mode, audit log viewer |
| `recovery` | Backup checks, the restore drill, replay with derived IDs |
| `adapters` | One interface per vendor; real and stand-in implementations chosen by configuration |

**Invariants:**
- every outside action carries an idempotency key derived from what it identifies;
- every state change writes an audit row;
- no homeowner contact detail leaves the vault except on a confirmed, paid pick;
- money moves only through `payments`, and the ledger always reconciles to Stripe.

## 6. Build order

**Phase 1 — build everything against stand-ins.** This covers the kit (K0–K9) and stages 1–5 in `sources/02_build_plan/Build_Plan.md`.

The stand-ins:
- **Texting and calls:** the fake Twilio (from the twin).
- **Payments:** `stripe/stripe-mock` (Docker) plus the twin's fake Stripe for webhooks and timing.
- **Email and letters:** fake Resend and fake Lob.
- **The AI:** a deterministic stand-in AI.
- **The database:** a local Postgres.

Each stand-in has **contract tests written from the vendor's published API documentation**, covering request shapes, response shapes, error codes and webhook signatures.

Phase 1 also produces:
- the terms, privacy policy and SMS terms (from changes file section 7);
- the Twilio campaign description;
- the DNS records for Cloudflare;
- the runbook and owner guide;
- `docs/account-checklist.md`.

**Account checkpoint.** When Phase 1's last task merges, the runner:
1. sets state to `WAITING_FOR_OWNER`;
2. writes an outbox message that becomes the issue **"Your turn: set up accounts"**, containing `docs/account-checklist.md`;
3. puts the same checklist at the top of STATUS.md;
4. stops.

The checklist has two parts:
- **Part 1, test accounts (no LLC needed):** Railway, Stripe test mode, Twilio trial, Resend, Lob, Sentry, UptimeRobot and a Claude Console workspace.
- **Part 2, business accounts, needed only for Gate C:** LLC with a Maricopa County registered agent, EIN, Google Workspace, Arizona TPT license, insurance, Twilio brand and campaign registration, Stripe activation and zero retention.

For each account the checklist gives the exact clicks and says exactly where each test key goes: either a network secret (with the host to lock it to) or a test-project variable on the app host.

The runner resumes when Casey comments "accounts done" in the issue (§7, "Owner input").

**Phase 2 — final build and testing.**
1. Add each vendor's real sandbox behind its adapter. The Twilio secret uses the "Username and password" type.
   - **Twilio in Phase 2:** use Twilio's test credentials and magic test numbers, which send nothing real. If the account has no test credentials, real texts may go **only to Casey's own verified phone**, and only after he answers "yes" to that exact question in an `owner` issue. No other real phone receives anything before Gate C.
2. Run every contract test against the real sandbox, and fix any differences in the adapter and stand-in together.
3. Deploy to the Railway **test** project.
4. Run the end-to-end journeys there.
5. Test the real AI from there; this is the first time any real AI key is used.
6. Run the ZAP baseline scan.
7. Run the restore drill with the switchover.
8. Run a fresh Opus audit.

Then **Gate B**: Casey's guided click-through, plus his screen-reader check (§9).

**Stage 0.5** (the simple askhava.com page) comes after the checkpoint and only with Casey's separate go.

**Gate C** comes after the business accounts. Casey enters live keys and says "go live." A real-phone rehearsal runs on his phone, and the first live charge needs his go.

## 7. The runner (how the build moves itself)

**Shape:**
- one scheduled task (Appendix A) runs the **runner**;
- the runner does **one task per session, one session at a time**. There's no parallel building, so there are no state races or merge conflicts;
- parallelism exists only inside a session, as read-only review subagents.

**What starts a runner session:**
1. **Primary:** the scheduled task's **GitHub trigger** on `pull_request.closed` where the PR is merged into `main`. Each merge starts the next task.
2. **Safety net:** the same task's **hourly schedule**. If a runner finds a live lease or nothing eligible, it exits in under a minute.
3. **Backup:** `.github/workflows/backup-runner.yml` runs hourly through the Claude Code GitHub Action with `CLAUDE_CODE_OAUTH_TOKEN` (subscription, not pay-per-use).
   - **When it runs:** it exits at once unless `kit/state.json` has `"backup_runner": "on"`. The runner sets that flag (and sends an outbox message saying so) only if the scheduled task fails to start twice in K9 or later.
   - **Pushes:** it leaves `github_token` unset, so it pushes as the Claude GitHub App, and its pushes start workflows normally.

**State and lease:**
- `kit/state.json`, STATUS.md and PROGRESS.md live on the `claude/kit-state` branch;
- the runner claims a task by committing a lease (task ID, session URL, expiry 3 hours) and pushing. It renews the lease every 30 minutes while working;
- a rejected (non-fast-forward) push means another runner won, so this one exits. Git's own rule is the lock;
- only the runner writes state. Worker subagents never touch `claude/kit-state`;
- **nothing new is claimed while any task is `MERGE_PENDING`.** One task is in flight at a time, so a branch can never fall behind another task's merge. If `main` moved anyway (for example, Casey edited a file), the runner merges `main` into the task branch with a normal merge commit; it never rebases and force-pushes.

**Talking with Casey (the only channel), done through git so no session needs GitHub API access:**
- **Why not post issues directly:** Claude's pushes and comments carry **Casey's own GitHub identity**. GitHub doesn't notify people about their own actions, and the runner couldn't tell its own comments from Casey's answers.
- **Outbox:** to ask or tell Casey something, the runner commits a file to `outbox/` on `claude/kit-state` (`outbox/<id>.md`: title, body, kind). The **`notify`** workflow (`on: push` to `claude/kit-state`, `permissions: issues: write, contents: write`) opens or updates an issue as `github-actions[bot]`, labels it `owner`, @-mentions the repository owner (so GitHub notifies him), and moves the file to `outbox/sent/`.
- **Inbox:** the **`inbox`** workflow (`on: issue_comment` and `issues: closed`, for issues labeled `owner`) runs only when the commenter is the repository owner (`github.event.sender.login == github.repository_owner`). It writes the comment text to `inbox/<issue>-<comment-id>.md` on `claude/kit-state`.
- **Reading answers:** every runner cycle reads new `inbox/` files first. Each one is Casey's answer to that issue. The runner records it in `kit/state.json` (and in `docs/decisions.md` through the next task's PR), clears that wait, and moves the file to `inbox/done/`.
- This covers K3 questions, the K2b approval, Gate A ("OK go"), `BLOCKED` questions, "accounts done," the Stage 0.5 go, Gate B, Gate C and the first live charge.
- Chat with Claude elsewhere is never read by the build.

**Blocking is narrow:** a `BLOCKED` task holds only the tasks that depend on it. Other eligible tasks continue. The whole build waits only at a gate, or when no eligible task remains.

**One task, step by step:**
1. **Start routine:**
   - read this file's §2 and §7;
   - read `kit/state.json` and the last 30 lines of PROGRESS.md (both on `claude/kit-state`), and the last 20 commits on `main`;
   - run the smoke test on `main`. If `main` is red, the only task is "fix main";
   - honor gate and `WAITING_FOR_OWNER` states by posting status and exiting. A `BLOCKED` task holds only its dependents.
2. **Pick** the next task whose dependencies are all `DONE`, and claim the lease.
3. **Delegate the work** to a subagent:
   - it runs on the **model named in the task** (Haiku, Sonnet or Opus, passed explicitly as a full model ID);
   - it gets only the task prompt `kit/prompts/T###.md` plus the files it lists;
   - it works on branch `claude/task/T###-<slug>`, created from the latest `main`;
   - **model check:** the runner confirms which model actually ran from evidence it can inspect itself (session or usage records), not from what the subagent says. K8 establishes which evidence is reliable and which model-ID format the subagent call accepts. A substituted model fails the attempt.
   - **path check:** before handing in, the runner checks the diff against the task's owned files and the protected paths (§2, rule 7).
4. **Fresh-eyes review:** a reviewer subagent (Opus for money, privacy, security and recovery tasks; Sonnet otherwise) sees only the diff and the task's spec files. It checks for:
   - drift from the spec;
   - duplicated logic;
   - dead code and TODOs;
   - secrets or PII;
   - accessibility regressions.

   Findings go back to the worker subagent once.
5. **Hand in:** push the branch. The push comes from Claude's GitHub identity, so workflows run. The `auto-pr` workflow (triggered `on: push` for `claude/task/**`, `permissions: contents: write, pull-requests: write`) opens a PR and enables squash auto-merge.
6. **Required checks:** `ci`, `twin`, `security` and `a11y` each trigger **`on: push` for `claude/task/**`** as well as `on: pull_request`. A PR opened by `GITHUB_TOKEN` doesn't start `pull_request` workflows; checks attach to the commit, so the push-started runs satisfy the PR. Each workflow's **job name equals the required check name exactly**, and each sets `permissions: contents: read`.
7. **Merge:** GitHub merges automatically when the checks pass on an up-to-date branch. The merge event should start the next runner session (K9 proves this; otherwise the hourly run carries on).
8. **Finish:** the next runner marks the task `DONE` only after confirming two things:
   - the PR merged with all four checks green on its head SHA;
   - **the squash commit on `main` has exactly the same tree as that tested head** (`git rev-parse <sha>^{tree}` matches).

   That proves `main` is the tested code, even though a merge done through `GITHUB_TOKEN`'s auto-merge may not start workflows on `main`. A `nightly-main` workflow (scheduled, so it always runs) runs the full suite on `main` every night and at the end of every stage. A red nightly run makes "fix main" the next task. The runner then updates STATUS.md and PROGRESS.md on `claude/kit-state`.

**Failures:**

| What happened | What the runner does |
|---|---|
| Checks fail | The same model gets one more try with the failure log. A Sonnet task that fails twice gets one Opus try, logged. After that the task is `BLOCKED`: dependents wait, and an outbox message asks Casey one plain question |
| Flaky test (passes on rerun with no change) | Logged, and a fix task is queued first in line. A quarantined test keeps running and reporting; it may leave its required check for at most 48 hours. It is never deleted |
| Vendor sandbox outage (Phase 2) | `RETRY_LATER` with backoff, so it isn't counted as a failure |
| Usage limit hit, or a run rejected | `RETRY_AFTER_RESET`; the hourly run picks it up after the reset. No pay-per-use |
| Lease expired with no PR | The task returns to the queue, and the stale branch is kept for evidence |

**STATUS.md (Casey's view):**
- tasks done out of total, by stage;
- what's running;
- anything blocked and the one question it needs answered;
- the next owner action, if any;
- the last usage reading.

**Notifications to Casey** go through the outbox: GitHub emails him and notifies him in its phone app. They cover gate requests, `BLOCKED` tasks, the account checkpoint, and Phase 1 or Phase 2 being complete.

**Things K8 must check in the real cloud environment, with a fallback for each:**
- **Browsers in the session:** can Playwright's browsers be installed? If not, browser and accessibility tests run only in GitHub Actions, which is where the `a11y` check runs anyway.
- **Stripe's mock server:** can `stripe/stripe-mock` be pulled from Docker Hub? If not, it runs in CI only, and the twin's fake Stripe is used in sessions.
- **Merges starting the next run:** does the GitHub trigger fire on merges done by auto-merge? If not, the hourly run carries the chain.

**K8 builds this runner and K9 proves it.** The rehearsal uses three practice tasks: one passes, one fails on purpose, and one is triggered twice. It also includes a simulated usage-limit rejection. If any mechanism here doesn't behave as described, K9 must record that and switch to the documented fallback before Gate A.

## 8. Security (OWASP ASVS Level 2 as the checklist)

**Secrets and environments**
- Network secrets for build-time test keys, each locked to its vendor host; Railway variables for the running app.
- No plain environment variables for keys, and no keys in code, logs, fixtures, screenshots, PR text or chat.
- gitleaks and the PII scanner run on every PR, and any hit fails the build.
- Test and live are separate Railway projects with separate keys, databases, webhook endpoints and phone numbers. The live project has auto-deploy off.
- Deny rules in `.claude/settings.json` cover:
  - live key patterns (`sk_live`, `rk_live`);
  - Railway CLI or API deploy commands;
  - force pushes and pushes to `main`;
  - reading `.env*` files;
  - `curl` to live vendor hosts with credentials;
  - deleting the `claude/kit-state` branch.

**Authentication and sessions**
- **Homeowner quote page:**
  - an unguessable 128-bit token in the link;
  - the page shows no contact details;
  - picking requires a code sent to the homeowner's phone (Twilio Verify): 6 digits, 10-minute expiry, 5 tries, then a cooldown;
  - the code field allows paste and one-time-code autofill.
  - quote-page tokens stop working 30 days after the job closes.
- **Business portal:** sign-in by a one-time link emailed to the address on file (single use, expires in 15 minutes), plus an SMS code on a new device. Sessions last 30 days, and it can't release contact details without a confirmed pick.
- **Owner portal:** password (argon2id) plus a passkey or TOTP as the second factor. It's the only portal with pause, refund and release powers. Every action is audit-logged, and sessions are short.
  - **Recovery:** at least two passkeys or authenticators registered, plus one-time recovery codes that Casey keeps offline (in Bitwarden). There's no email-only recovery.
- **Cookies:** `Secure`, `HttpOnly`, `SameSite=Lax`, rotated on login. CSRF tokens on every form.

**Inputs and outputs**
- Every input is validated by a pydantic schema.
- Jinja autoescaping is always on, and homeowner and business text is always rendered as text, never as HTML.
- Strict security headers:
  - a CSP with no inline script (nonces where needed);
  - HSTS;
  - `frame-ancestors 'none'`;
  - `Referrer-Policy: no-referrer` on token pages;
  - `Permissions-Policy` locked down;
  - `X-Content-Type-Options: nosniff`.
- No third-party trackers or ad scripts. Fonts are self-hosted.
- Rate limits per phone, per IP and per business: request intake, code sends, sign-in links and quote submissions.
- **Webhooks:**
  - Twilio is verified with `X-Twilio-Signature`;
  - Stripe is verified with `Stripe-Signature` and a 5-minute tolerance;
  - both are idempotent by event ID, and unverified calls get a 403 and are logged.

**Photos**
- Optional, with size and type limits checked from the file bytes, not the name.
- Re-encoded, with EXIF and GPS data stripped. Stored privately and served only through short-lived signed links.
- Never sent to the AI unless needed, and never placed on job cards until zero retention is signed.

**AI safety**
- The AI receives only scrubbed text and returns only schema-validated fields.
- It has no tools, and nothing it says is executed. It can't add claims to a quote.
- Prompt-injection attempts in homeowner text are tested and must not change routing, fees or disclosures.

**Data protection**
- The contact vault holds names, phones and addresses with field-level encryption (AES-GCM). Its key is separate from the database credentials.
- Backups are encrypted, and access to the vault is audit-logged.
- **Retention (decided defaults):**

  | Data | Kept for |
  |---|---|
  | Homeowner contact details | Purged 180 days after the job closes, unless there's a dispute or complaint |
  | Job text | Kept with contact details removed |
  | Consent, opt-out and message logs | 5 years after the last message to that number |
  | Ledger and payment records | 7 years |
  | Photos | Deleted 30 days after the job closes |
  | Audit log | 7 years |

- Never collect SSNs, driver's licenses or tax ID images.

**Payments:** Stripe Elements or Checkout collects cards (PCI SAQ A), and card data never touches our servers.

**Supply chain:**
- the lock file is committed;
- pip-audit and bandit run on every PR;
- Dependabot alerts are on;
- the Docker base image is pinned by digest.

**Operations**
- Sentry with `send_default_pii=False` plus a scrubber.
- Logs carry request IDs only, never phone numbers.
- An incident runbook that includes Arizona's 45-day breach notice (A.R.S. 18-552).
- Safe mode pauses charges, releases and outbound texts if Casey hasn't acknowledged a High alert within 24 hours.

**Security testing**
- Unit tests for authorization on every route (who can see what).
- Tests that one homeowner's token can never read another job.
- Brute-force tests on codes, signature-forgery tests on webhooks and replay tests on idempotency.
- The ZAP baseline scan in Phase 2.
- The threat model in `docs/threat-model.md`, reviewed by Opus at K4 and in the final audit.

## 9. Accessibility (ADA): WCAG 2.2 Level AA

**Pages (site and all three portals)**
- Semantic HTML first; ARIA only where HTML can't do it. Every page has one `h1`, landmarks and a skip link.
- `lang="en"` or `lang="es"` on every page, and on any inline passage in the other language.
- Full keyboard use. Focus is always visible and never hidden behind sticky headers (2.4.11), with a logical order and no keyboard traps.
- Targets at least 24×24 CSS px (2.5.8). Nothing needs dragging or complex gestures.
- Contrast: 4.5:1 for text, 3:1 for large text and UI parts.
- Works at 320px wide and at 400% zoom without sideways scrolling, except data tables. Those scroll inside a focusable region with a label, per revision note 7.
- Text spacing can be overridden without breaking. Motion is reduced for users who ask for it.
- **Forms:**
  - visible labels;
  - instructions before inputs;
  - errors named in text, linked from an error summary and announced;
  - nothing the user already entered is asked for again (3.3.7);
  - sign-in needs no memory or puzzle tests, and pasting and autofill work (3.3.8).
- Live updates (new quotes arriving) are announced politely with `aria-live`, and focus moves to the result heading after a pick.
- **Time limits (2.2.1):**
  - only where security needs them: pick codes, sign-in links and owner sessions;
  - each warns before expiring and offers a one-step way to get a new code or extend;
  - anything already entered is kept.
  - Quote windows are business rules shown as plain text, not countdown pressure.
- **Error prevention (3.3.4) for money and agreements:** the homeowner's pick and the business's confirm-to-pay each have a review step showing exactly what happens and what it costs, with "Go back" before the final confirm.
- **Contact after a pick for deaf and hard-of-hearing users:**
  - at the pick, the homeowner chooses how the business should reach them: a call through the masked number (which works with 711 relay services), or email, which they choose to share;
  - the business portal shows that choice;
  - no phone number is ever shared.
- Help is in the same place on every page (3.2.6).
- Status banner and alerts use `role="status"` or `role="alert"` appropriately.

**Text messages and calls**
- Plain words, no meaning carried only by emoji, and full links.
- Spanish templates for every message type.
- The voice code call speaks slowly, repeats the code twice and offers "press 1 to hear it again."

**Emails:** real text, not images; a plain-text version; meaningful link text.

**Accessibility statement page:** the conformance target, known limits, and a phone and email contact for help, answered within 2 business days.

**Testing**
- axe-core on every page and state at 320, 768 and 1280px. Any violation fails the `a11y` check.
- Playwright keyboard-only journeys for all four end-to-end flows.
- Automated reflow and zoom checks.
- **The manual check Claude can't do:** at Gate B, Casey runs VoiceOver on his iPhone through the homeowner flow and the business card-entry step, using `docs/a11y.md`'s script. Stripe's card form is inside an embedded frame that axe-core can't scan, so it's checked by hand with the keyboard and VoiceOver. Claude records the results.

## 10. Compliance built in

**Every item in changes file sections 3, 7 and 7.1 is required.** For example:
- the content filter;
- users must be 18 or older;
- no arbitration clause;
- pest control is never advertised;
- moves leaving Arizona are refused;
- septic pumping goes only to ADEQ-licensed haulers;
- home watch never includes rent collection or leasing.

The list below is the minimum, not the whole set:

**Texting**
- **Opt-out words:** a text that is only *stop, quit, end, revoke, opt out, cancel* or *unsubscribe*, or the Spanish *parar, alto, cancelar, baja*, opts out of all texts. The confirmation explains START.
- Jobs close only with "CLOSE ####," the button on the quote page, or a longer cancel-intent message.
- The full consent text is the first message on every path.
- Quiet hours: 8 a.m.–8 p.m. in the stricter of Arizona time and the recipient's area-code zone (urgent safety texts excepted).
- Consent and opt-out logs kept for 5 years.
- Business consent by sign-up checkbox (E-SIGN), with version and timestamp.
- The identity voice call says "This is Ask Hava LLC" and gives a callback number.

**Wording**
- "Every eligible quote shown, in the order received." "Insurance (business-reported)."
- "Ask Hava is not a contractor and does not perform, bid on or supervise work."
- No "vetted," no "best," no rankings.
- Fees are called an "introduction fee by job size," never a commission.

**Routing**
- Licensing per the category list: pest, alarm, inspection and septic jobs go to licensees only.
- Handyman cap, split-job check, gas and permit block, and the disclosure line.
- Junk removal and garage doors are off at launch.
- The Spanish tag is shown only to businesses that ticked "We speak Spanish." The tenant flag reads "Landlord approval may be needed."

**Switches and terms**
- **Settings switches** (owner portal, logged): fee mode (tiers or flat per job type); texting on or off (web-and-email mode); category on or off.
- **Terms:** the homeowner terms (OK by text, plus "By choosing, you agree" on the quote page) and the business terms (card authorization, refund rules exactly as coded, non-discrimination, handyman certification) are drafted in Phase 1. A test checks that the refund rules in the terms match the code.

## 11. Testing strategy

| Layer | What | Gate |
|---|---|---|
| Unit and property | Fee calculation from the worked examples (including the $150/month → $600 → $75 case), scrubber, keyword matching, eligibility, quiet hours, state transitions | `ci` |
| Mutation | mutmut on `payments`, `catalog` and `licensing`: breaking the 4-month rule or the license filter must turn tests red | `ci` (nightly full run) |
| Integration | Real Postgres in the session; the job queue; outbox and inbox never double-send | `ci` |
| Contract | Each stand-in against the vendor's documented API; in Phase 2, the same tests against real sandboxes | `ci` and Phase 2 |
| Twin | The updated twin as the oracle. Any failed check makes the run fail (the old twin exited 0 despite failures); each pre-existing warning is explained in K5 | `twin` |
| End to end | The four journeys (homeowner, business, operator, vendor outage) in Playwright | `ci` |
| Security | Auth matrix, token isolation, brute force, webhook forgery, replay, PII and secret scan, bandit, pip-audit | `security` |
| Accessibility | §9 | `a11y` |
| Recovery | Restore drill with replay, no duplicate sends or charges | Phase 2 |

**Coverage:** the overall coverage may never drop, and money, privacy and licensing code must stay at or above 95% branch coverage.

## 12. Quality rules (no AI-made mess)

- **Task scope:** a task touches only the files it owns. Small PRs.
- **Code limits:** functions under 50 lines, and complexity limits enforced by ruff.
- **Strict checks:** `mypy --strict` clean. No `# type: ignore` without a reason comment.
- **No mystery code:** no commented-out code, TODOs or unused code on `main`. A cleanup task runs after every stage.
- **One source of truth:** `docs/architecture.md` and `docs/decisions.md` are updated in the same PR as the change they describe.
- **Fixing mistakes:** if something breaks `main`, revert it. Don't patch over it.
- **The CLAUDE.md file** stays under 200 lines and points here.

## 13. Models, context and pacing

**Models**
- **Haiku 5.5:** running tests and summarizing, scans, formatting, docs upkeep.
- **Sonnet 5.5:** most code.
- **Opus 5.5:** spec, architecture, money, privacy, security, recovery, reviews of those, and the final audit.
- **Fable:** not used.
- Models are named in each task. The runner's own session uses Sonnet at low effort.

**Context**
- One task per session, with each task's file list loaded only when needed.
- Long logs go to subagents.
- If a session compacts mid-task, the runner abandons the attempt and restarts fresh.

**Pacing**
- At full Max 20x limits, around the clock.
- At a usage limit, wait for the reset; never pay per use.
- K9 measures usage per task by model, and the schedule is set from that.

## 14. Environments and configuration

| Environment | Where | Data | Keys |
|---|---|---|---|
| Build | Cloud session VM | Fake only | Stand-ins in Phase 1; sandbox network secrets in Phase 2 |
| CI | GitHub Actions | Fake only | None (stand-ins only) |
| Test | Railway test project | Fake only | Test keys in Railway test variables |
| Live | Railway live project | Real | Live keys entered by Casey at Gate C; auto-deploy off |

All configuration comes from environment variables validated at startup. The app refuses to start if a live key appears in a test environment, or the reverse.

## 15. Handover pack (end of Phase 2)

- `docs/owner-guide.md`: a plain-English guide to how everything works and the daily and weekly checks.
- `docs/runbook.md`: outages, safe mode, refunds, disputes, complaints, STOP issues, the restore procedure and the incident and breach steps.
- How to change prices, add or switch off a category, and turn web-and-email mode on or off.
- Restore drill results; the final audit report; test results; the accessibility conformance notes and statement.
- `docs/account-checklist.md`, with Gate C's live-key checklist.

## 16. When Claude stops and asks Casey

Claude stops and asks Casey at each of these points, and at nothing else:

1. K3 questions, one at a time.
2. Approval of the category list and worked examples (K2b).
3. Gate A (approve the kit, then "OK go").
4. Any `BLOCKED` task.
5. The account checkpoint.
6. The go for the askhava.com page (Stage 0.5).
7. Gate B.
8. Gate C.
9. The first live charge.

**Conflicts between this file and the changes file:** stop, send an outbox message quoting both lines. Never pick one silently.

---

## Appendix A — the scheduled task (Casey creates it once)

At **claude.ai/code/routines → New task**:

- **Name:** `Ask Hava build runner`
- **Model:** Sonnet 5.5 (`claude-sonnet-5-5`)
- **Repository:** `ask-hava`
- **Cloud environment:** `ask-hava-build`
- **Connectors:** remove all of them (none needed)
- **Triggers:**
  1. **GitHub event:** Pull request → closed. Filters: base branch equals `main`; is merged equals `true`.
  2. **Schedule:** Custom, `7 * * * *` (hourly at 7 past the hour).

**Prompt** (paste exactly):

```
You are the Ask Hava build runner. Read BUILD_BRIEF.md sections 2, 7 and 16 now and follow them exactly.

Do exactly one cycle:
1. Fetch branch claude/kit-state and read kit/state.json. If it does not exist yet, the next task is K0 (Appendix B).
2. Read new files in inbox/ on claude/kit-state. Each is Casey's answer to an owner issue: record it in kit/state.json, clear that wait, and move the file to inbox/done/. To ask Casey anything, write a file to outbox/ (section 7); never post to GitHub directly.
3. If any task is MERGE_PENDING: if its PR merged with all four checks green on its head SHA, and main's squash commit has the same tree as that head, mark it DONE. If it failed, apply the failure rules in section 7. If it is still pending, update STATUS.md if stale and stop; never claim a second task while one is pending.
4. If the build is at a gate or WAITING_FOR_OWNER with no answer yet, or no task is eligible (BLOCKED tasks hold only their dependents), update STATUS.md if stale and stop.
5. Run the smoke test on main. If main is red, the next task is "fix main".
6. Claim the next eligible task with a lease (push to claude/kit-state; if the push is rejected, another runner has it, so stop). Renew the lease every 30 minutes.
7. Run the task through a worker subagent on the model named in the task, check the model and the changed paths, run the fresh-eyes reviewer subagent, then push claude/task/T###-<slug> so the auto-pr workflow opens the pull request with auto-merge.
8. Update kit/state.json (MERGE_PENDING), STATUS.md and PROGRESS.md on claude/kit-state, then stop.

Never break section 2. Never claim a test passed unless you ran it here.
```

## Appendix B — what the first sessions do (Phase 1 kit)

| Step | What happens |
|---|---|
| **K0** | Done in this order, in one PR:<br>1. **Deny rules first** (`.claude/settings.json`, §8).<br>2. Fingerprint `sources/` and write the order of authority.<br>3. Add the workflows:<br>• `ci`, `twin`, `security` and `a11y` start as stubs that pass on an empty app. Each triggers `on: push` for `claude/task/**` plus `pull_request`, has a job name exactly equal to the check name, and sets `permissions: contents: read`;<br>• `auto-pr` (`on: push`, `claude/task/**`);<br>• `nightly-main` (schedule);<br>• `backup-runner` (schedule, off by flag);<br>• `notify` (`on: push` to `claude/kit-state`; creates the `owner` label on first use);<br>• `inbox` (`on: issue_comment` and `issues: closed`; only the repository owner's comments are captured).<br>4. Create `claude/kit-state` **from the K0 task branch** (so it carries the workflow files), with the initial state, STATUS.md, PROGRESS.md, `outbox/` and `inbox/`. Each cycle, the runner merges `main` into `claude/kit-state` with a normal merge commit, so workflow updates reach it.<br>K0's own branch is `claude/task/K0-setup`, so its push starts the new workflows and its PR can merge.<br>6. After K0 merges, send an outbox message: "The four checks (ci, twin, security, a11y) have now run once. If you skipped adding them in PHONE_SETUP step 5, add them to the protect-main ruleset now." Until Casey confirms, no task is marked DONE unless all four checks show green on its PR. |
| **K0 also seeds the queue** | `kit/state.json` lists K1–K9 and Gate A as tasks, with dependencies, models and types (`harness` for K0 and K8), taken from the Kit Plan. K6 later adds the build tasks |
| **K1** | Write the constitution and CLAUDE.md |
| **K2** | Write the spec, one area per session |
| **K2b** | Write the category list and worked examples, then wait for Casey's approval |
| **K3** | Check for conflicts; ask Casey's questions one at a time |
| **K4** | Architecture, threat model and stack versions |
| **K5** | Test design; the twin made hard-failing |
| **K6** | Task graph |
| **K7** | Task prompts |
| **K8** | Harness, runner, stand-ins and contract tests |
| **K9** | Rehearsal with the three practice tasks; usage measured |
| **Gate A** | The one-page summary; Casey says "OK go" |

## Appendix C — running cycles by hand (phone, before the scheduled task exists)

If you can't create the scheduled task yet, the build can still run, one cycle at a time:

1. In the Claude app, open **Code** and start a new session.
2. Pick the repository `ask-hava` and the environment `ask-hava-build`.
3. Pick the model **Sonnet 5.5**.
4. Paste the Appendix A prompt exactly and send it.
5. When the session says it stopped, start another session the same way.

Each session does exactly one cycle, so starting one by hand is always safe. If a cycle is already running, the new one sees the lease and stops. The scheduled task simply automates step 5.
