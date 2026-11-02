# AI developer operating manual

**For:** any Claude session that works in this repository: the scheduled runner, a worker subagent, a reviewer, an audit, a plan-change session Casey starts, or a session a year from now that has never seen the code. **Adopted:** October 28, 2026 (D-479). Keep this file under 250 lines; detail lives in the files it points to.

The whole codebase is maintained by AI. There is no human programmer to ask. Everything you need is in the repository, and anything you cannot find there you must not invent.

---

## 1. Which kind of session are you?

| You are | Started by | Your instructions | You may change |
|---|---|---|---|
| **Runner** | the Actions runner (`backup-runner.yml`, as the Claude app) or, as safety net, the scheduled routine; both run `python3 -m runner` once D-486 lands (D-483) | `BUILD_BRIEF.md` sections 2, 7, 16 and Appendix A | `kit/state.json`, `STATUS.md`, `PROGRESS.md`, `outbox/` on `claude/kit-state`; nothing on `main` |
| **Worker** | the runner, for one task | `kit/prompts/T###.md` and only the files it lists | the task's owned paths and the shared paths, on `claude/task/T###-<slug>` |
| **Reviewer** | the runner, after a worker | the diff and the task's spec files only | nothing; you report findings |
| **Audit** | a task of type `audit` | its prompt | its report file |
| **Plan-change** | Casey, in chat, naming the change | this manual, `docs/governance.md` section 1 | the blueprint, with `kit/plan.lock` rewritten and a `D-###` entry |
| **Cold start** (after "Finished") | Casey, with a change request | `docs/README.md`, then this manual | whatever the change needs, through a pull request that passes every check |

If you are not sure which you are, you are a plan-change session with no plan change authorised: read, report, change nothing.

## 2. Start routine (every session, in this order)

1. `pwd` is the repository root; `git status` is clean; you are on the branch your role allows.
2. Read `CLAUDE.md`. Then `BUILD_BRIEF.md` section 2 (the rules that cannot bend) and section 7 or 16 as your role needs.
3. Read `docs/governance.md` section 1 (who may change what) and section 2 (what is machine-enforced).
4. Read `kit/state.json` and the last 30 lines of `PROGRESS.md` from `claude/kit-state` (read only). Note `current`, `main_red`, `build_hold`, any lease, and any `owner_waits`.
5. Run `bash scripts/smoke.sh`. If `main` is red, the only task is "fix main" (`docs/governance.md` section 5). Do not start anything else.
6. Confirm the task you were given is the next eligible task in `kit/state.json`. If not, stop and report.
7. Load only the files your task prompt lists. Do not read the whole repository "for context": that is how sessions compact mid-task.

## 3. Where things are

| Question | File |
|---|---|
| What is Ask Hava, and what must it never do | `docs/constitution.md`; `BUILD_BRIEF.md` section 1 |
| What exactly to build (numbered, one line each) | `docs/spec/<area>.md`; index in `docs/spec/index.md` |
| Why a rule is what it is | `docs/decisions.md` (search the `D-###` the requirement names) |
| Which source line a rule rests on | the `Source:` part of the requirement; `docs/sources.md` for the order of authority |
| Module boundaries, tables, layers, invariants, state machine | `docs/architecture.md` sections 3 to 6 (locked) |
| Pinned versions and approved crates | `docs/architecture.md` section 11 |
| How a thing is tested, at which layer | `docs/test-design.md` (search the `TD-` ID your task names) |
| What attacks the design must survive | `docs/threat-model.md` |
| How to write code and comments so the next session understands it | `docs/code-docs.md`; each crate's `CLAUDE.md` |
| How customer-facing words must sound | `docs/voice.md`, `docs/copy/copy-panel.md`, `docs/ai_smell.yaml` |
| The expertise behind a module (Rust patterns, Postgres operations, texting law and carrier rules, Stripe, accessibility, email delivery, what contractors want, the operating frameworks) | `docs/brains/` (index in `docs/brains/README.md`); your task prompt names the ones to read |
| The look of every page | `docs/design/DESIGN_SPEC.md` |
| The task graph and your task's place in it | `kit/tasks.json`; `python3 scripts/validate_tasks.py` |
| What is done, blocked, waiting | `kit/state.json`, `STATUS.md` on `claude/kit-state`; `python3 scripts/progress.py` |
| Open risks | `docs/risk-register.md` |
| Whether the build is allowed to proceed | `docs/build-readiness.md` |
| How the vendor fakes behave | `stand_ins/`, `tests/contract/` |

## 4. Rules you will be tempted to break, and why you must not

1. **Do not read `sources/` for a rule when a requirement exists.** The spec restates the sources; the sources contain superseded values (old fees, old brand, old caps). If a requirement looks wrong, log a decision; never build the source's old value.
2. **Do not edit a requirement, a test-design entry, a test you do not own, or the architecture's locked sections to make your task pass.** `Ci` fails the change without a decision and a lock rewrite, and the runner rejects the path. If the plan is wrong, say so in your report; the plan-change session fixes the plan.
3. **Do not add a crate, a feature flag, a `#[allow]`, a wildcard match on a status enum, or an `unsafe` block** without the decision entry each of them needs. The compiler, clippy and the architecture test will refuse most of these; the rest a reviewer sends back.
4. **Do not claim a check passed.** Paste the command and its real output in your report. "Should pass" is a failure.
5. **Do not put real names, phones, addresses, photos or keys anywhere**, including logs, fixtures, screenshots, pull-request text and this chat. Fixtures come from `tests/fixtures`.
6. **Do not touch `claude/kit-state` unless you are the runner.** Do not push to `main`. Do not force-push. Do not rebase a pushed branch.
7. **Do not start a second task, a research round, a simulation or a rehearsal** that your prompt did not ask for. Report what you think is needed; the plan decides.
8. **Do not stop to ask Casey** except at the stops in `BUILD_BRIEF.md` section 16. Decide by the order of authority, log it, keep going. A conflict you cannot place as "what" or "how" in one sentence follows `docs/governance.md` section 4.
9. **Do not narrate.** Your report is: files changed, commands run with real output, decisions logged, anything open, a two-line note for `PROGRESS.md`.

## 5. Doing a build task

1. Read the requirement lines and the `TD-` entries your prompt names, the crate's `CLAUDE.md`, and the `docs/brains/` files your prompt lists; cite the brain principle you applied in the pull request when a design choice rests on it. Read the cited source lines only to settle a doubt.
2. Write the tests first, from the `TD-` entries (behaviour, boundary, failure, invariant), each naming its IDs in a doc comment. Run them; they fail.
3. Write the smallest code that passes, inside your owned files. Functions under 50 lines. Only `api` is `pub`. Money through `payments`; vault plaintext through `privacy::api`; every state change writes an audit row; every outside action carries an idempotency key.
4. Document as you go (`docs/code-docs.md`): `//!` at the top of every file, `///` on every public item, requirement IDs on rule code, the crate's `CLAUDE.md` updated.
5. If you changed any words a homeowner or business reads, run `scripts/check_ai_smell.py` and `scripts/check_copy.py`, then the cold read and the copy panel as your prompt describes.
6. Run `bash scripts/smoke.sh` on your branch. Then `cargo test -p <your test crate>` for your `TD-` entries, and the twin if your prompt says so. Commit, then run `bash scripts/prehandin.sh` last (the `Ci` steps smoke does not run: the change rules against `main`, the PII scan, the npm and cargo audits); the runner runs the same gate at `--after` and holds or fails a red hand-in (D-1300).
7. Update `docs/architecture.md` (sections 7 to 12 only, unless you are a harness task with a decision) and `docs/decisions.md` in the same commit when behaviour or design changed.
8. Hand in: push `claude/task/T###-<slug>`; the `auto-pr` workflow opens the pull request. Do not mark the task done.

## 6. Doing a review

You see only the diff and the spec files named. Send the change back if any of these holds: it drifts from a requirement sentence; a test would still pass with the rule deleted; logic is duplicated; dead code, TODOs or commented-out code; a secret or personal detail; an accessibility regression; a wildcard arm on a status enum; a new dependency without a decision; a locked file changed without a decision and lock rewrite; customer copy that reads as machine-written. Report findings as file:line, what is wrong, what would fix it. One round of findings; if the second round still finds a blocker, the task fails its attempt.

## 7. Doing a plan change (Casey asked for it in chat)

1. Say in one sentence what will change and which layer (`docs/governance.md` section 1). If it touches `sources/`, stop: sources never change.
2. Branch `claude/<change-slug>` from `main`.
3. Make the change in the blueprint files. Keep requirement IDs stable; withdraw in place. A change to any value the runner writes into `kit/state.json` (a new key, a new string under `usage.source`) is checked against `kit/state_schema.json` before it merges: run `python3 scripts/check_state.py` over a state that carries the new value, because the runner refuses a state the schema rejects before it does anything (D-1156). `BUILD_BRIEF.md` and every other file the spec cites by line number must keep its line count and the cited lines in place (edit in place, pad or merge lines rather than insert or delete); `scripts/trace_spec.py` fails on a shifted citation. Update `docs/architecture.md`, `docs/test-design.md` and `kit/tasks.json` together so `validate_tasks.py`, `trace_spec.py` and `gen_prompts.py --check` stay green. Regenerate prompts with `python3 scripts/gen_prompts.py`.
4. Take the entry's number from the runner's counter first: `python3 -m runner --reserve-decision --reason "<change-slug>"` prints `reserved D-####`; never pick a number by hand (D-1058). If the session is denied the write to `claude/kit-state`, do not route around it: use the interim rule (D-1036's entry, widened in D-1064, D-1075 and D-1144: start = max(`next_decision`, main's highest entry + 1, the highest number on any open plan PR + 1) at the moment of numbering, then add two claim blocks (10); re-check both right before pushing, and open the PR promptly, because once it is open a claim reserves above its numbers, D-1144) and say so in the entry; D-1075 keeps this fallback on purpose, replacing D-1058's "last number chosen that way". Then append the `D-###` entry at the end of the file (question, choice, reason, source lines; name every requirement ID changed). Add the plain-language item to `docs/for-casey-review.md` when the change was Claude's judgement rather than Casey's instruction.
5. `python3 scripts/check_plan_lock.py --write`, then `python3 scripts/gen_prompts.py --check --strict` (every measured context budget is within its task's limit and every stored one within 3 points of its measurement, D-1036, D-1075; a `WARNING` that `docs/decisions.md` is past 75% of the PII scanner's limit means raising `MAX_BYTES` or splitting the file now, while there is room), then `bash scripts/smoke.sh`, then `python3 scripts/check_pr_change.py --base origin/main --branch <branch>`.
6. For an architecture change, run an independent Opus review of the diff before opening the pull request; fix what it finds; record the review in the decision entry.
7. Update `docs/risk-register.md` and `docs/build-readiness.md` if the change opens or closes a risk.
8. Push and open the pull request. The ruleset requires `Ci`, `Twin`, `Ally` green; nothing merges by hand.

## 8. Resuming after an interruption

A session can die at any moment (usage limit, timeout, compaction). Design every step so a fresh session finds the truth in the repository:

- the runner's lease expires on its own; a stale branch stays for evidence; a lost push means another runner won: exit;
- never hold work only in memory: commit to your task branch early and often (the auto-pr workflow only opens a pull request on the first push, and `Ci` runs on every push);
- if you find a task branch for your task that you did not create, read its last commit and its `Ci` result before deciding whether to continue it or start over; say which in `PROGRESS.md`. When the runner's hand-off says `branch_ready`, the runner made the branch for you (on top of an earlier attempt's pushed work, with `main` merged in, when there is one): switch to it and never create, reset or rebase it, because the runner never force-pushes and a branch rebuilt from `main` cannot be pushed (D-583);
- a task BLOCKED by something outside its own work (a rejected push, no head, a permission block, a missing stage proof, an unchecked merge) goes back to the queue only through `python3 -m runner --requeue T### --reason "..."` once its cause is fixed, never by editing `claude/kit-state`; it keeps its attempt count and waits while any lease is live (D-583). A task that used all its attempts because the plan was wrong (it could never pass as planned) goes back only after a plan change that fixes its `kit/tasks.json` entry or prompt, and adds its decision to the task's own `decisions` list, has merged with that decision, through `python3 -m runner --requeue T### --plan-fix D-### --reason "..."`: exactly one more attempt, on Opus, once per decision (D-677);
- with two lanes (`kit/runner.json` `"lanes": 2`) another session may be working a different task at the same time: a live lease on another lane is not yours to touch, and the runner, never you, merges `main` into a task branch that fell behind (D-620); after two harness-type failures in a row on lane 2 the runner keeps to one lane (`lanes_fallback`, an outbox item says why) until `python3 -m runner --restore-lanes` is run once the cause is fixed;
- if `kit/state.json` and the repository disagree (a task DONE whose files are not on `main`), stop and write an outbox message: that is a hands-on stop.

## 9. Reporting

End every session with exactly this, in this order: (1) task ID and branch; (2) files changed; (3) commands run and their real results; (4) decisions appended (`D-###`); (5) anything open, with the file:line it concerns; (6) two lines for `PROGRESS.md`. Nothing else.
