# Ask Hava security readiness (ISO/IEC 27001:2022) — index

**Status:** draft for owner review (D-2740, November 11, 2026). Nothing here is approved, certified or a legal conclusion. Items that need Casey or counsel say so.

**Why this exists.** Casey shared an outside ISO 27001 readiness gap analysis on November 11, 2026 (reviewed `main` at `f8231346`). It found that the build already carries strong engineering controls, and that what is missing is a small, evidenced management system around them. This folder is that management layer, kept deliberately small: one index, one scope, one Statement of Applicability, one evidence register, one audit and review template, one inventory, one owner checklist. It does not replace the build's own plan, risk register or legal drafts; it points at them.

**What this is not.** It is not a certification claim, not an independent audit, not a penetration test and not proof that any live control works. An AI review (a fresh Opus session, T125, T140) is engineering review, not an independent ISMS audit (`audit-and-review.md`). The build's own risk register still says certification is a post-launch decision (`docs/risk-register.md:26`, R-21); this folder only prepares for it without adding build gates.

## Files

| File | What it is | Status |
|---|---|---|
| `reconciliation.md` | Every finding in the gap analysis matched to existing evidence, queued tasks or owner actions; proposed new tasks | Draft |
| `isms-scope.md` | Scope, boundaries, exclusions and a proposed risk method | **Owner approval required** |
| `soa.md` | Statement of Applicability skeleton: every Annex A 2022 control by reference and short title | Draft; nothing marked operating without operating evidence |
| `security-objectives.md` | A few measurable objectives and a monthly scorecard | **Owner approval required** |
| `evidence-register.md` | Where proof of each control lives (path, SHA, run), never the sensitive record itself | Started |
| `audit-and-review.md` | Internal audit, management review and corrective-action templates | Template |
| `asset-data-vendor-inventory.md` | Systems, data classes, providers and retention, reconciled with the WISP and the architecture | Draft; owner to complete the marked gaps |
| `owner-actions.md` | The short list only Casey or counsel can close | Open |

## Existing documents this folder builds on (not duplicated)

| Topic | Source of truth |
|---|---|
| Written information security program (WISP) | `docs/legal/information-security-program.md` (AI draft, not legally reviewed) |
| Incident response | `docs/legal/incident-response-plan.md` (AI draft, not legally reviewed); runbook to come (T117) |
| Build and security risks | `docs/risk-register.md` (R-IDs) and `docs/threat-model.md` section 6 (residual risks R1 to R20) |
| Security requirements and ASVS mapping | `docs/spec/security.md`, `docs/spec/privacy.md`, `docs/spec/ops-recovery.md`, `docs/threat-model.md` section 5 |
| Change control and machine-enforced rules | `docs/governance.md` sections 1 and 2; `kit/plan.lock`; `docs/decisions.md` |
| Recorded audit findings and their owners | `docs/reviews/` (dispositions files) |
| Task ownership and status | `kit/tasks.json` (definitions) and `claude/kit-state` `kit/state.json` (live status) |

## Responsibilities (proposed, owner approval required)

| Role | Who | May do | May not do |
|---|---|---|---|
| ISMS owner, risk owner, release authority, incident lead | Casey | Approve scope, risk acceptance, exceptions, objectives, live access and every production release | — |
| Backup for the above | None today (OPS-002, R-11) | — | See `owner-actions.md` P0: decide whether to accept this or name a deputy |
| Preparer | Claude sessions (Sonnet drafts, Opus reviews security-sensitive work) | Draft documents, map evidence, propose tasks, run checks | Approve its own privileged change, accept risk, sign an audit, or record a control as operating without a tool run or owner record |
| Internal auditor | To be named (`audit-and-review.md`) | Audit the ISMS | Audit work they did themselves |
| Counsel | To be engaged | Legal applicability, notice wording, contracts | — |

## Document control

Each file here carries a status line. A file becomes **approved** only when Casey records the approval as a decision (`docs/decisions.md`) naming the file and the commit SHA approved; the next review date is then one year later or on any trigger in `audit-and-review.md`. Git history is the version record. An AI-written status line saying "approved" is not an approval.

## Evidence rule: no secrets, no personal data, no confidential vendor material

This repository is **public**. Never put here: key values, key or token fingerprints, MFA seeds or recovery codes, passwords, account or project IDs, customer or homeowner personal data (fake data only, `BUILD_BRIEF.md:66`), vendor SOC reports or other confidential vendor security documents, penetration-test details of unfixed findings, or internal incident details. Record only a pointer: a repository path and SHA, a CI run number, or an opaque label for an item held in a private, access-controlled evidence store (`evidence-register.md`). `Ci`'s gitleaks and PII scans run over this folder like the rest of the tree.
