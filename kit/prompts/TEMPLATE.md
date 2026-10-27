# ${id}: ${title}

Task ${id} (${type}, phase ${phase}, stage ${stage}). Slug: ${slug}.
Branch: `${branch}`
Model: `${model}`, effort ${effort}. Reviewer: `${reviewer_model}` (areas: ${review_areas}).

${goal_section}## Start routine (do this first, every session)

1. Confirm the working folder is the repository root.
2. Read `BUILD_BRIEF.md` section 2 and section 7, and `CLAUDE.md`. Read `docs/constitution.md` for any rule you are unsure of.
3. Read `kit/state.json` and the last 30 lines of `PROGRESS.md` (both on `claude/kit-state`, read only) and the last 20 commits on `main`.
4. Run the smoke test. If `main` is red, stop and report: the only task then is "fix main".
5. Confirm this task (${id}) matches the next task in `kit/state.json`. If it does not, stop and report.
6. Create the branch `${branch}` from the latest `main`. Work only on that branch.
7. Load only the files listed under Scope below.

## Scope

Files you own (change only these, plus the shared paths below):

${owns}

Files you read (do not change):

${reads}

Shared paths any task may edit: `docs/architecture.md`, `docs/decisions.md`, `docs/for-casey-review.md`, `app/main.py`.

Size budget: at most ${budget_files} files, ${budget_lines} changed lines, ${budget_context}% of the context window. Keep the pull request small.

## What this task rests on

Requirements:

${requirements}

Tests that must pass (test-design entries and journeys from `docs/test-design.md`):

${tests}

Decisions (`docs/decisions.md`):

${decisions}

Source lines (cited; read them, do not guess):

${cites}

## Rules that cannot bend

- Fake data only in code, tests, fixtures, logs and screenshots. No real names, phones, addresses or photos.
- Never paste, print, log, commit or store secrets or keys.
- Never weaken a test to make it pass. A wrong test changes only through a spec change logged in `docs/decisions.md`.
- Protected paths (`.github/`, `.claude/`, `runner/`, `stand_ins/`, `sources/`, `twin/`, tests you do not own) change only in a `harness` task. This task is type `${type}`.
- No sign-ups, purchases, outreach, publishing, live messages, live charges or production deploys. Never use live keys, live phone numbers or the live Railway project.
- No TODOs, commented-out code or unused code. Functions under 50 lines.
- Wording: call the fee "introduction fee by job size" every time. Use no ranking or endorsement wording; `docs/constitution.md` lists the banned words. Fees are $$25 / $$75 / $$199; the recurring rule is 4 months; the brand is Ask Hava.
- If the brief and the changes file conflict, the changes file wins on what and the brief wins on how. Decide, log both lines and your choice in `docs/decisions.md`, and keep going.
- When your change touches design or behavior, update `docs/architecture.md` and `docs/decisions.md` in the same pull request.

## Definition of done

- Required checks pass on your commit: ${checks}.
- `ruff check`, `ruff format --check` and `mypy --strict` are clean for the code you changed (where the task has code).
- ${tests_done}
- Coverage does not drop; money, privacy and licensing code stays at or above 95% branch coverage.
- A fresh-eyes review of the diff (reviewer model above) finds nothing open.
- Only the files you own and the shared paths changed.

## End routine

1. Run this task's tests, the full suite and the twin.
2. Run the fresh-eyes review on the change and fix what it finds.
3. List the files you changed and the commands you ran with their real results. Do not claim a check passed unless a tool run in this repository shows it.
4. Include a short plain-English note for `PROGRESS.md` in your report (the runner writes it on `claude/kit-state`).
5. Do not mark this task done. Do not touch `claude/kit-state`. Do not push to `main` and do not force-push. The runner pushes, opens the pull request, and marks the task done only after `Ci`, `Twin` and `Ally` are green on the merged commit.
