# Note for the after-phase-1 runner-fix batch: a short "retro" on every failed attempt

**Date:** Nov 25, 2026. **Casey's go:** "Yes", on the overseer's suggestion to note this for the runner-fix batch (PF-R2, `docs/handoff/PF-R2_runner_fix_design.md`).

## Why

- **T222's last attempt left no record of why it failed.** It spent about 48M tokens over 2.4 hours and ended BLOCKED.
  - `last_failure` still held attempt 2's text: that is the known `record_failure` BLOCKED-branch gap.
  - The job-log API does not return the worker step's output.
  - The work in progress was not pushed, because it changed `.github`.
- The overseer could not tell whether the worker hit attempt 2's test-clock gap, ran out of turns, or failed on something new.

## Idea

The idea comes from the `/retro` pattern in the open-source agent skills at aihero.dev. Only the idea is taken: no files, nothing installed.

- **What the worker writes:** at the end of every attempt that does not hand in green, a few lines saying:
  - what stopped it;
  - which file or decision would have saved the attempt;
  - whether it is a plan gap, a flake, a budget overrun or a code defect.
- **What the runner does with it:**
  - stores the note, capped, as the attempt's history row `failure_reason` (PF-R2 problem 2's optional field);
  - shows it under BLOCKED in `runner/status.py`.
- **Fallback:** when the worker ends without a report (turn or time limit), the runner records "no report: <session-end reason>", so the row is never empty.

## Scope

- This folds into PF-R2's second harness task, the per-attempt failure reason, as one more source for that field.
- It is not a new task under the freeze.
- Check it against the template's existing `PLAN GAP:`, `ALLY RED:` and `MAIN RED:` report lines, so the note never duplicates or contradicts them.

## More for phase-2 planning and the runner-fix batch (Casey: "Add in what you think is best")

These come from Matt Pocock's open agent-skills material (aihero.dev). They are chosen only where they answer a problem this build actually has.

Nothing here applies now, during the freeze. Each item names where it belongs.

1. **A turn cap that ends with the retro note (runner fix; the cheapest win).**
   - **Problem:** T222's attempt 3 started with a small context but ran 230 turns. Every turn re-reads the growing context, so it cost about 48M tokens.
   - **Fix:** when a worker passes a turn limit, it stops and writes the retro note above, rather than running on.
   - **Setting the limit:** take it from the runner's own usage data, for example the 95th-percentile turn count of DONE tasks of the same type. Do not use a borrowed number.
2. **Review findings must cite a rule or spec line, and review does not loop until clean (template plus runner).**
   - **Problem:** T222 and T122 ran out of attempts while the reviewer kept finding new wording points after the fix round.
   - **Blocking findings:** a finding blocks hand-in only if it cites a requirement ID, a decision, a `BUILD_BRIEF.md` rule or a failing check.
   - **Everything else:** it becomes a follow-up line in `docs/for-casey-review.md`, not a fail.
   - **Fix round:** the reviewer checks only what it flagged, not a fresh full review.
   - This keeps every money, privacy, consent and security rule enforced, because those all have IDs.
3. **Phase-2 tasks as vertical slices, not crate layers (planning).**
   - **Problem:** this week's plan gaps (T305, T801, T820, T119) were mostly a crate-shaped task needing a template, test or reader that it did not own.
   - **Fix:** plan each phase-2 task around one behaviour you can demo, and draw its ownership from that path through every layer. Ask of each task: "what can I demo when this is done?"
4. **Acceptance must fail at the start (pre-flight).**
   - Each pre-flight, outside or Opus, also checks that the task's named tests or acceptance would fail on today's `main`, and can be written inside the task's owned files.
   - That catches tasks that are already green and tests that can't be placed.
5. **Size tasks by expected total spend, not only by starting context (planning plus pacer).**
   - Starting context is already capped (about 15–50% of the window).
   - The long journey tasks (T222, T119, T532) are the ones that run away.
   - Split any task the pacer estimates above a few million tokens: one journey, or one step group, per task.
   - Let a worker hand broad, many-file searches to a cheap read-only helper and keep only its conclusion. Use this for searches only, not single lookups.

**Not adopted:**
- installing his skills;
- his doc layout (GLOSSARY.md, docs/agents/);
- his parallel-worktree build flow.
The runner, the crate guides and `docs/spec/wording.md` already cover those, with stricter gates.

## Pre-Gate-C quality plan: independent checks without a human engineer (Casey: "Do what you recommend")

**Context.** Casey has no access to a human engineer for an outside review, so the build must create its own independence.

**The main risk is shared blind spots, not sloppiness.** The same model family writes the code, the tests and the reviews. A misunderstanding can pass all three. T801 is an example: earlier tasks' tests contradicted CON-005 and passed review.

**Schedule:** after phase 1 and before Gate C, ahead of the account checkpoint where possible. Each item is planned as normal tasks or outside jobs under the usual gates. Nothing here starts during the freeze.

**Priority order** (cheapest, most mechanical and most aimed at the risk come first):

1. **Black-box tests written from the spec, not the code.**
   - **Who writes them:** an outside model (ChatGPT, or Grok when it has tokens) writes test cases only from `docs/spec/` requirement IDs and `docs/test-design.md`. It sees no code, no existing tests and no decisions.
   - **Start with the costly areas:** fees and charges (FEE, PICK), consent and texting (CON, TXT), privacy and the vault (PRIV), sign-in (SEC).
   - **How a harness task uses them:** it turns the cases into a new test suite and runs them. Each failure is triaged: a real bug becomes a fix task, and a spec gap becomes a decision for Casey, through Fable.
   - The test-weakening rule still applies: a new spec-driven test never replaces an existing one; it only adds to it.
2. **Mutation testing on money and consent code.**
   - **What it is:** a mutation tool (for example `cargo-mutants`) deliberately breaks the code, for example by flipping a comparison or deleting a check, and confirms a test fails.
   - **What a surviving mutant means:** a test that looks thorough but doesn't guard the rule.
   - **Scope:** `app/payments`, `app/picks` and the consent and opt-out paths in `app/messaging` and `app/intake`.
   - **Library approval:** the tool needs a `docs/decisions.md` library entry (purpose, licence, cargo-audit) and must pass cargo-deny.
   - **When it runs:** as a one-off audit task, not on every build, because it is slow.
   - **Output:** each survivor becomes a test-strengthening task.
3. **Outside reviews of the five risky areas** (money, consent and texting, privacy and the vault, sign-in and security, legal wording).
   - **Package:** each one goes out as its own zip under `docs/handoff/ChatGPT_job_standard.md`, holding only the spec and the code, never Claude's tests or decisions.
   - **Verification:** every finding is checked on `main` by Opus before acting, as now.
4. **Casey's own run-throughs.**
   - Gate B is already his click-through. Make it adversarial: as a homeowner and as a business, on his phone, try wrong numbers, STOP mid-flow, two picks at once, going back and forth, Spanish, and a slow connection.
   - A short checklist for this can be added to `docs/owner-guide.md` or the Gate B notes.
   - Some of this may need Casey's own hands-on stop; it is never delegated.
5. **War games (T243) aimed at the risky areas, then a small pilot.**
   - Part of T243 (Casey's go, D-1473) targets consent, fees and privacy.
   - After Gate C, launch first with a handful of friendly businesses and homeowners before going wide.
