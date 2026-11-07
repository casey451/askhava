# Third repository audit of November 7, 2026: dispositions

**Input:** a third external, read-only review of `main` at `9a710c0` (a full audit and a recommendations list), received as an attachment and treated as untrusted input. The overseer checked each finding, and this plan change checked each again against `main` at `6ea04a3` (T093, #330, merged) before planning anything. The audit's own task and pull-request drafts are not committed.

Verdicts: **confirmed** (the code on `main` shows it), **already owned** (a queued task covers it), **declined** (correct as built, or covered elsewhere). Decisions D-1866 to D-1873.

## Taken

| Finding | Verdict | Evidence on `main` 6ea04a3 | Owner | Acceptance (test-design) | Decision |
|---|---|---|---|---|---|
| `Ci`'s Rust, Coverage and E2e path rules skip compiled-in documents | Confirmed | `.github/workflows/ci.yml:127`, `:299`, `:404` and `scripts/prehandin.sh:62` match neither `docs/` nor `schemas/`; `app/server/src/lib.rs:136`, `app/ai/src/schemas.rs:23` and seven `docs/catalog/*.yaml` files are `include_str!`ed. `a11y.yml` and `twin.yml` have no path rule | T283 (new, harness) | TD-OPS-32 | D-1867 |
| Nothing proves a documented route is wired | Confirmed | routes are described only in prose; `build_router` (`app/server/src/lib.rs:75`) merges five routers and nothing compares them with a list | T284 (new) | TD-SEC-41 | D-1868 |
| Owned files are checked only when a later task looks | Confirmed | `runner/cycle.py:625` marks DONE from PR evidence alone; T092's `purge.rs` gap surfaced at T093 and T253 (D-1801) | T285 (new, harness) | TD-OPS-33 | D-1869 |
| No request time limit | Confirmed | `Cargo.toml:38` enables `timeout`; `build_router` applies no `TimeoutLayer` | T286 (new) | TD-SEC-42 | D-1870 |
| The Confirm path charges a caller-supplied amount | Confirmed | `open_intent` and `confirm_and_pay` (`app/payments/src/charge.rs:91`, `:146`) use `Press::amount_cents` (`app/payments/src/models.rs:325`), checking only that it is not 0; no code stores the fee at the pick (`app/picks/src/service.rs:340`) | T287 (new); queued T261 now uses it | TD-FEE-48, TD-FEE-45 | D-1871 |
| A fee-mode switch between pick and Confirm fails only after Stripe charged | Confirmed | `record_fee_at_confirm` (`app/payments/src/service.rs:344`) refuses `Mismatch` after the charge; `may_confirm` checks only the authorization version | T287 (new); T261 ends the pick with no fee | TD-FEE-48, TD-FEE-45 | D-1871 |
| The reconcile looks back 7 days only | Confirmed | `LOOKBACK` (`app/payments/src/reconcile.rs:51`); nothing keeps a charge it could not settle | T288 (new) | TD-FEE-49 | D-1872 |
| The refused-move test counted 0 errors once | Confirmed (one `Ci` run, T196 attempt 1) | `app/jobs/src/tests/transitions.rs:102` counts through a thread-local subscriber; tracing caches call-site interest globally | T289 (new, harness) | TD-JOB-15 | D-1873 |
| The total coverage floor is 0.0 | Already owned | `tests/coverage_floor.json:3`; T264 (QUEUED) sets it from a named `Ci` Coverage run (D-1759) | T264, unchanged | TD-OPS-30 | D-1866 |
| Risk R-22 still read "Open until T250 merges" | Confirmed | T250 merged as `1472cf6` (#299) with TD-SEC-34 | closed in `docs/risk-register.md` | — | D-1866 |

## Declined

- **The recurring-range rounding "bug" (the audit's T283):** declined. `four_months` (`app/payments/src/rules.rs:290`) computes the 4-month figure exactly and rounds the fee basis down once, which is D-033's rule ("compute exactly, then round the fee basis down to the whole cent before choosing the tier"); flooring the midpoint first would change D-033.
- **Unit tests inside `ops`:** declined. The pause switches and the audit log are exercised by `tests/integration/tests/charge_attempts.rs` and `tests/integration/tests/audit_log.rs`.
- **A cross-crate duplication scan:** declined. Each stage's cleanup task already looks for duplicates.
- **Reworking the deny list in `.claude/settings.json`:** declined. Governance section 2 already names the ruleset, not the deny list, as what enforces `main`.
- **A faster plan-gap escalation:** declined as a separate change. T285's close-time mark reaches the overseer in the same cycle; no second repair path.

## Plan fixes made on the way

- T103, T105, T204 and T151 (QUEUED, nothing written) move their migrations from 0172 to 0175 up to 0174 to 0177, so T287's 0172 and T288's 0173 rise in the runner's serial order (V9).
- T100 (the stage-4 cleanup) depends on T283 to T289.
- `docs/architecture.md` section 3 lists `payments.pick_fee` and `payments.reconcile_open`; the threat model gains row D13; register rows IC-50 to IC-52 and risk R-31 are added.
- No task IN_PROGRESS or MERGE_PENDING was edited.
