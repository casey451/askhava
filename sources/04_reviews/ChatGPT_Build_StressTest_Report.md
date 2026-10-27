# Ask Hava build plan: independent stress-test report (ChatGPT, October 27, 2026)

**Scope:** Independent audit of the *build process*, not a fresh market-demand forecast. Reviewed the package's controlling delta, K0–K9 kit, build plan, old technical blueprint, digital twin and regression tests, selected website prototype code, prior simulation material, and current official vendor documentation. No accounts, external environments, customers, business records, live services, or hosted projects were accessed or changed.

**Evidence legend (used throughout):** **[G]** expressly stated in the uploaded package; **[M]** independently measured here in a local offline run or direct source inspection; **[D]** verified in public *official* documentation on **October 27, 2026** (linked at end); **[A]** analyst estimate, design advice, extrapolation, or unverified live behavior. A simulation finding is **[M: simulation]**, not a verified defect in a production app.

## 0. What I could do

- **[M]** Unpacked the 121-entry ZIP archive (99 files). Examined `START_HERE_StressTest.md`, `01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`, `02_build_plan/Build_Plan.md`, `02_build_plan/Kit_Plan_Before_Coding.md`, the v9.3 blueprint, the twin README and source, and relevant prototype JavaScript/README. Inspected selected historical reviews for context without allowing them to override the October 27 delta.
- **[M]** Executed `python3 review_tests.py` in `01_spec/hhq_twin/`: **19 of 19 v9.3 targeted regression cases passed**; the process exited **0**. These are tests of the *old simulated application*, not proof that the future implementation will work.
- **[M: simulation]** Executed `python3 twin.py normal_fixed 1`: process exited **0**, but the scenario output flagged **6.0 occurrences/run of `qualified_business_left_out`**. Executed `python3 twin.py vendor_chaos_fixed 2`: process exited **0**, but flagged **4.5 occurrences/run of `qualified_business_left_out`** and **3.5 occurrences/run of `job_card_outside_license`**. The decimals are *average occurrences per simulated run*, not percentages or real-world rates. The test runner's exit code does not treat these scenario-level failures as a failing command.
- **[M]** Ran `node --check` on `flows.js`, `site.js`, and `sample-data.js`: all passed **syntax** checking, not browser or accessibility testing. Inspected the prototype fee expression and found `kind.value==='monthly'?a*2:a`, conflicting with the authoritative **four-month** recurring-fee rule. This is a **prototype defect**, not evidence the unbuilt server has one.
- **[D]** Successfully read current official Claude Code documentation: cloud scheduled tasks, cloud environments, GitHub Actions, hooks, subagents, model configuration, auto mode, cloud sessions, and Claude Max usage. Also read Twilio A2P and Railway PITR documentation. Links in Section 9.
- **[A] Unperformed:** I could **not** run an authenticated Claude Max cloud session, register a Routine/API trigger, create an actual GitHub Action using a Max OAuth token, verify the user's availability/quotas, configure network secrets, inspect Railway's Backups tab, run a real Postgres/Stripe/Twilio/Lob end-to-end test, check an attorney's conclusions, or use a real device to test the UI. Those require owner-controlled environments and approvals. Claims about behavior in *this user's* cloud setup remain unverified until the rehearsal.

## 1. Verdict

**Will the process succeed as planned? NO, not reliably in the exact form written.** The architecture-first, test-first and gated-launch ideas are reasonable, but the orchestrator has no proven failure/retry discipline; its completion signal can be falsely green; the test oracle already emits unmatched invariants; and some hardcoded data and guardrails disagree with the intended authoritative decisions.

- **Estimated success of an unattended, correct, fully tested app on the original schedule, without changing the plan:** **25% [A]** (subjective engineering judgment, not a probability inferred from measured outcomes).
- **Estimated success after the priority fixes and a passing 3-task end-to-end rehearsal:** **75% [A]**, provided a human remains available for blocked tasks, Gate A/B/C decisions and external approvals. This is *not* a forecast of business success, vendor acceptance, or launch date.
- **Top three stoppers:** (1) the queue does not reliably and safely resume tasks after subscription limits, agent failures or event delivery gaps; (2) a test run or session can report success despite rule violations, incomplete work, or a skipped integration; (3) the simulator, old specs and prototype contain conflicting or incomplete rules that can yield a green but wrong build.

**Launch-readiness decision:** **No-go on Gate A until the mandatory items under Section 3 are demonstrably fixed.** No independent reason found to abandon Claude Code or the project itself. An autonomous workflow should be a supervised, fail-closed build factory—not an unrestricted agent with a prompt to keep going.

## 2. Claude feature check (Area A)

| Assumption in plan | Verdict | Evidence | Impact if wrong | Required treatment |
|---|---|---|---|---|
| Cloud sessions run after laptop closes and can push branches | **Yes [D]** | [Cloud sessions](https://code.claude.com/docs/en/claude-code-on-the-web) | Low | Use dedicated private repo and short independent sessions. |
| A saved scheduled task can invoke a prompt and a selected model | **Yes [D]** | [Cloud scheduled tasks](https://code.claude.com/docs/en/routines) | Low | Pin model in task configuration; record actual model in job evidence. |
| Routine starts the **next job immediately after** the prior job | **Partly [D]** | [Scheduled triggers](https://code.claude.com/docs/en/routines): cron cannot exceed **once/hour**; API and GitHub events can fire on demand | **Critical** | Use an authenticated API trigger or an event-driven dispatcher; retain hourly poll as a recovery safety net, not the main worker trigger. |
| Routines are stable and guaranteed | **No [D]** | Same page calls them **research preview** | High | Isolate trigger adapter; rehearse manual resumption/fallback. |
| GitHub Actions fallback **must** incur API metered billing | **No [D]** | [GitHub Actions](https://code.claude.com/docs/en/github-actions) supports `CLAUDE_CODE_OAUTH_TOKEN` generated by the **owner** via `claude setup-token` | High | Max subscription fallback is *technically documented*; store token as a GitHub Actions encrypted secret, and rehearse token validity, usage-stop behavior and privileges. Do **not** accidentally select API-key mode. |
| A failed or quota-rejected run is auto-replayed | **Unverified; do not assume [D/A]** | [Scheduled tasks usage](https://code.claude.com/docs/en/routines) says new runs without credits are **rejected until reset**; it does not promise a replay of each missed task | **Critical** | Durable task ledger + periodic dispatcher that retries eligible jobs after reset. |
| Max 20x means a known number of sessions/agent-hours | **No [D]** | [Max plan](https://support.claude.com/en/articles/11049741-what-is-the-max-plan), [Code on Max](https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan) | High | Determine cadence only from logged real usage; avoid a fixed 1–2-week commitment. Usage is shared with interactive Claude. |
| Network secrets are available to Pro/Max and hidden from VM | **Yes, with caveats [D]** | [Cloud environments](https://code.claude.com/docs/en/cloud-environments) | High | Secret host scoping is useful, but run explicit vendor reachability probes; **`api.anthropic.com` never receives user-added network secrets**. |
| Both Stripe bearer and Twilio basic auth definitely work out of the box | **Partly / Twilio unverified [D/A]** | Cloud environments document Bearer/custom header and other secret types including username/password in a simplified form | High | Prototype *each* authenticated request, including method/header, before K8. Do not treat a saved secret as proven working auth. |
| Cloud VM can host Postgres, Docker, tests | **Yes [D]** | [Cloud environments](https://code.claude.com/docs/en/cloud-environments): PostgreSQL 16, Docker; ~4 vCPU/16 GB/30 GB; services not running by default | Medium | Start Postgres every session; pin dependencies; run migrations and DB tests against ephemeral local service. |
| Cloud setup script can do any amount of setup and persists running DBs | **No [D]** | Same: setup must finish in ~5 minutes to be cached; only files/images persist, **not processes** | Medium | Cache installs in setup, start databases from SessionStart/test harness. Plan Playwright install separately and measure reliability. |
| Stop, PreToolUse and PreCompact hooks all exist | **Yes [D]** | [Hooks reference](https://code.claude.com/docs/en/hooks) | Medium | Use them, but do not treat Stop as a CI guarantee. |
| Stop hook always blocks the job from finishing until tests pass | **No [D]** | Stop hooks can be skipped on interruption/API failure; continuation has an **8-consecutive-continuation cap** reset by tool calls | **Critical** | Hard pass/fail only in external CI + runner verification; Stop is fast feedback. Use StopFailure and runner heartbeat. |
| PreCompact can always stop context compaction cleanly | **No [D]** | [Hooks reference](https://code.claude.com/docs/en/hooks): blocking compaction after a context-limit API error may cause the request to fail | Medium | Save checkpoint first; fail task and restart fresh rather than depending on a noncompacting session. |
| Subagent frontmatter guarantees the model | **Partly [D]** | [Subagents](https://code.claude.com/docs/en/sub-agents): per-invocation model can override frontmatter; disallowed models may fall back | High | Use explicit model IDs, version pin, model allowlist, and verify observed model in run metadata/task panel; hard-fail unacceptable substitutions. |
| Auto mode has no unattended surprises | **No [D]** | [Scheduled-task permissions](https://code.claude.com/docs/en/routines), [Auto mode](https://code.claude.com/docs/en/auto-mode-config): repeated blocks may prompt for a human | Medium | Define trusted development destinations *outside the repo*; rehearse real package installs, PR pushes and server startup. Never suppress critical denies. |
| Worktrees alone prevent conflicts | **No [A]** | Isolation prevents same-folder clobbering, not merge/rebase or shared-state races | High | Merge queue + head-SHA compare + conflicts become blocked tasks for review. |
| Playwright and axe automatically prove WCAG 2.2 AA | **No [A]** | Browser automation and static scans cover only a subset of accessibility | Medium | Install browser, run at 320px, 200% zoom, keyboard, screen reader and manual Gate B acceptance. |
| Claude Doc can always be updated and phone notified from cloud | **Unverified [A]** | No configured connector or notification route supplied in package | Medium | Treat GitHub `STATUS.md`/PR checks as truth; use an optional connector only after testing. |

### Immediate implementation implication

**[D/A]** The strongest path that preserves **subscription billing only** is: owner-configured Max-backed Claude Code scheduled task with an **API trigger** (preview) + a simple, token-protected dispatcher; optional GitHub Action authenticated using the owner's Max OAuth token; an hourly reconciliation task if preview APIs misfire. Restrict its powers to non-production repositories, and **fail closed** if all three fail. The fallback is *not* verified simply because docs describe it.

## 3. Findings, ranked

**Severity definitions:** **Critical:** may permit false success, unauthorized release/charge, or a dead build; **High:** likely major rebuild, data mismatch, or stalled pipeline; **Medium:** schedule/quality drag or narrower failure; **Low:** polish. Confidence indicates confidence in the *finding* given evidence, not probability it will occur.

| ID | Severity / Area | Finding and concrete scenario | Evidence | Fix | Cost / confidence |
|---|---|---|---|---|---|
| **F01** | **Critical / A,C** | **No durable event-driven scheduler and quota recovery.** Last worker finishes; hourly Routine misses next-task latency or API fire is rejected at limit; scheduler marks nothing; build remains idle across reset. | [G] `Build_Plan.md` pacing; [D] Routines max hourly cron, rate/usage rejection | Create an idempotent dispatcher triggered by **completion** and polled hourly; atomic claim lease; exponential retries with quota-aware not-before; alerts. | **Medium / High** |
| **F02** | **Critical / C,F** | **A green session is not a green task.** Worker exits normally with test failure, Stop cap reached, blocked API request or missing browser check; runner trusts transcript/`passes: true`, merges broken branch. | [D] [scheduled outcomes](https://code.claude.com/docs/en/routines) and [Stop hooks](https://code.claude.com/docs/en/hooks); [G] sessions can flip `passes` | Only the runner can mark success after checking CI check-runs for the **exact commit SHA**, artifact hashes, oracle invariants, review status and stage gates. Make `tasks.json` immutable during execution. | **Medium / High** |
| **F03** | **Critical / B,C** | **The current oracle reports real violations while returning shell success.** K5 copies twin as-is; CI exits 0; app goes green with missed eligible contractors or over-broad licensed matches. | [M: simulation] `normal_fixed` 6.0 misses/run, `vendor_chaos_fixed` 4.5 misses and 3.5 outside-license/run, exit 0 | Investigate against `app.py`, expected truth, original scenarios; resolve or document and reject each baseline warning; make any **non-waived invariant fail CI**. Add exact sample scenario reproduction. | **Medium / High** |
| **F04** | **Critical / F** | **Production isolation rests on instructions/denies rather than separate capability.** CI code or a leaked deployment route triggers production action before Gate C, even if agent was instructed not to. | [G] production deploy planned off, but test auto-deploy from GitHub; [D] classifier isn't safety guarantee | Separate prod and staging org/project/service identities, credentials, webhook routes and protected branches. No production integration at all until Gate C; protected manual approval for prod deployments; transaction/sending kill switches. | **Medium / High** |
| **F05** | **High / B** | **K9 contradicts Gate A's “no app code before approval.”** K9 runs the *first three real tasks* to measure usage, although K0–K9 are “before coding”; the stage-0 Gate A location also differs from kit Gate A. | [G] `Kit_Plan_Before_Coding.md` K9/last paragraph; `Build_Plan.md` Stages 0 and 1 | Define **Kit Gate A** after architecture/tests/harness, rehearse three **synthetic tasks only** (or get explicit permission for 3 sandbox code tasks); Stage 0 must precede it, and no real implementation runs without approval. | **Small / High** |
| **F06** | **High / B** | **Authoritative spec conflicts with runnable examples.** Builder uses the prototype's `monthly * 2`; $150/month pool work falls below the $500 fee threshold when it should count 4 months; UI and ledger disagree. | [M] `assets/js/flows.js` line 94; [G] delta §3 says 4 months and prototype is not source of truth | Freeze precedence (`delta > v9.3 > tests > prototype/historical`), write executable price examples first, replace copied fee rules in demo and generated tests, add traceability to source line. | **Small / High** |
| **F07** | **High / B** | **Circular oracle from the same AI:** agent updates twin to new 25-category decisions then writes app matching that twin; identical mistaken license interpretation passes all tests. | [G] Stage 1 update twin first; [A] correlated-model error | Independent immutable requirements examples authored/reviewed **before** either implementation; mutation tests of twin; differential tests of real app vs independent fixtures; external license/legal approval. | **Medium / High** |
| **F08** | **High / C** | **Concurrent task ownership and state races:** K2 parallel agents amend shared decision IDs; two task sessions both read `queued`; one overwrites the other's `tasks.json`/`PROGRESS.md`; merge order changes semantics. | [G] worktrees plus sessions may flip passes; [A] race analysis | Single-writer runner, atomic lease using GitHub issue/PR/check or transactional state DB, append-only event log, unique run ID, compare-and-swap on version and SHA; serialized merges. | **Medium / High** |
| **F09** | **High / F** | **Unverified vendor-secret setup:** Stripe secret injection works but Twilio basic header is not the one intended; integration falsely passes against mocks; build claims real sandbox coverage. Anthropic API specifically **cannot** receive a user-added network secret. | [D] cloud environment secret exclusions, types; [G] test AI + test vendors planned | Vendor matrix: host, auth type, allowed methods, test-only account, reversible check, expected status, response log redaction, capability test. Do not wire live Anthropic key to cloud build; inject fake AI or have owner authorize isolated API-funded test only as a separate explicit exception. | **Small–Medium / High** |
| **F10** | **High / C,D** | **Model policy is not enforceable by a prompt.** Task named “Sonnet” creates Opus subagent or model unavailable and silently inherits; actual usage exceeds capacity and planned quality split. | [D] Subagent precedence and fallbacks | Use full model IDs, verify actual invocation metadata, pin Claude Code CLI version, reject substitutions rather than marking task complete; allocate Opus reviews especially to money/privacy/restore. | **Small–Medium / High** |
| **F11** | **High / B** | **Existing category/licensing table is not a verified executable source:** expansion to ~95 services/25 launch categories with ROC, city and pest rules demands human classifications; K2 can guess missing job types or changed definitions and still reach “no conflicts.” | [G] delta §2 and v9.3 5-trade twin; [A] missing verified catalog | Require a versioned catalog covering **each of 25 launch categories** with all allowed business classes, excluded job types, permits, recurring fee formula, source/date, reviewer and tests. Unresolved categories disabled, never guessed. | **Medium–Large / High** |
| **F12** | **High / E,F** | **Stage 0.5 may not pass A2P review and violates publish guardrail without a separate owner approval.** Landing page with generic privacy/SMS terms but no verifiable opt-in or business context is submitted; Twilio rejects; test-to-real texting stalls. | [G] Stage 0.5; [D] [Twilio business info](https://www.twilio.com/docs/messaging/compliance/a2p-10dlc/collect-business-info), [insufficient site context](https://www.twilio.com/docs/api/errors/30920) | Owner signs off on publishing Stage 0.5 before any DNS change; show brand, local-service description, real opt-in evidence/flow, public policies/terms, disclosures, HELP/STOP, contact and exact example messages matching submitted use case. Submit only after attorney review. | **Medium / High** |
| **F13** | **High / C** | **Failure escalation under-specified.** Sonnet fails twice, Opus once, no decisive policy for corrupted tests, flaky failures, vendor outage or invalid requirement; dependent task executes against partial change. | [G] escalation rule; [A] failure chain | State machine `QUEUED → CLAIMED → RUNNING → VALIDATING → MERGE_PENDING → DONE` or `BLOCKED`; classify failures, cap retries, quarantine flakes, block dependents, alert owner with an actionable single question. | **Medium / High** |
| **F14** | **High / B,C** | **40–70 tasks grossly underspecifies integration and cross-area work.** Complex money/contacts/timer/restore paths share files and cross-cut database and API boundaries; enforcing disjoint ownership either creates giant files or frequent deadlock/merge churn. | [G] proposed task count/strict ownership; [A] engineering sizing | Graph **vertical slices** with schema contracts and integration tasks, 5–10 boundary tests per key state machine, migration and operations tasks separate; allow 80–140 tasks if measured rehearsal warrants. | **Medium / Medium** |
| **F15** | **High / F** | **PII may travel through the build unintentionally.** Fixtures/logs contain real job messages or photos; sent to consumer cloud Code/sandbox/review prompts, violating no-identifiers-to-retaining-AI rule. | [G] scrubbing and zero-retention division; [A] possible spill path | Synthetic fixtures only; seeded fake phones/addresses; PII and secret scanners on git diff, logs, traces, exceptions, Sentry, email, screenshot/video, PR summary and subagent payloads. ZDR production gating separate. | **Medium / High** |
| **F16** | **Medium / C,F** | **Restore “drill” may validate the wrong operation.** Railway PITR creates a **new sibling Postgres service**, not an automatic rollback of original; app may reconnect old service, or Stripe/Twilio replay may double-send. | [D] [Railway PITR](https://docs.railway.com/volumes/point-in-time-recovery); [G] restore replay requirement | Rehearse cutover plus lineage change, pause sending/charging, reconcile external transaction IDs, verify no duplicate messages/charges, restore from a real backed-up database image and record RPO/RTO. | **Medium / High** |
| **F17** | **Medium / B,C** | **Test-first criterion is superficially satisfiable.** One trivial failing unit test per requirement goes red then green; quote page fails keyboard focus, race handling, actual refund webhook and private link auth. | [G] K5 “one test per acceptance criterion”; [A] test-quality gap | Require tests per **behavior/boundary/failure/invariant**, test mutation score on core money/identity rules, integration tests, accessibility and end-user journeys. Reviewer judges assertions rather than test count. | **Medium / High** |
| **F18** | **Medium / D** | **20% context budget is not prospectively measurable from file sizes.** Tool responses, stack traces, browsing, tests, internal reasoning, repeated failed edits and subagent reports grow usage; precompact hook can turn limit into error. | [G] static size budget; [D] model window and hooks | Track *actual input/output* and context high-water mark after rehearsal; task budget based on 95th percentile, cap tool output, checkpoint at 60% of allowable task budget, restart on compaction. | **Small–Medium / High** |
| **F19** | **Medium / C** | **“Status in Claude Doc” has no tested path.** Worker completes but never updates doc/phone; owner thinks nothing happened; blocked task sits unknown. | [G] docs/notifications; [A] integration unverified | Commit `STATUS.md` plus machine JSON; GitHub checks/issues as primary owner view; verify any Claude Doc/phone adapter independently and never rely on it to change task state. | **Small–Medium / High** |
| **F20** | **Medium / B,F** | **A11y/browser acceptance insufficient.** Three JS files parse, and prior 1280px axe scan reported clean, but mobile 320px had issue; 200% zoom/screen-reader were not run, and prototype contains fake login/quotes. | [M] JS syntax passes, README explicitly front-end-only; [G] delta §6 | Playwright with seeded backend, mobile/tablet and desktop, keyboard-only, screen reader, axe, focus handling, untrusted HTML input, actual authenticated links; manual Gate B. | **Medium / High** |
| **F21** | **Medium / E** | **Outside-approval dependency schedule is too optimistic.** LLC/EIN/brand matching, A2P, attorney-reviewed opt-in, insurance, Spanish and zero-retention negotiations can remain open well after coding. | [G] plan 4–8 weeks; [D] Twilio verification needs public matching opt-in | Run approvals in parallel as soon as owner authorizes; plan separate **technical ready** and **public launch** gates; no automatic launch after build green. | **Small / High** |
| **F22** | **Medium / C** | **Recheck on latest branch missing.** PR tests passed for SHA A; another merged PR changes matching logic; late PR merges after outdated pass and breaks integration. | [G] parallel PRs; [A] git race | Require status checks on exact merge head, merge queue, sequential protected merges, smoke test after merge, no stale approvals. | **Small–Medium / High** |

### The five mandatory changes before Gate A

1. **F01/F02/F08/F13:** build a *real orchestrator* with single-writer state, lease expiry, SHA-bound external evidence, retry taxonomy, quota-reset requeue, blocked dependents and owner notifications. A session or agent may submit an artifact, not certify itself.
2. **F03/F07/F17:** transform the simulator into a **hard-failing, independently checked oracle**, not just a Python command returning 0. Fix/triage the measured matching/license invariant failures first, and retain negative tests that fail deliberately when logic is mutated.
3. **F05/F06/F11:** make **one authoritative source tree** with versioned test oracles and generated conflicts checklist; settle the pre-Gate-A rehearsal contradiction; enforce 4-month recurring fee and 25-category license rules.
4. **F04/F09/F12/F15:** isolate staging from every production/sending/billing capability; verify credential proxy mechanics with the *test sandbox only*; enforce synthetic-data-only build, and require an explicit owner approval for even the preliminary public website.
5. **F10/F18/F19/F22:** pin and confirm model/runtime; prove real 3-task completion including PR merge and post-merge CI; test recovery from quota, lost network, paused tool, flaky test, Git conflict, and invalid test; give owner an auditable status view.

## 4. Walk-through: first approximately 10 sessions

**Precondition:** Owner creates/selects a *private, development-only* GitHub repository with protected `main`, a Max subscription and a cloud environment. No real accounts/keys/prod project are manipulated by an agent. The session schedule below replaces unverified parallel writes with controlled file ownership.

| Session | Model [A] | Reads | Writes | Exit evidence and failure trap |
|---|---|---|---|---|
| **S01 — K0 inventory** | Sonnet 5.5 | All package filenames, hashes, `START_HERE`, authoritative delta | `kit/source-manifest.json`, immutable source copies, initial state | Script verifies exact file hashes and files; no derived spec yet. Trap: missing reference/license catalog → `BLOCKED_SOURCE`. |
| **S02 — K0 source precedence** | Opus 5.5 | Source manifest, v9.3, delta, business/design notes | `docs/source-precedence.md`, conflict inventory | Zero unresolved *priority rules*; identifies recurring `4 vs 2` and brand conflicts. Trap: assuming old prototype rules are current. |
| **S03 — K1 constitution** | Opus 5.5 | User guardrails, decisions, independent verification rules | `docs/constitution.md`, minimal `CLAUDE.md`, policy tests | Independent reviewer finds every guardrail represented; cannot proceed if no distinct prelaunch publish approval. |
| **S04 — K2 money** | Opus 5.5 | Delta §3, blueprint money, simulation tests, pricing UI | `docs/spec/money.md` numbered requirements and fixed sample values | Trace table including $150/month → $600 exposure → $75 tier; questions not invented as facts. |
| **S05 — K2 matching/license** | Opus 5.5 | Delta §2, license rules, twin `CLASS_ALLOWS` | `docs/spec/matching.md`, `docs/category_catalog.yaml` draft | 25-category catalog audit; unknown classes flagged, not auto-approved. **Do not write same files simultaneously with S04.** |
| **S06 — K2 messaging/consent** | Opus 5.5 | Delta role play + Twilio guidance | `docs/spec/texting.md`, consent fixtures | Inbound/outbound opt-in, STOP/HELP, quiet hours, safety, translated templates; separate submitted carrier examples. |
| **S07 — K2 lifecycle and identity** | Opus 5.5 | v9.3 statuses, cancel and identity deltas, twin tests | `docs/spec/lifecycle.md`, identity requirements | Cross-state terminal/expiry conditions explicit; unresolved external check flagged. |
| **S08 — K2 portals, privacy, operations** | Opus + Sonnet | Prototype, revision notes, owner and security requirements | Distinct spec files and component list | Verifies prototype **not implementation**, no PII fixtures, no production actions; WCAG acceptance defined. |
| **S09 — K3 cross-area conflict audit** | Opus 5.5 fresh session | All requirements and source manifests; no earlier agent discussion | `docs/decisions.md`, `docs/conflicts.md` | Every conflict either resolved by existing decision or **blocked pending owner**; no self-invented approval. |
| **S10 — K4 architecture/contracts** | Opus 5.5 | Approved area specs and threat model | `docs/architecture.md`, module boundaries, DB invariants, migration plan | Every requirement owned; schema/state machines cover receipt dedupe, transactional state, message outbox, auditing and idempotency. |

**Then K5–K8:** independent behavior tests → category mutation tests → task graph with explicit integration edges → prompts → runner/harness with failure injection. **K9:** rehearse three **synthetic** tasks end to end; any real app implementation before Gate A requires a distinct owner authorization and must be renamed as such. Run each rehearsal through a real scheduled cloud session to `PR → CI → merge → durable state → owner status`; it must include one deliberate failure.

## 5. Critical path to go-live (Area E)

### Dependency order and gates

1. **Immediately, with owner consent [G/A]:** privately prepare repo, source manifest and test infrastructure. Independently begin legal counsel review of business model/license matching, text consents and privacy policies. Ask an insurance broker for coverage; contact Anthropic sales about ZDR terms. These can proceed in parallel with a private code build.
2. **Form entity [G]:** owner forms the Arizona LLC, completes any needed local notice, then obtains EIN. Timing in package: roughly **1–2 weeks**, not a guarantee or an independently verified ACC queue estimate.
3. **Public registration-quality site [D/G]:** owner explicitly approves Stage **0.5 publish**. Provide publicly accessible brand/service context, **real consent mechanism or verifiable proof**, brand-consistent privacy policy and SMS terms, HELP/STOP, frequency and rates notices. A generic coming-soon page can be rejected. Twilio reviews live pages, not just copy drafts ([Twilio info](https://www.twilio.com/docs/messaging/compliance/a2p-10dlc/collect-business-info), [error 30920](https://www.twilio.com/docs/api/errors/30920)).
4. **A2P onboarding [G/A]:** after brand profile matches legal/EIN records and consent package is approved internally, owner submits to Twilio. Package says brand **72 h+** and campaign **5–15 days**; those are planning placeholders, **not guaranteed carrier service-level agreements**. A rejection may reset portions of the path.
5. **Account verification [G/A]:** owner performs Stripe account activation, bank/KYC, operational email/domain checks, and restricted sandbox key placement. Do not equate a Stripe *test* checkout passing with live processing approval.
6. **Production privacy readiness [G/A]:** owner verifies the *executed* zero-data-retention arrangement, data flow and exception coverage for the **production** Claude API (not Max consumer/Code usage). Spanish templates are reviewed by a native speaker; attorney completes license/fee review; insurance is bound.
7. **Gate B [G/A]:** actual test-environment click-through with end-to-end synthetic customer/provider/operator cases; backup/PITR restore and Stripe/Twilio reconciliation; security/privacy/accessibility results; incident runbook and owner alert drill.
8. **Gate C [G/A]:** only the owner enters live keys and explicitly authorizes go-live. Production deploy requires separate protected action and live smoke tests. **No live phone/SMS/payment before owner gives the relevant specific permission.**

### Earliest credible dates

- **[A] Technical test environment:** approximately **4–8 calendar weeks** after a successful Gate A rehearsal, allowing for integration corrections; could be earlier or later.
- **[A] Public launch:** **6–12+ calendar weeks** from October 27, 2026 (**roughly Dec 8, 2026–Jan 19, 2027 or later**) if outside approvals and owner tasks are completed in parallel. This is *not* a committed launch date, and more complex compliance or test failures could extend it substantially.
- **[G] Original rough outside-approval estimate:** **4–8 weeks**. **[A]** The original schedule and technical task estimate should not be conflated with customer-visible launch readiness.

### What to start first

**[A]** Start legal/privacy/insurance/ZDR discussions and LLC setup **in parallel** with kit preparation. Do not wait until the app is coded to discover the carrier dislikes the opt-in process or the attorney changes fee/contractor matching rules. The Stage 0.5 page needs a separate *explicit* publish authorization, otherwise it violates the stated guardrails.

## 6. What's missing from the kit (Area B)

**[A]** Add these required artifacts, each with acceptance checks, before promoting tasks to build:

- **Authoritative source matrix** (`source_id`, file+line, effective date, priority, superseded_by, requirement IDs). Prevents historical $45/$59/$149 fees, 2-month recurring multiplier and old brand leaking into generated code.
- **Versioned complete category/license registry**, including 25 launch categories, job types, no-license/ROC/pest controls, permit boundaries, town list, audit freshness and blocked-unknown behavior. Attorney review status stored as a data field.
- **Canonical state machines / state transition tables** for intake, pick, consent, verification, charge/refund, complaint, STOP, quote-window timers and restoration. Invariant: every non-terminal state has expiry/recovery and every irreversible side-effect has a stable idempotency key.
- **Idempotent outbox/inbox schemas** for SMS, email, Stripe webhooks, confirm, and status messages. Need repeated webhook and stale restore-event tests; unique constraints, locking semantics, collision ordering and reconciliation against actual provider state.
- **Security and data-flow threat model** for homeowner contact vault, URLs/tokens/codes, demo credentials, signed provider links, masked calls, log retention, backups, support access, Sentry, screenshots, and third-party AI. Map the zero-retention constraint to every external flow.
- **Fixture and data-handling policy**: synthetic inputs only in Code/cloud sessions; robust logging redaction; fail PR when names/contact/photos show up. Keep seeded fake IDs in a separate namespace; forbid production data export.
- **Environment capabilities matrix:** test, staging, production, what each can access, with explicit no-live guardrails and connection tests. Stage 0.5 publishing its own owner sign-off.
- **Runner written contract**: atomic lease ownership, retry/backoff, heartbeat, usage reset, fatal errors, dependency blocking, repo token scopes, SHA-bound pass evidence, job ID, eventual consistency and idempotent schedule fire.
- **Real pipeline fault-injection spec**: crash immediately after commit/before state update; duplicate GitHub event; failed CI; lock expiration; rejected scheduled run; stale PR; malicious code command; STOP hook capped; API outage; quota limit; human prompt. Tests pass only if runner safely recovers or blocks.
- **DB version/migration policy**: Postgres version, schema migrations (forward/back), staging seed/rollback, PITR drill, secrets and environment promotion.
- **Vendor contract suite**: Stripe test capture/3DS/webhooks, Twilio test credentials vs real routing/voice, Lob identity delivery, Resend sending domains, ROC/municipal snapshots, Anthropic test fake adapter, browser package installs; expected response/headers, failures and redacted fixtures.
- **Manual user-acceptance suite** for homeowners, contractors and the solo operator (see below), not replaced by mock business simulations or a static prototype.
- **Accessibility and translation gate** beyond automated axe: manual keyboard and screen-reader, 200% zoom, 320px view, native Spanish proofreading, SMS segment counting and encoding.
- **Observability/SLO**: synthetic requests, failed delivery count, unresolved picks, payment mismatch, backlog duration, rate limits, clock skew, license feed staleness, and *owner-offline safe mode* with tested pager/alert path.
- **Test provenance and versioning**: every test records which source revision it protects. Require full change impact review when source, catalog, translations or vendor contract changes.
- **Hard-definition of complete:** a runnable app deploy, functional screens connected to live staging adapters, green external CI on merge SHA, reviewed ledger, signed off operator guide, restore proof, vendor approvals and owner Gate B.

### Four end-to-end acceptance journeys not to skip

**[A] Homeowner:** Request a leaking pipe, receive consent, change language, receive first quote, see multiple quotes in arrival order, select one with an unforwardable one-time code, test contact privacy, cancel before payment, retry after STOP/START, request after quiet hours and request no eligible provider. Verify messages arrive in correct order and disappear/close when promised.

**[A] Contractor:** Sign up under a legitimate but narrow ROC scope, fail a mismatched job, pass identity verification, set availability/tenant/language filters, quote fixed/range/recurring, confirm on an expired pick, fail a 3DS payment, get no customer data until confirmed and paid, refund per unreachable customer policy, inspect ledger and repeat-customer free service.

**[A] Solo operator:** Work only 15 minutes; spot a licensing data outage, high charge mismatch and STOP complaint; click pause; resume without duplicate delivery; deny an applicant without identity proof; see alert when absent 24 hours; run recovery; reconcile all Stripe charges and refunds; audit which AI saw any customer content.

**[A] Platform failure:** Drop 20% of carrier webhooks, inject duplicates, delay an accepted payment notice until after cancellation, restart mid-release, corrupt/restore DB at a timestamp in the past; verify no new charge, contact leak, resend, restored identifier collision or orphaned job. A conditional *expected failed test* should force a nonzero CI result.

## 7. Timeline and usage check (Area D)

| Phase | Original package [G] | Revised practical estimate [A] | Explanation |
|---|---|---|---|
| K0–K9 kit | ~25–35 sessions / **3–5 days** | **30–60 sessions / 1–3 weeks** | Independent category oracle and fault-injection orchestrator must be made real; many models/sessions on Max share weekly caps. |
| Application build | ~40–70 tasks / 60–120 agent-hours / **1–2 weeks** | **80–140 tasks / 150–350 agent-hours equivalent / 3–8 calendar weeks** | Bilingual pages, payments, permissioned messaging/calls, 25 licensing categories, external adapter validation, portals, migration, restore and accessibility require integration and correction cycles. Hours represent summed autonomous work and are not a billing estimate or empirically measured Claude usage. |
| Vendor and legal sign-off | **4–8 weeks** | **4–12+ weeks** | LLC/EIN, A2P, legal text, insurance, ZDR and live financial activation vary and should overlap. |
| Total customer-visible launch | *Implied by separate figures; not explicit* | **6–12+ weeks** | Mostly parallel work, with integration and approval dependencies at the end. |

**[D]** Claude Max 20x provides 20× per-session Pro capacity but **not unlimited throughput** or a published universal tasks/hour allowance; the five-hour and weekly pools also include normal chats. Opus can consume a quota more quickly than Sonnet for some workloads, and subagents share the same pool. Do **not** convert context size to scheduled throughput. **[A]** Budget by *percent of five-hour allowance and weekly allowance consumed per actual completed task* (with risk-tier and model), and measure the 95th percentile of task duration/context and PR rework; reset forecasts every 10 tasks.

**[A]** Model allocation: Haiku is appropriate for *summarizing deterministic* tests and formatting; **do not delegate judgment on financial/identity/security invariants to Haiku without deterministic fixtures and higher-model review**. Sonnet handles most code; Opus owns architecture, legal-data modeling, state transitions, hard debugging, payment/restore and stage audits. Use actual outcomes rather than force a 30/60/10 token split.

## 8. Conflicts with guardrails and decision table (Area F)

| Source item | Problem | Resolution |
|---|---|---|
| **[G]** “No publishing before explicit go” vs Stage 0.5 public `askhava.com` publish | Publish occurs *before* Gate C but no independent authorization is defined | Add Gate **0.5-Publish** owner approval; only publish a reviewed static/compliance site and only after owner confirms DNS and privacy text. |
| **[G]** “No app code until Gate A” vs K9 “real run of first three tasks” | K9 might implement app without consent | Synthetic task rehearsal, or a distinct early-code permission with explicit owner approval. |
| **[G/M]** Four months recurring vs demo `*2` | Wrong computed fee shown in UI | Correct code and add shared price function + executable fixtures. |
| **[G]** App AI must be ZDR, but test keys are cloud network secrets | Proxy **excludes `api.anthropic.com`** from network-secret injection | Use fake AI in Code/cloud staging until owner-approved separate test route; no sensitive real customer data until signed ZDR. |
| **[G/D]** Source code/PR access permitted vs no production deployment before Gate C | Repository or automatic deploy settings could couple test and live | Independent staging/prod deployment identity, disabled prod auto-deploy and protected manual approval, reviewed GitHub workflows. |
| **[G]** Subscription-only Max vs GitHub Actions backup | Original plan may assume a paid API account is needed for Action | GitHub Action accepts owner-provisioned subscription OAuth token. Never create an `ANTHROPIC_API_KEY` fallback silently. Secret stored in GitHub Actions secrets, not code. |
| **[G]** Old 5-trade twin vs 25-category launch | Oracle doesn't yet represent authoritative data and has observed invariant failures | Freeze independent catalog + fixture acceptance first, update twin and app separately and enforce hard-fail CI. |
| **[G]** All opted-in qualified businesses receive every card vs baseline measured misses | A green command doesn't imply business promise met | Independent expectation check and failing exit code for any miss; validate if simulator checker has false positives before blaming future app. |
| **[G]** “Always free repeat work” vs refund/free-win edge cases | Multi-account matching and identity may lead to accidental fee/reused promotion | State machine, business legal-entity identity key and exact repeated-customer fixtures with independent review. |
| **[G]** Owner alone; 24-hour safe mode | Alerts aren't handled during absence; pending money/consent stuck | Test operator outage and safe mode before launch; no autonomous risky recovery on live accounts. |

### Minimum proof required to approve Gate A

- **[A]** One live, **owner-authorized cloud** rehearsal using fake-only data: trigger a synthetic task by API/event; clone private repo; prove correct model; write small code change; run test; create PR; merge with protected checks; verify status and alert. 
- **[A]** Repeat with **test failure**: runner leaves task blocked, no merge, dependent task remains queued. 
- **[A]** Repeat with **duplicate trigger**: exactly one worker claims task; no duplicated side effect. 
- **[A]** Repeat with **quota or authenticated-trigger rejection** (safely simulated): job remains pending and is retried after cooldown without API overage. 
- **[A]** Repeat with **rejected command/no permission**: no production/publish/charge action, owner notified. 
- **[A]** Oracle fail injection: deliberately disable license filter or bypass the 4-month fee multiplier; CI **must go red** on the exact immutable test case. 
- **[A]** Demonstrate there are no real identifiers or secrets in code, tests, commits, artifacts or logs; and the owner can read a truthful live progress fraction based only on completed validated requirements.

**Recommended owner decision:** Approve the **kit repair and rehearsal**, not the unattended full build yet. Move to Gate A only after these pass and all unmatched source decisions are explicit. Preserve the owner's three later approval gates, plus the added Stage 0.5 publish gate.

## 9. Official sources verified October 27, 2026

All documentation links below were opened or their relevant official search results checked on **October 27, 2026 [D]**. These pages can change, especially research-preview features; recheck them when implementing the harness.

1. Anthropic — [Automate work with scheduled tasks / Routines](https://code.claude.com/docs/en/routines) (hourly cron minimum; on-demand API and GitHub triggers; scheduled runs and quota behavior; research preview; model and permissions).
2. Anthropic — [Claude Code GitHub Actions](https://code.claude.com/docs/en/github-actions) (subscription OAuth token `CLAUDE_CODE_OAUTH_TOKEN`, `claude setup-token`, workflow authentication and encrypted GitHub secret).
3. Anthropic — [Configure cloud environments](https://code.claude.com/docs/en/cloud-environments) (network secrets and excluded hosts; startup caching; installed PostgreSQL/Docker; network reachability and VM limits).
4. Anthropic — [Hooks reference](https://code.claude.com/docs/en/hooks) (PreToolUse, Stop, StopFailure, PreCompact; stop-loop cap, error behavior and hooks outputs).
5. Anthropic — [Custom subagents](https://code.claude.com/docs/en/sub-agents) (frontmatter model setting, invocation overrides, model substitution and task model reporting).
6. Anthropic — [Model configuration](https://code.claude.com/docs/en/model-config) (current model IDs/aliases, model default effort, 1M contexts and compaction).
7. Anthropic — [Configure auto mode](https://code.claude.com/docs/en/auto-mode-config) (trust boundary, classifier decisions, managed/user config, ask/deny requirements).
8. Anthropic — [Use Claude Code in the cloud](https://code.claude.com/docs/en/claude-code-on-the-web) (cloud sessions and GitHub integration).
9. Anthropic — [Claude Max plan](https://support.claude.com/en/articles/11049741-what-is-the-max-plan) and [Use Claude Code with Pro/Max](https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan) (usage pools and subscription vs API).
10. Twilio — [Collect A2P campaign business/opt-in information](https://www.twilio.com/docs/messaging/compliance/a2p-10dlc/collect-business-info), [Campaign quickstart](https://www.twilio.com/docs/messaging/compliance/a2p-10dlc/quickstart), [website insufficient business context](https://www.twilio.com/docs/api/errors/30920) (public verifiable opt-in, legal pages, matching brand/service context).
11. Railway — [Point-in-Time Recovery](https://docs.railway.com/volumes/point-in-time-recovery), [Back up and restore Postgres](https://docs.railway.com/guides/postgres-backups-restores) (rough four-week archive if configured, new sibling service on restore, restore drill).

## Appendix A. Exact offline execution evidence

Commands ran locally from `01_spec/hhq_twin/`, using bundled fake vendors and synthetic actors only:

```text
$ python3 review_tests.py
  exit code: 0
  v9.3: 19 PASS / 0 FAIL (the 19 targeted regression tests)

$ python3 twin.py normal_fixed 1
  exit code: 0
  requests: 298; cards: 757; quotes: 466; picks: 161
  6.0 x qualified_business_left_out

$ python3 twin.py vendor_chaos_fixed 2
  exit code: 0
  requests: 297; cards: 798; quotes: 471; picks: 162
  4.5 x qualified_business_left_out
  3.5 x job_card_outside_license

$ node --check assets/js/flows.js
$ node --check assets/js/site.js
$ node --check assets/js/sample-data.js
  all three: exit code 0 (syntax only)
```

**Important [M/A]:** The twin's `twin.py` calculates `job_card_outside_license` using `CLASS_ALLOWS` and a simulated contractor's license. It also calculates `qualified_business_left_out` from the simulated event history. These may include simulation artifacts, stale truth labels or genuine matching bugs; resolving which is the first prerequisite. **Do not report them as proven real-product violations.** Conversely, **do not call the oracle green because the Python process returned zero**.

## Appendix B. Proposed minimal dispatcher contract (pseudocode, not executed)

```text
on wakeup/event:
  read authoritative state with version (transaction or CAS)
  do not take new work when owner gate or global pause is closed
  find task with deps all DONE and status QUEUED/RETRY_READY
  acquire expiring lease atomically; assign unique attempt ID
  start owner-authorized Claude subscription-backed session
  persist session ID, chosen model, start timestamp, task/commit hash

on result/timeout:
  inspect real session outcome, logs and model; never trust self-report
  require named deterministic tests, independent oracle and reviewer
  require green protected GitHub checks for EXACT head SHA
  merge via serialized queue; rerun post-merge checks on merged SHA
  only runner sets DONE in durable state
  on transient failure: RETRY_READY with capped backoff and not-before
  on quota: RETRY_READY after reset + safety margin (no paid overflow)
  on semantic failure, repeated block or unknown: BLOCKED; stop dependents
  preserve partial changes in non-live branch, record evidence, notify owner
  all duplicate events/old attempts must be no-ops
```

**This is a design recommendation [A], not evidence of a working scheduled runner.**
