# Integration-closure register

**Status:** started November 6, 2026 (D-1760). **Read by:** `scripts/stage_exit.py` once T265 merges, and by every plan-change session.

One row per cross-module handoff: a piece of work one task or decision passed to another module or a later task. This is not a second task graph. Owners are task ids from `kit/tasks.json`; a row is closed only by a plan change that names the merged commit or pull request and the test that proves it. A decision note naming a future task is not closure.

**Rule (T265, D-1760).** A row whose area is `security`, `money` or `privacy` and whose status is not `closed` is judged at stage N only when every owner task it names has a `stage` at or below N in `kit/tasks.json` (the runner re-runs every earlier stage's exit on each claim, so a row may only hold the stage its owners belong to). A judged row fails when it names a task id missing from `kit/tasks.json` or only tasks that are DONE or SKIPPED. A row with no owner (`none`) fails at every stage. A `closed` row with no evidence fails too. Rows of other areas are printed and never fail. When an owner task merges, the overseer's next plan change either closes the row with its evidence or names a new owner.

Columns: **Owner** is one or more task ids separated by commas, or `none`. **Status** is `open`, `tracked` (a non-gating improvement watched by its owner) or `closed`. **Evidence** is `-` until closed.

Rows IC-06 to IC-09 and IC-33 name T253, T254 and T255, added by the release plan change (D-1739, #313).

| ID | Handoff | Decision or finding | Producer | Consumer | Area | Owner | Test | Status | Evidence |
|---|---|---|---|---|---|---|---|---|---|
| IC-01 | First texts from a new phone: a lost contact race must not lose a text | A01, D-1751, D-752, D-1311 | privacy vault | server inbound route, intake | privacy | T256 | TD-TXT-32 | open | - |
| IC-02 | Business phone STOP and auto-reply from the inbound route | D-1715, D-1751 | server inbound route | businesses consent | privacy | T256 | TD-BIZ-39 | open | - |
| IC-03 | Scrubber to AI and card: vault name and stated names across texts and restarts | A02, D-1752, AI-003, AI-020 | privacy scrubber | server inbound route, intake, ai | privacy | T257 | TD-AI-19 | open | - |
| IC-04 | Refund answer to refund row, ledger and notices | A03, D-1753, D-1545 | adapters payments | payments refunds, reconcile, ledger | money | T258 | TD-FEE-42 | open | - |
| IC-05 | Stripe webhook events to refund, dispute and early-fraud handlers, once per event ID | A08, D-1754, D-1543 | payments webhook route | payments handlers | money | T259 | TD-FEE-43 | open | - |
| IC-06 | Payment success or $0 decision to settle, mint and release; webhook router merge | D-1480 (b), D-1563 (1), D-1739 | payments | picks, server | money | T254 | TD-PICK-49, TD-PICK-50 | open | - |
| IC-07 | Released pick to the signed-in business's repeat read (no stored copy) | D-1565 (1), D-1564, D-1739 | picks release | privacy, portals | privacy | T253, T085 | TD-PICK-48 | open | - |
| IC-08 | Release to masked line, and a failed line opened again | D-1688, D-1739 | picks release | calls | privacy | T254 | TD-PICK-50 | open | - |
| IC-09 | Emergency release waiting for an address, then re-cleared and released | D-1563 (2), D-1739 | picks | intake, payments, server | money | T255 | TD-INT-39 | open | - |
| IC-10 | Stand-in and adapter confirm flow and `IntentStripe` outside tests | A08, D-1754, D-1424, D-1480 (a) | adapters, stand-ins | payments, portals | money | T260 | TD-FEE-44 | open | - |
| IC-11 | Served Confirm page under real middleware, Confirm-only Stripe CSP, bank-approval handling | A08, D-1754, D-1480 (a) (c), D-1740 (1) | portals Confirm view | server router, payments | security | T261 | TD-FEE-45, TD-SEC-37 | open | - |
| IC-12 | Confirm route to J2's closing checks (payment, release, portal view) | D-1739, D-1754 | server, portals | e2e journey | money | T099 | TD-FEE-45 | open | - |
| IC-13 | Stored random PIN per line, durable PIN tries, inbound voice route | A09, D-1755, D-1684, D-1685, D-1708 | calls | server voice route | security | T262 | TD-CALL-13, TD-CALL-14 | open | - |
| IC-14 | Changed public record to a fresh identity episode | A10, D-1756, D-1712 | businesses sign-up | businesses identity, Verify | security | T196 | TD-BIZ-40 | open | - |
| IC-15 | Global AI daily cap as an atomic admission | A04, D-1757, D-811 | ai service | ai provider transport | money | T236 | TD-AI-20 | open | - |
| IC-16 | Monthly fee limit as a hard cap across picks and the off-session charge | A11, D-1758, D-1427 | payments charge | portals Confirm, picks auto-confirm | money | T263, T209 | TD-FEE-46 | open | - |
| IC-17 | Coverage report to the floor checker; nonzero measured total | A05, A06, D-1759, D-694 | Ci Coverage job | scripts/coverage_floor.py | security | T264 | TD-OPS-30 | open | - |
| IC-18 | This register to the stage exit | A12, D-1760 | docs/integration-closure.md | scripts/stage_exit.py | security | T265 | TD-OPS-31 | open | - |
| IC-19 | Card sender checks business consent (`may_get_cards`) at send time | D-1715, D-1760 | businesses consent | quotes card pool | privacy | T251 | TD-BIZ-41 | open | - |
| IC-20 | Business texts check `may_text` per receiving phone at send time | D-1715, D-1760 | businesses consent | messaging business notices | privacy | T147 | TD-TXT-17 | open | - |
| IC-21 | Sign-up form consent box, welcome request after activation, welcome and phone-stopped emails | D-1715, D-1709, D-1380, D-1788 | businesses consent | portals sign-up, businesses identity, messaging email | privacy | T268 | TD-CON-29 | open | - |
| IC-22 | Homeowner's PICK-018 contact choice and shared email stored and passed to the release | D-1740 (2), D-1264, D-1761 | portals pick flow | picks release, server release sink, privacy vault | privacy | T266 | TD-PICK-52 | open | - |
| IC-23 | AI writing calls (questions, card) and `ai::api` wired into intake | D-990, D-1198 | ai | intake | ai | none | - | open | - |
| IC-24 | Owner switches and safe mode checked at action time | audit A12 table, D-1427 | ops switches | payments, messaging, intake | security | T105 | TD-OPS-01 | open | - |
| IC-25 | Call logs to refund decisions (person, voicemail, side, windows) | audit A12 table, D-1527 | calls log | payments refund rules, picks | money | T191 | TD-FEE-22 | open | - |
| IC-26 | Restore and key rotation keep phone indexes, sender directory, suppression and replay keys usable | audit A12 table, PRIV-024 | privacy, recovery | every module with a blind index | privacy | T110, T257 | TD-OPS-04, TD-AI-19 | open | - |
| IC-27 | Every emitted task kind has a registered, configured handler before its journey needs it | audit A12 table, D-1428 | every module's outbox | server worker | reliability | T140, T237 | - | tracked | - |
| IC-28 | Worker lease as at-least-once execution: lease loss during a vendor call | audit improvement 2 | core tasks | every vendor effect | reliability | T237, T140 | - | tracked | - |
| IC-29 | Size bounds on trusted vendor responses (`exchange` reads whole bodies) | audit improvement 4 | vendors | adapters | reliability | T131, T132 | - | tracked | - |
| IC-30 | Full journeys under the restrictive app role and the shipped image | audit improvement 6 | deploy | every module | reliability | T140, T121 | - | tracked | - |
| IC-31 | Stale module guides ("not here yet" lines) repaired as work closes | audit improvement 5 | every task | next sessions | docs | T100, T126, T142 | - | tracked | - |
| IC-32 | Release and card-fix emails carry a per-recipient signed link that opens sign-in | D-1565 (2), D-1761, PICK-024 | messaging business notices | portals sign-in | security | T147, T269 | TD-TXT-18, TD-PICK-51 | open | - |
| IC-33 | Confirm route's $0 path to the production release sink (merge order) | D-1761 (3), D-1739 | portals Confirm (T261) | server release sink (T254) | money | T261, T254 | TD-FEE-45, TD-PICK-49 | open | - |
| IC-34 | Ally page-state registry split from the Ally check machinery into the shared `tests/a11y/page_states.ts`; `fixtures.ts` back to owner-only after IC-36 | D-1772, D-1788 | page-adding tasks | tests/a11y checks | security | T267 | TD-A11Y-03 | open | - |
| IC-35 | Lock this register in `kit/plan.lock` (step 1: `LOCKED_FILES` in `scripts/check_plan_lock.py` on `main`; step 2: rewrite the lock) | D-1760 (2), D-1788 | plan changes | Governance check | governance | none | - | tracked | - |
| IC-36 | After T267 merges: drop `tests/a11y/fixtures.ts` from `shared_paths` and `ALLOWED_SHARED` in one plan change (`tests/a11y/page_states.ts` is already shared, D-1788) | D-1788 | T267 | kit/tasks.json, scripts/validate_tasks.py | governance | none | - | tracked | - |
