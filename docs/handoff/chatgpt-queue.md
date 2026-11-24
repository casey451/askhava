# ChatGPT job queue (standing list for Casey)

Casey runs these in order whenever he has time: open a new ChatGPT chat, upload the zip, and say exactly "Read README_REVIEW_BRIEF.md first and follow it exactly." Then send the report back to the overseer.

The overseer keeps three or four jobs ready at a time, because older packages go stale as the code changes. It verifies every report on `main` before acting, removes finished jobs, and adds new ones. Each package follows `docs/handoff/ChatGPT_job_standard.md`: fake data only, and a PII scan and secret check before it goes out.

## Ready now

Nothing queued. The next jobs get built as their work comes near.

## Next, when they're near (not built yet)

- **Phase 2 first-wave pre-flight:** near the account checkpoint.
- **Flake diagnosis:** whenever a task fails twice on a test it doesn't touch, a log-plus-files diagnosis before its last attempt.

## Done

| Job | Outcome |
|---|---|
| PF-Q1 | T817 and T843 fixed (D-5793). |
| PF-T222, PF-CLK, PF-T801, PF-T801b | Filed; input to Fable's scope record. |
| PF-E2E-CLOCK | Verified; no further fix needed now (D-5809). |
| PF-PLAN-T801 | Draft received Nov 24; filed as `PF-PLAN-T801_draft.md` for the Thursday plan. |
| PF-PLAN-JOURNEYS | Draft received Nov 24; filed as `PF-PLAN-JOURNEYS_draft.md` for the Thursday plan. |
