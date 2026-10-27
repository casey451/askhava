# START HERE: stress-test the Ask Hava build plan

**From:** Casey Solomon (owner), with Claude · **Date:** October 27, 2026 · **Package version 5**

> **Version 5.** Updated after ChatGPT's build stress test and Grok's legal research, and after Casey's decisions that Twilio needs no pre-clearance and that all 18 legal questions get working answers now. Both reports, Claude's check of them and the legal decisions record are included. The build plan, kit plan and changes file reflect the fixes. A new reviewer should test the **revised** plan, especially:
> - the runner design;
> - the build order: everything built against stand-ins first, then an account checkpoint before the final build (changes file, section 8.1);
> - the Twilio determination and registration plan (changes file, section 7);
> - the 18 working legal answers (changes file, section 7.1);
> - whether anything in the ChatGPT or Grok reports was missed.
**For:** an independent reviewer (Grok, or a fresh Claude session that has not seen this work)

## What I'm asking

I'm about to have Claude build a whole business app on its own, in Claude Code cloud sessions, on a Claude Max 20x plan. Before that starts, I want to know **whether the build process will actually work**, and what will break it. Find the problems now, while fixing them is cheap.

The business itself has already been reviewed and simulated (Grok ran a full simulation of v9.3). **This review is about the build process**, not whether the business idea is good. Comment on the business only where it would make the build fail or produce the wrong thing.

"Successful" means all of these:
1. The finished app does what the spec says, proven by tests that were written from the spec before the code.
2. The build runs mostly unattended, at the plan's usage limits, pausing and resuming cleanly when limits reset.
3. No session runs out of context, loses its place or quietly uses the wrong model.
4. The code at the end is clean and consistent, not an "AI-made mess."
5. No guardrail is broken along the way (section "Rules that can't bend").
6. I know at every point where the build is and what it needs from me.

## The package

| Folder / file | What it is | Read it? |
|---|---|---|
| `START_HERE_StressTest.md` | This brief and the report template | **First** |
| `BUILD_BRIEF.md` | The file Claude Code reads first, every session: stack, runner, security, accessibility, testing, checkpoints and Casey's setup | **Core** |
| `02_build_plan/Kit_Plan_Before_Coding.md` | Steps K0–K9 that line everything up before any app code: constitution, numbered requirements, architecture, failing tests, task graph, task prompts, harness, rehearsal | **Core** |
| `02_build_plan/Build_Plan.md` | Accounts, how keys reach the build, which Claude features do what, session layout, stages 0–6 with gates A/B/C, pace/quality/model rules, decisions table | **Core** |
| `01_spec/Changes_Since_v9.3_AUTHORITATIVE.md` | Every change since v9.3. **Wins over all other files** | **Core** |
| `01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md` | Base technical spec (old brand name "Havasu Home Quotes") | Skim; it's long. Use it to judge spec size and test oracles |
| `01_spec/hhq_twin/` | The digital twin: a Python simulator of v9.3 (`app.py` ~ the app's rules, `twin.py`, `vendors.py` fake Twilio/Stripe/etc., 56 checks, 19 review tests, results) | Look at it; the plan makes it the test oracle |
| `03_business_and_design/` | Business Plan v9 (partly out of date, see delta), all-services expansion doc, brand brief, ChatGPT website prototype (34 pages) and its 8 revision notes | Reference |
| `04_reviews/Claude_Verdicts_on_Oct27_Reviews.md` | What Claude confirmed, adopted or rejected from the two Oct 27 reviews | **Core** |
| `04_reviews/ChatGPT_Build_StressTest_Report.md` | ChatGPT's stress test of the build process (v1 of this package) | Reference |
| `04_reviews/Grok_Legal_Research_Findings.md` | Grok's legal research with citations | Reference |
| `07_legal/Legal_Decisions_Record.md` | The 18 legal questions, the working answer now in effect and the fallback for each | Reference |
| `04_reviews/grok_v93_simulation/` | Grok's simulation report, code and summary results | Reference |
| `05_history/` | Original handoff START_HERE and earlier reviews | Only if needed |
| `06_briefs_sent_to_other_ais/` | The briefs given to Grok and ChatGPT earlier | Only if needed |

Note on dates: some documents say "Havasu Home Quotes." That was the working name; it is now **Ask Hava** (askhava.com). The doc exports replaced a diagram with a placeholder line ("embedded content"); the diagram showed lead session → builder sessions → reviewer/test subagents → CI → test environment.

## The plan in one page

- **Who builds:** Claude Code **cloud sessions** (claude.ai/code), running while my computer is off. Models: Opus 5.5 plans and reviews (~10%), Sonnet 5.5 writes code (~60%), Haiku 5.5 runs tests, scans and summaries (~30%). Fable is not used. A Sonnet task that fails twice gets one Opus attempt, logged.
- **Before any app code (the kit, K0–K9 plus K2b, ~30–60 sessions, est. 1–3 weeks):**
  1. sources copied in, with fingerprints and an order of authority;
  2. the constitution;
  3. one spec file per area, with numbered requirements (Given/When/Then);
  4. a checked category list and worked examples that I approve;
  5. a conflict check;
  6. architecture with state machines and file ownership;
  7. tests for behaviors, edges and failures, with failing twin checks failing the run;
  8. the task graph (~80–140 tasks);
  9. one prompt per task;
  10. the harness plus a real **runner**, with failure drills;
  11. a rehearsal of 3 practice tasks.

  I approve **Gate A**, then say "OK go."
- **The build (stages 0–6, est. 2–8 weeks with limit pauses):** setup → twin updated first → core → money → calls/checks/portals → website and owner tools → go-live prep. **Gate B:** I click through the test environment. **Gate C:** I enter live keys and say "go live." Going live also waits on outside steps (LLC, EIN, Twilio A2P 10DLC, insurance, TPT license, zero-retention agreement, Spanish review). Attorney review is optional.
- **How a session knows where it is:** `kit/state.json` (only the runner edits it), `kit/tasks.json` (only the runner changes status), `PROGRESS.md` (appended at the end of each session), and git history. Every session runs the same start routine (read state, last 30 lines of progress, last 20 commits, smoke test, confirm task matches) and end routine (tests + twin via Stop hook, fresh-eyes review subagent, commit, report, append progress, one-line status to my Claude Doc).
- **Context:** one task per fresh session; each task's measured budget ≤ ~20% of the context window; spec split into files loaded just in time; noisy work (logs, twin runs) in subagents; CLAUDE.md under 200 lines; rules that must survive compaction live in deny rules and hooks.
- **Pacing:** the runner starts the next task through the scheduled task's API trigger as soon as the last one merges, with an hourly scheduled run as a safety net. A **GitHub Actions schedule** using a subscription token is the backup. At usage limits, runs are rejected, so tasks are marked "retry after reset" (no pay-per-use). Only the runner marks a task done, from GitHub's tests on the exact commit.
- **Twilio:** Claude read Twilio's rules and judged the model isn't "third-party lead generation" (no pre-clearance request). Registration waits until the full site is live, with careful wording; if Twilio objects, the app runs on web and email. Public launch is estimated at 6–12+ weeks, gated mostly by the LLC and EIN, then Twilio's approval.
- **Keys:** test-mode vendor keys go into the cloud environment as **network secrets** (the proxy attaches them; sessions can't read them). Live keys go only into Railway service variables, entered by me at Gate C. Claude never holds a hosting key; the test service auto-deploys from GitHub.

## What to stress-test

Please work through every area. For each, the questions are a starting point, not a limit.

### A. Will the Claude tooling actually do what the plan assumes?
Verify each claim against **current official documentation** (code.claude.com/docs, support.claude.com, platform.claude.com) and cite the page. If you can't open a page, say so; don't answer from memory.
1. Cloud sessions: can a scheduled job start a new cloud session with a given prompt and model, unattended? What triggers exist (Routines, GitHub Actions, API)? Minimum interval? What happens when the plan's usage limit is hit mid-session?
2. Routines are a preview. If Routines disappear or change, does the GitHub Actions backup really work on a **Max subscription** without an API key (pay-per-use)? If it needs an API key, the "no pay-per-use" decision breaks. What's the right fallback?
3. Network secrets: which auth styles are supported? Twilio uses HTTP **basic auth** (Account SID + secret). Stripe uses a bearer key. Can each be injected by the proxy? Can a test inside the session call these vendors at all (network allowlist)?
4. The cloud environment: can tests run a real **Postgres** (setup script, apt, Docker)? How long may a setup script run, and is its result cached? Can Playwright/Chromium run there for browser and accessibility tests?
5. Hooks: do Stop, PreToolUse and **PreCompact** hooks all exist and behave as the plan uses them (Stop hook blocking the session from finishing until tests pass; PreCompact as a "context too big" tripwire)? Can a Stop hook loop forever on a test that can't pass?
6. Subagents with a **fixed model** in frontmatter: does that reliably override the session model? Can the runner set and then **verify** the model a session actually used?
7. Auto mode: could its safety classifier block routine build actions (git push to a branch, package install, running a local server), stalling an unattended run?
8. Worktrees for parallel builders, and the lead merging in order: what happens on merge conflicts with nobody watching?
9. Max 20x limits: is the per-5-hour and weekly usage known well enough to plan "2–8 weeks for the app build"? Does Opus use limits much faster than Sonnet? Does my own chatting share the pool?

### B. Will the kit (K0–K9) produce something buildable?
1. Is the spec (v9.3 + delta) complete and unambiguous enough to write Given/When/Then requirements without inventing behavior? Point to the spots where a builder would have to guess.
2. The twin is the test oracle. It's written in Python and models 5 trades and old fees. Is "update the twin first (Stage 1)" sound, or does it create a circular oracle (the same AI writes the twin and the app, so both share the same misunderstanding)?
3. Is 80–140 tasks at ≤20% context each realistic for this app (texting state machine, Stripe off-session charges, masked calls, three portals, 34-page bilingual site, owner tools, restore replay)?
4. Are the validators in each K step (traceability script, ownership script, task-graph checks) enough to catch a gap, or can a requirement slip through untested?
5. What's missing from the kit entirely? (Examples to consider: stack choice and versions, local fake vendors, seed data, migration strategy, error budget, logging/PII redaction tests, Spanish template QA.)

### C. Will the build loop stay on track unattended?
1. Walk through the first ~10 sessions in order (K0, K1, K2 ×n in parallel, K3 … then build T001–T00x). For each: what it reads, what it writes, what could go wrong, how it's caught.
2. Failure handling: a task fails twice → one Opus try → then what exactly? Does the queue stop, skip, or block dependents? Who notices, and how fast?
3. Flaky tests, a vendor sandbox outage, a test that's wrong rather than the code: how does the loop tell these apart without me?
4. Drift: how could code drift from the spec over 60 tasks despite the rules? Is the per-stage cleanup task and the final Opus audit enough?
5. State integrity: can two sessions ever race on `state.json` or `tasks.json`? Can a session mark a task passing without the tests passing?

### D. Context and model control
1. Is the "≤20% of context per task" budget measurable before a task runs? How?
2. What happens to a session that compacts mid-task anyway?
3. Is the model split (Haiku 30 / Sonnet 60 / Opus 10) sensible for this work? Where would Haiku produce bad results that cost more later?

### E. The critical path outside the code
1. Map the real dependency order: LLC (commercial agent in Maricopa County, no newspaper notice; 1–2 weeks) → EIN → Twilio Business Profile (72 h+) → A2P campaign (5–15 days, needs live site with privacy policy and opt-in page) → real-phone rehearsal; Stripe activation (EIN, bank, live site); zero-retention agreement (sales); attorney review; insurance; Spanish review. What's the earliest realistic go-live, and what in the build plan should move earlier to shorten it?
2. Stage 0.5 puts a simple askhava.com page up before A2P. Is anything else needed for Twilio's reviewers?

### F. Guardrails and security
1. Can any path in the plan lead to a live text, a live charge, a production deploy or an account sign-up without my explicit go?
2. Can a secret end up in a log, a commit, CLAUDE.md, a test fixture or chat?
3. Can homeowner data reach an AI that retains it, during the build (fixtures, logs) or in production?

## Rules for your review

- **Label every number and claim:** **[G]** Given in the package, **[A]** Assumed by you, **[M]** Measured by you (you ran something), **[D]** from official documentation (cite URL and the date you read it).
- **Say plainly whether you could run code.** If you couldn't, don't present anything as measured or tested.
- **Don't treat your findings as must-change.** Rank them, and say how confident you are in each.
- **Each finding needs:** a concrete failure scenario (step by step), the evidence, the fix, and the cost of the fix (small / medium / large).
- **Don't invent product features.** If you're not sure a Claude feature exists or works a certain way, say "unverified."
- Don't use real names, phones or addresses in examples.

## Rules that can't bend (my guardrails)

1. No sign-ups, purchases, outreach, publishing, live messages, live charges or deployments without my explicit go. AI never creates accounts or accepts terms for me.
2. Never paste or store secrets (passwords, API keys) in chat or plain files.
3. Never put homeowner names, phones, addresses or photos into an AI tool that keeps data; use request IDs.
4. Don't claim something was tested unless it actually was.

A fix that breaks one of these is not a fix.

## Report template

Please return one markdown file named `Ask_Hava_Build_StressTest_Report_<Reviewer>.md` with these sections:

```
# Ask Hava build plan: stress-test report (<reviewer>, <date>)

## 0. What I could do
- Could I run code? (yes/no, what)
- Could I open the official docs? (list pages read, with dates)

## 1. Verdict
- Will the build process succeed as planned? (Yes / Yes with fixes / No)
- Estimated chance of success as planned: __%; with my top fixes: __%  [A]
- The 3 things most likely to stop the build

## 2. Claude feature check (area A)
| Plan assumes | True? (Yes / No / Partly / Unverified) | Source [D] | Impact if wrong | Fix |

## 3. Findings, ranked
| # | Severity (Critical/High/Medium/Low) | Area (A–F) | Finding | Failure scenario (steps) | Evidence + label | Fix | Fix cost | Confidence |

## 4. Walk-through of the first ~10 sessions
| Session | Model | Reads | Writes | Ends when | What could go wrong | Caught by |

## 5. Critical path to go-live
- Ordered list with durations [G/A/D], earliest realistic go-live, what to start now

## 6. What's missing from the kit
- Bullets, each with why it matters

## 7. Timeline check
- Kit: plan says ~30–60 sessions, 1–3 weeks. Your estimate: __ and why
- Build: plan says ~80–140 tasks, 2–8 weeks at Max 20x limits. Your estimate: __ and why

## 8. Anything that conflicts with the guardrails or the decisions table
```

## Added in version 2: is Claude's Twilio call right?

Claude decided, from Twilio's published rules, that Ask Hava is not "third-party lead generation," and Casey chose not to ask Twilio first. Please test that reasoning hard: read Twilio's forbidden-categories article and error 30951 and the CTIA principles, then read the campaign description in changes-file section 7. Say whether a carrier reviewer could still reject it, and what wording or flow change would lower that risk. Do the same for the 18 working legal answers in section 7.1: flag any you believe is wrong, with a primary source.
