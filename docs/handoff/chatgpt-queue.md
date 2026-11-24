# ChatGPT job queue (standing list for Casey)

Casey runs these in order whenever he has time: open a new ChatGPT chat, upload the zip, and say exactly "Read README_REVIEW_BRIEF.md first and follow it exactly." Then send the report back to the overseer.

The overseer keeps three or four jobs ready at a time, because older packages go stale as the code changes. It verifies every report on `main` before acting, removes finished jobs, and adds new ones. Each package follows `docs/handoff/ChatGPT_job_standard.md`: fake data only, and a PII scan and secret check before it goes out.

## Ready now

| # | Zip | What it's for | Prepared |
|---|---|---|---|
| 1 | `Laftly_PF-PLAN-T801.zip` | Drafts the plan entries for the T801 chain (web chat runs the whole request on screen with no texts), from Fable's scope record and both earlier reviews. Thursday's session checks the drafts instead of writing them. | Nov 24, 12:00 pm MST, `main` e2c7ee73 |
| 2 | `Laftly_PF-PLAN-JOURNEYS.zip` | Drafts the plan entries for the journeys chain (the narrow clock fix, the test-data helper, the J1 and J2 core journeys, two variant tasks; T532 and T533 adjusted). | Nov 24, 12:00 pm MST, `main` e2c7ee73 |

## Next, when they're near (not built yet)

- **Phase 2 first-wave pre-flight:** near the account checkpoint.
- **Flake diagnosis:** whenever a task fails twice on a test it doesn't touch, a log-plus-files diagnosis before its last attempt.

## Done

| Job | Outcome |
|---|---|
| PF-Q1 | T817 and T843 fixed (D-5793). |
| PF-T222, PF-CLK, PF-T801, PF-T801b | Filed; input to Fable's scope record. |
| PF-E2E-CLOCK | Verified; no further fix needed now (D-5809). |
