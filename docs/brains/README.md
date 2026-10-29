# Brains: the expertise behind the build

**Adopted:** October 29, 2026 (D-487). **Locked** (`kit/plan.lock`): a change needs a decision entry. **Never above the spec:** a brain explains and sharpens; when a brain and `docs/spec/` or `docs/architecture.md` disagree, build the spec and report the disagreement.

Ask Hava is built and maintained entirely by AI sessions that start with no memory. Each brain is the sourced knowledge a careful specialist would bring to one part of the work: who the recognized authorities are, the rules they set, why each rule exists, and how it maps to Ask Hava's numbered requirements. Every principle carries a URL to a primary or authoritative source; anything that could not be sourced is marked `[inferred]`.

| Brain | Read it before | Serves |
|---|---|---|
| [rust-production.md](rust-production.md) | Any Rust code (R1 to R18) | SEC-029 to SEC-031, SEC-046, JOB-004, JOB-006, D-481 |
| [postgres-operations.md](postgres-operations.md) | Migrations, roles, the task queue, backups, the restore drill (P1 to P15) | D-092, OPS-012 to OPS-022, PRIV-002, JOB-006 |
| [sms-compliance.md](sms-compliance.md) | `messaging`, `intake`, the SMS terms, the campaign registration (S-tags) | CON-*, TXT-*, QH-*, TERMS-011 to TERMS-014, D-305, D-424 |
| [payments-stripe.md](payments-stripe.md) | `payments`, `picks` confirm and release, the business terms (P-tags) | FEE-*, PICK-048, PICK-056, SEC-039 |
| [accessibility-forms.md](accessibility-forms.md) | `site`, `portals`, every page and form, the Gate B script | A11Y-*, WEB-*, SEC-042 |
| [email-deliverability.md](email-deliverability.md) | `messaging` email, `adapters` email, inbound email, the DNS records | SEC-025, SEC-028, BIZ-040, PRIV-026, TXT-034 |
| [contractor-lead-experience.md](contractor-lead-experience.md) | `businesses`, the quote form, the loss notice, fee wording, the hand-run test | BIZ-*, FEE-031, PICK-026, OPS-039 |
| [operator.md](operator.md) | Any business decision or plan change | D-455 to D-467, `docs/PREBUILD.md` |

## How a session uses a brain

1. Your task prompt lists the brains for your module under "Files you read". Read the sections for your task, not the whole file.
2. When a design choice rests on a principle, cite its tag (for example `R7`, `S12`, `P4`) in the pull request text and, for a rule the code applies, in the `///` comment beside the requirement ID (`docs/code-docs.md`).
3. Run the brain's review checklist before hand-in. The fresh-eyes reviewer runs it too.
4. A gap or conflict a brain names is a **proposal**, not a rule: a task may not add it. Proposals adopted into the spec carry a decision number; the rest wait for a plan change. The adopted set from the first edition is D-490.
5. A brain is corrected by a plan change with a decision entry, like any locked file. Add the date to "Last verified" when you re-check its sources.

## What is not here

- **Trade knowledge** (what each home-service trade needs to know to quote): `docs/catalog/trade_kb/`.
- **Copywriting**: `docs/copy/copy-panel.md`, `docs/voice.md`.
- **Arizona licensing law**: `docs/catalog/trade_kb/foundation/az_license_master.md`, `docs/catalog/license_map.yaml`.
- **Security threat model**: `docs/threat-model.md` (ASVS 5.0 L2).
- **The earlier research packs** (legal memos, the simulations, the GTM pack, the first operator pack) lived under `/home/claude/hhq/outputs/` outside this repository and are not reachable from a new session. `docs/risk-register.md` R-19 tracks bringing them in. `operator.md` is a rebuilt, sourced condensation of the operator pack.
