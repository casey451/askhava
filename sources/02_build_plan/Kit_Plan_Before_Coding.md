# Kit plan: everything lined up before coding

The kit is built in eleven steps (K0 to K9, plus K2b) in its own repository, by Claude Code cloud sessions, before a line of app code is written. It's done when a validator proves every requirement has a test and a task, every task fits its size budget, and a fresh reviewer finds no gaps. Then you approve Gate A and say "OK go."

## Principles

The kit copies two published methods: Anthropic's harness for agents that work across many sessions, and spec-driven development (constitution, then spec, then plan, then tasks, with consistency checks between them).

| Principle | Why | Source |
| --- | --- | --- |
| **A full, numbered list of what "done" means, written before any code** | Agents tend to declare a project finished early. A complete list, all marked failing at the start, prevents that | [Anthropic, Nov 2025](https://anthropic.com/engineering/effective-harnesses-for-long-running-agents) |
| **Machine-readable state in JSON, not prose** | Anthropic found the model is less likely to rewrite a JSON file by mistake. Only the runner changes a task's status, from GitHub's test result | Same |
| **One task per session** | Anthropic calls this "critical" to stop agents from doing too much at once | Same |
| **A fixed start routine and end routine every session** | Read the progress file and git history, run a smoke test before new work; commit and update notes at the end | Same |
| **Test like a user, not only with unit tests** | Agents marked features done after unit tests that missed broken screens; browser testing fixed it | Same |
| **Notes outside the context window** | Context is "a finite resource with diminishing marginal returns"; recall drops as it fills | [Anthropic, Sep 2025](https://anthropic.com/engineering/effective-context-engineering-for-ai-agents) |
| **Load files only when needed** | Keep file paths, open the file at the moment it's needed | Same |
| **Subagents for focused work** | Each has a clean context window and returns only a short summary | Same |
| **Constitution, spec, plan, tasks, with a consistency check before coding** | The check catches a plan that contradicts the principles, or tasks that skip part of the plan | [GitHub Spec Kit, via Microsoft Learn](https://learn.microsoft.com/en-us/training/modules/spec-driven-development-github-spec-kit-enterprise-developers/3-examine-github-spec-kit) |
| **A change goes back to the earliest affected step** | Spec, then plan, then tasks, then code are regenerated in order, so nothing drifts | Same |

## The steps

Each step ends with a check that a script or a fresh reviewer runs, not a judgment call. A step that fails its check is redone before the next one starts.

| Step | Produces | Tool | Model | Check that ends it |
| --- | --- | --- | --- | --- |
| **K0 Set up the kit repo** | Folder layout, `kit/state.json`, `PROGRESS.md`, `STATUS.md`, every source file copied in unchanged, and a source list with the order of authority: changes file, then v9.3, then tests, then the prototype and history | Claude Code cloud session | Sonnet | Every source present, with a fingerprint so any later edit shows; old rules (2-month pricing, old fees, old brand) listed as superseded |
| **K1 Constitution** | `docs/constitution.md`: your guardrails, the honesty rules, the legal gate, privacy, texting, the no-mess rules and every approval gate (including the separate go for the askhava.com page). A first CLAUDE.md under 200 lines | Cloud session, plan mode | Opus, high effort | Every guardrail and decision is cited to its source line |
| **K2 Requirements** | `docs/spec/`: one file per area. Every requirement numbered (for example REQ-MONEY-012), with Given/When/Then acceptance criteria and its source | One cloud session per area, one at a time | Opus | A traceability script: every source decision maps to a requirement or is marked "dropped by decision X" |
| **K2b Category list and worked examples** | A versioned list of the 25 launch categories: license rule, allowed and refused jobs, permit-type jobs, fee rule, source and review status. Plus worked examples for prices and matching (for example, a $150-a-month pool account counts as $600, so the fee is $75) | Cloud session | Opus | You approve one short table. Any category not confirmed stays switched off |
| **K3 Clarify and check for conflicts** | `docs/decisions.md` and a short list of real questions for you | A fresh reviewer per area, plus one cross-area reviewer | Opus | Zero unresolved conflicts. Any question goes to you one at a time, as a GitHub issue |
| **K4 Architecture** | `docs/architecture.md`: modules, tables, state machines (every waiting state has a timeout), message outbox and inbox that never send twice, which module owns which files, the test, staging and live environments and what each may touch, a data-flow and threat review, the stack and approved libraries | Cloud session, plan mode | Opus, high effort | Every requirement has one owning module; every outside action has a repeat-safe key |
| **K5 Test design** | Tests for each behavior, edge and failure, not one per requirement. The twin updated so any failed check makes the run fail (today it reports problems and still passes); the 6 "business left out" warnings explained first. Deliberate-break tests on money and license rules. Four end-to-end journeys: homeowner, business, you as operator, and a vendor outage | Opus designs, Sonnet writes, Haiku runs | Mixed | Breaking the license filter or the 4-month rule on purpose turns the tests red |
| **K6 Task graph** | `kit/tasks.json`: each task's goal, requirements, files it owns, spec files it reads, tests it must pass, dependencies, model, effort and size budget. Built as end-to-end slices with explicit integration tasks. Expect about 80–140 tasks | Cloud session | Opus | Script: no circular dependencies; every requirement and test covered; no two parallel tasks own the same file |
| **K7 Task prompts** | `kit/prompts/T001.md` and so on, from one template: start routine, scope, rules, definition of done, end routine | Cloud session | Sonnet, then a Haiku check | Every prompt names its files, tests and model |
| **K8 Harness and runner** | Settings with deny rules; hooks; helper agents; skills; CI; the **runner** (task leases, one merge at a time, marks done only from GitHub's test result on the exact commit, retry after usage reset, blocked tasks hold their dependents); a stand-in AI; a scanner that fails the build on real-looking names, phones, addresses or keys; stand-ins for every outside service (fake Twilio, Stripe's mock server, fake Resend and Lob, a stand-in AI) with contract tests written from each vendor's API docs; the account-checkpoint step that stops the runner and opens a "Your turn" GitHub issue | Sonnet builds, Opus reviews | Mixed | Failure drills all end safely: crash after a commit, a duplicate trigger, a failed test, a usage-limit rejection, a blocked permission |
| **K9 Rehearsal** | Three **practice tasks** (no app code) run through the real setup: started by the scheduler, cloud session, pull request, tests, merge, STATUS.md. One fails on purpose, one is triggered twice, one hits a simulated usage limit. Usage measured per model; `kit/schedule.json` set from it | Cloud sessions | Opus reviews | All three end correctly; the failing one is blocked and nothing merges |
| **Gate A** | A one-page summary and the K9 proof for you | This doc | — | You approve, then "OK go" |

## How Claude always knows where it is

Five records, each with one job. No session relies on memory from an earlier session.

| Record | What it holds | Who changes it |
| --- | --- | --- |
| `kit/state.json` | The current step or stage; each task's status (queued, claimed, running, checking, waiting to merge, done, retry after reset, blocked); attempts; model used; usage; last commit | Only the runner |
| `kit/tasks.json` | The full task list | Only the runner marks a task done, from GitHub's test result. Sessions hand in work; they never mark themselves done |
| `PROGRESS.md` | Plain-English notes: what was done, what's next, open issues | Each session appends at its end |
| `STATUS.md` | Your view: tasks done out of total, what's running, anything blocked and the one question it needs answered | The runner, after every merge |
| Git history | The truth of what changed and when | Every task ends in a pull request |

**Every session starts the same way** (written into every task prompt):

1. Confirm the working folder.
2. Read `kit/state.json`, the last 30 lines of `PROGRESS.md`, and the last 20 commits.
3. Run the smoke test. If anything is broken, fix that first and log it.
4. Confirm the task in its prompt matches the next task in `state.json`. If not, stop and report.
5. Load only the spec files and code files the task lists.

**Every session ends the same way:**

1. Run the task's tests, the full suite and the twin (the Stop hook sends the session back to work if they fail).
2. Run the fresh-eyes review subagent on the change.
3. Commit and open a pull request.
4. Append to `PROGRESS.md`.
5. Hand in to the runner. GitHub merges it automatically once the four required checks pass on that exact commit. The next runner marks the task done only after confirming the merged code on main is identical to the tested commit, then updates `STATUS.md` (BUILD_BRIEF.md, section 7).

**You can check any time** by asking any Claude session "status" (a skill reads the records and summarizes them), or by reading this doc. You get a phone notification at each gate and when the build finishes.

## Tool and model for each kind of work

| Kind of work | Tool | Model and effort |
| --- | --- | --- |
| Decisions with you | This chat, one question at a time | Opus |
| Writing the constitution, spec, architecture, test design and task graph | Claude Code cloud session in plan mode | Opus, high |
| Independent reviews | A subagent with a fresh context that sees only the artifact and the source | Opus, high |
| Drafting the area specs | One cloud session per area, run one after another | Opus, medium |
| Test skeletons, task prompts from the template, harness files, all app code | Cloud session | Sonnet, medium |
| Validation scripts, twin runs, linting, scans, summaries | Subagent | Haiku, low |
| Browser checks of portals and pages | Playwright inside the session (not your Chrome) | Haiku runs; Sonnet fixes |
| Scheduling the runner | One scheduled task with a GitHub trigger on merged pull requests plus an hourly run; a GitHub Actions schedule as backup (BUILD\_BRIEF.md, section 7) | — |
| Status for you | STATUS.md in GitHub (the official record), copied to this doc when possible | — |
| Operational lookups after launch | Cowork with Claude in Chrome | Sonnet |

Models and effort are fixed in each helper agent's settings and in each task, so nothing silently falls back to the most expensive model. A Sonnet task that fails twice gets one Opus attempt, and that step-up is logged.

## Size, timing and your part

- **Size:** about 30–60 sessions, more of them Opus than the coding build, because this is where the thinking happens.
- **Time:** about 1–3 weeks on Max 20x with pauses for usage limits. Our first estimate of 3–5 days left out the runner, the category list and the failure drills. K9 measures the real pace.
- **No app code is written until you approve Gate A.**

**Your part:**

1. Answer the questions K3 raises, one at a time, in the GitHub issues Claude opens (labeled "owner"). The build never sees chat.
2. Approve the category list and worked examples from K2b.
3. Nothing to set up yet: Railway and the other vendor accounts wait for the account checkpoint after the app is built.
4. Create the backup runner's subscription token yourself (`claude setup-token` on your computer) and paste it into the repo's GitHub Actions secrets. It never goes in chat.
5. Read the Gate A summary and the K9 proof, and say "OK go."

**Where the kit lives:** a private GitHub repository. The Claude GitHub App installed, plus the cloud environment with its network secrets, is all the build needs. The accounts you create before K0 are GitHub, the Claude Max plan and the cloud environment. No vendor keys are needed for the kit or for Phase 1 of the build; they're added at the account checkpoint, after the app is built.

**Sources:** [Anthropic: effective harnesses for long-running agents](https://anthropic.com/engineering/effective-harnesses-for-long-running-agents) · [Anthropic: effective context engineering for AI agents](https://anthropic.com/engineering/effective-context-engineering-for-ai-agents) · [Microsoft Learn: GitHub Spec Kit](https://learn.microsoft.com/en-us/training/modules/spec-driven-development-github-spec-kit-enterprise-developers/3-examine-github-spec-kit) · [Claude Code best practices](https://code.claude.com/docs/en/best-practices) · [Claude Code subagents](https://code.claude.com/docs/en/sub-agents) · [Claude Code hooks](https://code.claude.com/docs/en/hooks)
