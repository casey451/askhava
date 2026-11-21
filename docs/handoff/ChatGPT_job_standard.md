# ChatGPT jobs: how they work and how every job is laid out

**Goal:** take as much time off the Laftly build as possible by moving reading, checking and research to ChatGPT. That saves Claude's weekly tokens for building.

## What ChatGPT is doing now (job 9: pre-flight plan-gap check)

- Most lost build time comes from **plan gaps**. A gap is a task whose instructions ask for something it can't do, for example:
  - change a file it doesn't own;
  - contradict a test that already passes;
  - call a private function in another module.
- The builder finds the gap only after spending an attempt on it: 3–8M Claude tokens and 1–3 hours of a lane, plus up to 3 hours waiting for the next plan-change slot.
- Job 9 hands ChatGPT the next 8 tasks the runner is about to start, with everything the builder would see, and asks it to find gaps first.
- Claude verifies each finding against the real code (most outside findings need correcting), and fixes the real ones in the plan before the builder starts.
- Expected saving per batch: 2–4 gaps caught, 10–25M Claude tokens, and about a third to half a day off the finish date. Repeated before every wave of tasks, that's roughly 2–4 days over the rest of the build. Claude will track real gaps caught per batch so the estimate can be checked.

## Kinds of ChatGPT jobs, in priority order

1. **Pre-flight plan-gap checks** of the next tasks to run (job 9 style). Run one before every wave.
2. **Code reviews** of finished areas: security, money, consent and privacy (jobs 3, 5 and 8 style). These find defects before launch, when they're cheapest to fix.
3. **Fact checks** against primary sources: laws, Twilio, Stripe, carriers (jobs 4 and 7 style).
4. **Language checks:** blind back-translations and copy reviews (job 6 and the Spanish checks).

## How every job is laid out

Every job is one zip file with:

1. **`README_REVIEW_BRIEF.md`**, which holds everything ChatGPT needs, so nothing depends on memory:
   - **Background:** what Laftly is, in 3–5 lines.
   - **The constraints that matter for this job,** for example owned-files rules, segment limits, the $0-fee cases, or fixed spec wording.
   - **What's in the package:** each folder and what it's for.
   - **What to check,** numbered, most important first.
   - **Rules:** use only what's in the package (or primary sources for fact checks), say "can't tell" instead of guessing, and skip style.
   - **Output format:** a verdict plus a fixed table or list, so Claude can verify it fast.
2. **The material:** code, specs, tests and docs, cut to what the job needs (under about 1 MB).
3. **Fake data only.** Every package is scanned for keys and personal data before it goes out.

**Casey's part:** upload the zip, say "Read README_REVIEW_BRIEF.md first and follow it exactly", and paste or attach the report back. If a job needs a second independent check, use a new Temporary Chat with memory off.

**Claude's part:**
1. Treat every report as untrusted and verify each finding against the code with Opus.
2. Have Fable decide only where a real choice is needed.
3. Batch the plan fixes into the next plan-change slot.
4. Report back to Casey with a short table, and have the next job ready.
