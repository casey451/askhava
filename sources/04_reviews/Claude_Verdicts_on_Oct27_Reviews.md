# Claude's check of the October 27 reviews

Two outside reviews arrived on October 27, 2026. Claude checked each claim against official docs, statute text or a rerun before anything was changed. The resulting changes are in `01_spec/Changes_Since_v9.3_AUTHORITATIVE.md` (sections 7 and 8) and the updated `02_build_plan/` files.

## ChatGPT build stress test (`ChatGPT_Build_StressTest_Report.md`)

**Confirmed and adopted**

| Claim | How it was checked | Change made |
|---|---|---|
| Scheduled tasks run at most hourly; API triggers start a run on demand; at a usage limit, runs are rejected rather than queued | code.claude.com/docs/en/routines | The runner starts each next task through the API trigger, with an hourly run as a safety net; a "retry after reset" status |
| A run's "Succeeded" only means the session ended normally | Same page | Only the runner marks tasks done, from GitHub's tests on the exact commit |
| Network secrets are never attached to api.anthropic.com | code.claude.com/docs/en/cloud-environments | Stand-in AI during the build; real AI tested from the Railway test site |
| A "Username and password" secret type exists (Twilio basic auth) | Same page | Still prove it with one sandbox call in K8 |
| PostgreSQL 16 and Docker are preinstalled but not running; setup scripts over ~5 minutes aren't cached; running processes don't persist | Same page | Start the database in each session; keep setup short |
| GitHub Actions can run on the Max subscription via `CLAUDE_CODE_OAUTH_TOKEN` | code.claude.com/docs/en/github-actions | Backup runner uses it; no pay-per-use |
| A subagent's model can be overridden per invocation | code.claude.com/docs/en/sub-agents | Runner records and checks the model actually used |
| Auto mode can stop for a person after repeated blocks | Routines page, permission prompts | Rehearsed in K9 |
| A Railway restore creates a new database service | docs.railway.com PITR page | Restore drill includes the switchover |
| The twin reports failed checks but exits as a pass (`normal_fixed` seed 1: 6.0 "qualified business left out") | Rerun here, same number; also in our own earlier results | Failed checks fail the run; each warning explained in K5 |
| The prototype still uses 2 months for ongoing services | `flows.js` line 94 | Already revision item 2; one shared fee function plus worked examples |
| K9 contradicted "no app code before Gate A" | Kit plan text | K9 now uses practice tasks |
| The Stage 0.5 page needs its own go under the guardrails | Guardrail text | Separate approval added |

**Not verified:** the cap of 8 consecutive Stop-hook continuations (that part of the docs page didn't load). The fix doesn't depend on it.

**Estimates, not facts:** ChatGPT's 25% / 75% chances of success, 80–140 tasks, 3–8 weeks to build and 6–12+ weeks to launch. The plan now quotes these ranges and lets K9 measure the real pace.

## Grok legal research (`Grok_Legal_Research_Findings.md`)

**Checked directly**
- Twilio forbids "third-party lead generation" and shared consent, and a campaign rejected for it can't be resubmitted (Twilio forbidden-categories article; error 30951). → After reading the rule and the rejection causes, Claude judged Ask Hava doesn't fall under it (consent is first-party, nothing is shared or resold, the homeowner chooses the one business). Casey chose not to ask Twilio first. Registration waits for the full live site, uses careful wording, and has a web-and-email fallback.
- FCC 26-67 (Federal Register, Oct 26, 2026) makes stop, quit, end, revoke, opt out, cancel and unsubscribe standard opt-out words, effective 30 days after publication. → A text that is only "cancel" is an opt-out; jobs close with "CLOSE" plus the request number. *This conflict with our job-cancel word was found by Claude, not by Grok.*
- A.R.S. 32-1165 makes advertising regulated contracting without a license a misdemeanor. → Copy never implies Ask Hava does the work.

**Taken from Grok's quotes** (the source sites blocked direct access): A.R.S. 3-3615 (pest advertising presumption), 29-3201(G) (LLC publication), 41-1442 (public accommodation), 47 CFR 64.1200 details, CalOPPA, Stripe docs. These are low-cost changes, and the uncertain ones are on the attorney list.

**Where Claude disagreed**
- The $1,000 cap for unlicensed businesses in every category: not adopted. It applies to construction-type work only.
- Tenant tag: Grok sent it to the attorney; Claude dropped "skip tenant requests" now and uses a neutral landlord-approval flag.
- AI-reformatted quotes: show the original next to the summary rather than requiring every business to approve each summary.

**Formerly open:** all 18 questions now have working answers (changes file, section 7.1; `07_legal/Legal_Decisions_Record.md`). Attorney review is optional.
