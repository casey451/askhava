# CLAUDE.md — Ask Hava session rules

Short rules loaded into every session. The full rules live elsewhere; read them, don't guess.

## Read first

1. **`BUILD_BRIEF.md`** — how to build (stack, runner, security, accessibility, testing, quality, stop points). Read §2 and §7 at the start of every runner cycle (`BUILD_BRIEF.md:238`).
2. **`docs/constitution.md`** — every guardrail, gate and decision, each cited to its source line.
3. **`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`** — what to build. It beats every other source (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:7`).
4. **`docs/sources.md`** — order of authority, the sources fingerprint and the superseded-rules list (its conflict line now states the decide-and-log rule, D-479).
5. **`docs/governance.md`** — who may change which layer of the blueprint, what a machine enforces, the red-`main` protocol, how progress is measured (D-479). **`docs/ai-operating-manual.md`** — the start routine and rules for every kind of session.
6. Your task prompt `kit/prompts/T###.md` and only the files it lists (`BUILD_BRIEF.md:245`).

## Order of authority

- `BUILD_BRIEF.md` decides **how**; the changes file decides **what** (`BUILD_BRIEF.md:7`).
- Then the v9.3 blueprint, then the twin, then everything else (`docs/sources.md:30`–`docs/sources.md:32`).
- If the brief and the changes file conflict: changes file wins on what, brief wins on how; decide, log both lines and the choice in `docs/decisions.md`, don't stop (`BUILD_BRIEF.md:539`).
- Never build a superseded value (`docs/sources.md:34`–`docs/sources.md:53`): no 2-month recurring rule (it's **4 months**), no $45/$59/$149 fees (it's **$25 / $75 / $199**), no "Havasu Home Quotes" brand (it's **Ask Hava**), no directory, no 4-per-trade cap.

## Rules that can't bend (BUILD_BRIEF.md §2)

1. No sign-ups, purchases, outreach, publishing, live messages, live charges or production deploys without Casey's explicit go. Never create accounts or accept terms for him (`BUILD_BRIEF.md:61`).
2. Never paste, print, log, commit or store secrets. No keys in chat, code, this file or plain env vars (`BUILD_BRIEF.md:62`; `sources/02_build_plan/Build_Plan.md:165`).
3. Never put homeowner names, phones, addresses or photos into an AI tool that keeps data. **Fake data only** in code, tests, fixtures, logs and screenshots (`BUILD_BRIEF.md:66`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:263`).
4. Don't claim something was tested unless a tool run in this repo shows it (`BUILD_BRIEF.md:57`).
5. Never use the live Railway project, live keys or live phone numbers before Gate C (`BUILD_BRIEF.md:68`).
6. Never weaken a test to make it pass; a wrong test changes only via a spec change logged in `docs/decisions.md` (`BUILD_BRIEF.md:69`).
7. Protected paths (`.github/`, `.claude/`, `runner/`, `stand_ins/`, `sources/`, `kit/layers.json`, `kit/allowed_crates.txt`, tests you don't own) change only in a `harness` task with an Opus review (`BUILD_BRIEF.md:70`).
8. The locked blueprint (the brief, this file, the constitution, the spec, the test design, the threat model, the task graph, the scripts, `docs/architecture.md` sections 3 to 6) is fingerprinted in `kit/plan.lock`; `Ci` and the `Governance` check fail any change without a lock rewrite and a `docs/decisions.md` entry, and a task branch may change only the locked file its task owns (`docs/governance.md` section 1, D-479). If the plan is wrong, say so in your report; never edit it to make a task pass.

## Working rules

- Work only on your task branch `claude/task/T###-<slug>`, created from the latest `main`; on a retry, the branch the runner prepared (the earlier attempt's pushed branch with `main` merged in, or a fresh `-a<n>` one): switch to it, never recreate, reset or rebase it (`BUILD_BRIEF.md:246`).
- Touch only the files your task owns. Small PRs (`BUILD_BRIEF.md:485`).
- Workers never touch `claude/kit-state`; only the runner writes state (`BUILD_BRIEF.md:223`).
- Never push to `main`, never force-push, never rebase-and-force-push (`BUILD_BRIEF.md:224`, `BUILD_BRIEF.md:303`).
- Only the runner marks a task done, from `Ci`, `Twin`, `Ally` green on the merged commit (`BUILD_BRIEF.md:261`–`BUILD_BRIEF.md:263`).
- If `main` is red, the only task is "fix main"; revert, don't patch over (`BUILD_BRIEF.md:240`, `BUILD_BRIEF.md:490`).
- Usage limit hit: wait for the reset; never pay per use (`BUILD_BRIEF.md:509`).
- Casey's answers come only from `owner` GitHub issues via `inbox/`; to ask him anything, the runner writes `outbox/` (`BUILD_BRIEF.md:228`–`BUILD_BRIEF.md:232`).

## Quality (BUILD_BRIEF.md §12)

- Rust (Axum, Askama pages, SQLx), PostgreSQL 16, Cargo workspace with one crate per module and a committed `Cargo.lock`; TypeScript only for small browser extras; Python only for tooling, stand-ins and the twin (D-453). The stack is decided (`BUILD_BRIEF.md:101`). New libraries need a `docs/decisions.md` entry with purpose, license and cargo-audit result, and must pass cargo-deny (`BUILD_BRIEF.md:122`).
- Functions under 50 lines; clippy with warnings denied and `cargo fmt --check` clean; `unsafe_code` forbidden; no `#[allow(...)]` without a reason (`BUILD_BRIEF.md:486`–`BUILD_BRIEF.md:487`).
- No commented-out code, TODOs or unused code on `main` (`BUILD_BRIEF.md:488`).
- Update `docs/architecture.md` and `docs/decisions.md` in the same PR as the change (`BUILD_BRIEF.md:489`).
- Document for the next AI session, which knows nothing: follow `docs/code-docs.md` (crate `CLAUDE.md`, `//!` and `///` comments, requirement IDs on rule code); `scripts/check_code_docs.py` fails the build on a gap (D-473).
- Coverage never drops; money, privacy and licensing code at or above 95% branch coverage (`BUILD_BRIEF.md:481`).
- This file stays under 200 lines (`BUILD_BRIEF.md:491`).

## Product rules most likely to be broken (details in docs/constitution.md)

- AI sees only scrubbed text through the Claude API (zero retention once signed; before that only under AI-008's switch, D-412), returns schema-validated fields, has no tools, adds no claims. Stand-in AI during the build (`BUILD_BRIEF.md:343`–`BUILD_BRIEF.md:344`; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:262`).
- No homeowner contact detail leaves the vault except on a confirmed, paid pick; the homeowner's phone is never shared (`BUILD_BRIEF.md:151`; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`).
- Texting: full consent text first on every path; a text that is only an opt-out word (English or Spanish, incl. "cancel") opts out of all texts; "cancel" alone never closes a job; quiet hours 8 a.m.–8 p.m. in the stricter of Arizona and area-code time; every text kind has a Spanish template (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:113`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:158`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:159`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:184`; `BUILD_BRIEF.md:416`).
- Wording: "introduction fee by job size," never "commission"; never "vetted," "best" or rankings; "Ask Hava is not a contractor and does not perform, bid on or supervise work" (`BUILD_BRIEF.md:453`–`BUILD_BRIEF.md:455`).
- Accessibility: WCAG 2.2 AA; any axe-core violation fails `Ally` (`BUILD_BRIEF.md:424`).
- Security: OWASP ASVS L2; gitleaks and the PII scanner fail the build on any hit (`BUILD_BRIEF.md:298`).

## When Claude stops and asks Casey (BUILD_BRIEF.md §16)

Only at: a hands-on task only a human can do; the account checkpoint; real texts to Casey's own phone in Phase 2; the go for the askhava.com page (Stage 0.5); Gate C and the first live charge (`BUILD_BRIEF.md:546`–`BUILD_BRIEF.md:551`). Gate B is Casey's own click-through after "Finished". Everything else Claude decides and logs in `docs/decisions.md` and `docs/for-casey-review.md` (`BUILD_BRIEF.md:542`).
