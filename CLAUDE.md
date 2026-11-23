# CLAUDE.md — Laftly session rules

One line per rule; read the cited line, don't guess.

## Read first

1. `BUILD_BRIEF.md`: how to build; §2 and §7 at the start of every runner cycle (`BUILD_BRIEF.md:238`).
2. `docs/constitution.md`: every guardrail, gate and decision, sourced.
3. The changes file `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`: what to build; beats every source (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:7`).
4. `docs/sources.md` (authority, superseded rules, conflicts) and `docs/governance.md` (who changes what, checks, red `main`, progress) (D-479); `docs/ai-operating-manual.md`: start routine per session kind.
5. Your `kit/prompts/T###.md` and only the files it lists (`BUILD_BRIEF.md:245`).

## Order of authority

- Brief decides **how**, changes file **what** (`BUILD_BRIEF.md:7`); then v9.3 blueprint, twin, the rest (`docs/sources.md:30-32`). Conflict: decide, log both lines and the choice in `docs/decisions.md`, don't stop (`BUILD_BRIEF.md:539`).
- No superseded value (`docs/sources.md:34-53`): recurring **4 months** (not 2); fees **$25 / $75 / $199** (not $45/$59/$149); brand **Laftly**, Laftly LLC, laftly.com (not "Havasu Home Quotes" or "Ask Hava"; D-3347); no directory; no 4-per-trade cap.

## Rules that can't bend (BUILD_BRIEF.md §2)

1. No sign-ups, purchases, outreach, publishing, live messages or charges, production deploys without Casey's explicit go; never create accounts or accept terms for him (`BUILD_BRIEF.md:61`).
2. Never paste, print, log, commit or store secrets (chat, code, this file, plain env vars) (`BUILD_BRIEF.md:62`; `sources/02_build_plan/Build_Plan.md:165`).
3. No homeowner names, phones, addresses or photos in an AI tool that keeps data; **fake data only** in code, tests, fixtures, logs, screenshots (`BUILD_BRIEF.md:66`; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:263`).
4. Claim tested only when a tool run in this repo shows it (`BUILD_BRIEF.md:57`).
5. No live Railway project, keys or phone numbers before Gate C (`BUILD_BRIEF.md:68`).
6. Never weaken a test to pass; a wrong test changes only by a spec change logged in `docs/decisions.md` (`BUILD_BRIEF.md:69`).
7. Protected paths (`.github/`, `.claude/`, `runner/`, `stand_ins/`, `sources/`, `kit/layers.json`, `kit/allowed_crates.txt`, tests you don't own): only a `harness` task with Opus review (`BUILD_BRIEF.md:70`).
8. The locked blueprint (this file too; `docs/governance.md` section 1) is fingerprinted in `kit/plan.lock`: changing it needs a lock rewrite and a `docs/decisions.md` entry or `Ci` and `Governance` fail; a task branch changes only the locked file it owns (D-479). Plan wrong: say so in your report, never edit it to pass.

## Working rules

- Only on `claude/task/T###-<slug>` from latest `main`; on a retry, switch to the branch the runner prepared, never recreate, reset or rebase it (`BUILD_BRIEF.md:246`).
- Touch only your task's files; small PRs (`BUILD_BRIEF.md:485`). Never touch `claude/kit-state`; only the runner writes state (`BUILD_BRIEF.md:223`).
- Never push to `main`, force-push or rebase-and-force-push (`BUILD_BRIEF.md:224`, `BUILD_BRIEF.md:303`).
- Only the runner marks done, on `Ci`, `Twin`, `Ally` green on the merged commit (`BUILD_BRIEF.md:261-263`).
- Red `main`: the only task is "fix main"; revert, don't patch over (`BUILD_BRIEF.md:240`, `BUILD_BRIEF.md:490`).
- Usage limit: wait for the reset; never pay per use (`BUILD_BRIEF.md:509`).
- Casey answers only in `owner` issues via `inbox/`; to ask him, the runner writes `outbox/` (`BUILD_BRIEF.md:228-232`).

## Quality (BUILD_BRIEF.md §12)

- Stack fixed (`BUILD_BRIEF.md:101`): Rust (Axum, Askama, SQLx), PostgreSQL 16, a crate per module, committed `Cargo.lock`; TypeScript only for small browser extras, Python only for tooling, stand-ins, twin (D-453). New library: `docs/decisions.md` entry (purpose, license, cargo-audit) and cargo-deny pass (`BUILD_BRIEF.md:122`).
- Functions under 50 lines; clippy warnings denied; `cargo fmt --check` clean; `unsafe_code` forbidden; `#[allow(...)]` only with a reason (`BUILD_BRIEF.md:486-487`); no commented-out code, TODOs or unused code on `main` (`BUILD_BRIEF.md:488`).
- Update `docs/architecture.md` and `docs/decisions.md` in the same PR (`BUILD_BRIEF.md:489`).
- Document for the next session per `docs/code-docs.md` (crate `CLAUDE.md`, `//!`, `///`, requirement IDs on rule code); `scripts/check_code_docs.py` fails a gap (D-473).
- Coverage never drops; money, privacy, licensing code at least 95% branch (`BUILD_BRIEF.md:481`).
- This file stays under 200 lines (`BUILD_BRIEF.md:491`).

## Product rules most likely to be broken (details in docs/constitution.md)

- AI sees only scrubbed text via the Claude API (zero retention once signed; before, only under AI-008's switch, D-412); schema-validated fields, no tools, no added claims; stand-in AI during the build (`BUILD_BRIEF.md:343-344`; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:262`).
- No homeowner contact detail leaves the vault except on a confirmed, paid pick, and never the homeowner's phone (`BUILD_BRIEF.md:151`; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`).
- Texts: full consent text first on every path; a text that is only an opt-out word (English or Spanish, incl. "cancel") stops all texts; "cancel" alone never closes a job; quiet hours 8 a.m.–8 p.m. in the stricter of Arizona and area-code time; a Spanish template per text kind (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:113`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:158`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:159`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:184`; `BUILD_BRIEF.md:416`).
- Wording: "introduction fee by job size," never "commission"; never "vetted," "best" or rankings; "Laftly is not a contractor and does not perform, bid on or supervise work" (`BUILD_BRIEF.md:453-455`).
- WCAG 2.2 AA, any axe-core violation fails `Ally` (`BUILD_BRIEF.md:424`). OWASP ASVS L2; gitleaks and the PII scanner fail the build on any hit (`BUILD_BRIEF.md:298`).

## When Claude stops and asks Casey (BUILD_BRIEF.md §16)

Only at (`BUILD_BRIEF.md:546-552`): a hands-on task only a human can do; the account checkpoint; real texts to Casey's own phone in Phase 2; the laftly.com page go (Stage 0.5); Gate C and the first live charge; the war-games go (T243, D-1473). Gate B is Casey's click-through after "Finished". All else Claude decides and logs in `docs/decisions.md` and `docs/for-casey-review.md` (`BUILD_BRIEF.md:542`).
