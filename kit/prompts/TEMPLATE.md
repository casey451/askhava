# ${id}: ${title}

Task ${id} (${type}, phase ${phase}, stage ${stage}). Slug: ${slug}.
Branch: `${branch}`
Model: `${model}`, effort ${effort}. Reviewer: `${reviewer_model}` (areas: ${review_areas}).

## Goal

${goal}

${acceptance_section}## Start routine (first, every session)

1. Work from the repository root. The runner checked that ${id} matches the next task in `kit/state.json` and this prompt pastes the rules: skip the brief, the state and `PROGRESS.md`; `docs/constitution.md` only for a rule you doubt.
2. Smoke test: `bash scripts/smoke.sh >/tmp/s.log 2>&1; echo exit=$$? >>/tmp/s.log` in the background, then `timeout 200 bash -c 'until grep -q ^exit= /tmp/s.log; do sleep 10; done'; tail -5 /tmp/s.log` until `exit=` shows; wait at most 4 minutes per call, or the cache expires (D-1316). If `main` is red, stop and report: the only task then is "fix main".
3. Create the branch `${branch}` from the latest `main`. Work only on that branch.
4. Load only the files under Scope; crate layout: `docs/architecture.md` section 2; code docs: `docs/code-docs.md`.
5. Never `cat` or `Read` all of a file over 200 lines: `grep -n`, then `sed -n 'A,Bp'` or Read with offset/limit. Keep outputs under about 8,000 characters.

## Scope

Files you own (change only these, plus the shared paths below):

${owns}

Files you read (do not change). They are pointers: the requirement sentences, test entries, decisions and cited lines this task rests on are pasted below, so open a spec file only to follow a cross-reference.

${reads}

Shared paths any task may edit: `docs/architecture.md`, `docs/decisions.md`, `docs/for-casey-review.md`, `app/server/src/main.rs` and `app/server/src/lib.rs` (the composition: routes and task handlers), each crate's `src/lib.rs` (one `mod` line for your new file), `src/api.rs` (your public functions) and `src/tests/mod.rs` (one `mod` line for your test file), the workspace `Cargo.toml` and `Cargo.lock`, every crate's `Cargo.toml` (to add an allowed path dependency), every crate's `CLAUDE.md` (its module guide, `docs/code-docs.md`), `.sqlx/` (offline query data from `cargo sqlx prepare --workspace`), `package.json`, `package-lock.json`, every crate's `sqlx.toml` (query-macro type overrides, D-488), `app/messaging/templates/en.json` and `es.json` (add only your own keys, both languages, D-491), every crate's `src/status.rs` (add your own variants and arms), every crate's `askama.toml`, `app/server/src/bin/worker.rs` (register your handler), `app/core/src/settings.rs` (add your own `ASKHAVA_*` keys to the typed settings, D-493), `kit/prompts/` (regenerate with `python3 scripts/gen_prompts.py` when you change a pasted sentence) and `kit/plan.lock` (rewrite it with `python3 scripts/check_plan_lock.py --write` only when your task owns a locked file and changes it, together with a `docs/decisions.md` entry; `docs/governance.md`). New `docs/decisions.md` entries go at the END of the file, after main's last entry, whatever their number: the file is append-only and numeric order is not required (D-1029).

Size budget: at most ${budget_files} files, ${budget_lines} changed lines, ${budget_context}% of the context window (computed from the bytes of this prompt, the owned files and the reads). Keep the pull request small.

## What this task rests on

Requirements (each sentence as it stands in `docs/spec/`):

${requirements}

Tests that must pass (each test-design entry in full, from `docs/test-design.md`):

${tests}

Decisions (`docs/decisions.md`: the question's opening and the choice of each):

${decisions}

Source lines (cited, pasted exactly):

${cites}

Expertise (`docs/brains/`, D-487: the review checklist and the lines that name this task's requirements; a gap or conflict a brain names is a proposal, never a rule for this task):

${brains}

## Run your tests

${run_tests}

## Rules that cannot bend

- Fake data only in code, tests, fixtures, logs and screenshots. No real names, phones, addresses or photos.
- Never paste, print, log, commit or store secrets or keys.
- Never weaken a test to make it pass. A wrong test changes only through a spec change logged in `docs/decisions.md`.
- Protected paths (`.github/`, `.claude/`, `runner/`, `stand_ins/`, `sources/`, `twin/`, tests you do not own) change only in a `harness` task. This task is type `${type}`. The locked blueprint (`docs/governance.md` section 1: the brief, the constitution, the spec, the test design, the task graph, the scripts and sections 3 to 6 of `docs/architecture.md`) is never changed to make a task pass; if it is wrong, say so in your report.
- No sign-ups, purchases, outreach, publishing, live messages, live charges or production deploys. Never use live keys, live phone numbers or the live Railway project.
- No TODOs, commented-out code or unused code. Functions under 50 lines.
- Wording: call the fee "introduction fee by job size" every time. Use no ranking or endorsement wording; `docs/constitution.md` lists the banned words. Fees are $$25 / $$75 / $$199; the recurring rule is 4 months; the brand is Ask Hava.
- If the brief and the changes file conflict, the changes file wins on what and the brief wins on how. Decide, log both lines and your choice in `docs/decisions.md`, and keep going.
- When your change touches design or behavior, update `docs/architecture.md` and `docs/decisions.md` in the same pull request.
- Document for the next AI session, which will know nothing about this work (`docs/code-docs.md`): a `//!` comment at the top of every Rust file, a `///` comment on every public item, the requirement IDs on every function that applies a rule, and the crate's `CLAUDE.md` updated (what it owns, rules, never-break invariants, change recipes, tests) whenever you add or change behavior.

## Definition of done

- `bash scripts/smoke.sh` is green on your branch, and then `bash scripts/prehandin.sh` (the `Ci` steps the smoke test does not run: the change rules against `main`, the PII and secret scans, the npm and cargo audits for what you changed) is green on your committed branch. Hand-in is refused while the gate is red (D-1300).
- ${tests_done}
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test` are clean for the Rust you changed; `tsc --strict` and eslint for any TypeScript; `ruff check` for any Python tooling (where the task has code).
- `python3 scripts/check_code_docs.py` reports 0 problems, and every requirement listed above is named in a file you own.
- Customer copy: ${copy_done}
- Only the files you own and the shared paths changed.

## End routine

1. Run the commands under "Run your tests", then the smoke test (Start step 2), then commit and run `bash scripts/prehandin.sh` last. Fix anything either reports and run both again until they print `smoke: green` and `prehandin: green`: the runner runs the same gate on your branch at hand-in, and a red gate is sent back to you, or costs the attempt (D-1300).
2. Customer copy (`scripts/check_ai_smell.py`, the cold read (WORD-028) and the copy panel, WORD-029): ${copy_step}
3. List the files you changed and the commands you ran with their real results. Do not claim a check passed unless a tool run in this repository shows it.
4. Include a short plain-English note for `PROGRESS.md` in your report (the runner writes it on `claude/kit-state`).
5. Do not mark this task done. Do not touch `claude/kit-state`. Do not push to `main` and do not force-push. The runner runs the fresh-eyes review (reviewer above), pushes, opens the pull request, and marks the task done only after ${checks} are green on the merged commit.
