# Internal audit, management review and corrective action

**Status:** templates (D-2722). The program itself starts only when Casey approves it and controls have operated long enough to sample (after Gate C).

## 1. AI review is not an independent ISMS audit

A fresh Opus session reviewing code (each task's reviewer, T125, T140) is engineering review. It is valuable and stays in the plan, but it is **not** an independent internal audit of the ISMS: the reviewer runs on the same account and runner as the builder (R-13, `docs/risk-register.md:23`), it cannot interview people or inspect consoles, and it cannot accept risk. An AI session may prepare an audit pack (evidence pointers, sample lists) but the auditor is a person who did not do the work.

## 2. Internal audit program

| Field | Proposal |
|---|---|
| Frequency | Once a year, and before any certification stage-1 audit |
| Auditor | A person independent of the work audited (an outside consultant or a qualified peer); named by Casey |
| Criteria | `isms-scope.md`, `soa.md`, `security-objectives.md`, the WISP and the incident plan |
| Sample | At least: two releases, one restore record, one access review, two supplier reviews, three closed findings, one incident or drill |
| Output | Findings (nonconformity, observation, opportunity), each with an owner and a due date in the corrective-action log |

### Audit record template

| Field | Value |
|---|---|
| Audit ID and dates | |
| Auditor and independence statement | |
| Scope and criteria | |
| Evidence sampled (evidence-register IDs) | |
| Findings (ID, type, control, description) | |
| Retest date and result | |

## 3. Management review

Held by Casey at least yearly (aligned with the WISP's yearly review, `docs/legal/information-security-program.md:11`) and after any significant incident. A Claude session may prepare the inputs; the decisions and signature are Casey's.

### Review record template

| Input | Summary |
|---|---|
| Status of actions from the last review | |
| Changes in scope, laws, suppliers or data | |
| Objectives scorecard (`security-objectives.md`) | |
| Incidents, alerts and drills | |
| Audit and external test results | |
| Risk register changes and residual risks above appetite | |
| Corrective actions open and overdue | |

| Output | Record |
|---|---|
| Decisions (scope, objectives, resources) | |
| Risks accepted (ID, residual score, expiry) | |
| Actions (owner, due) | |
| Signed by Casey, date, decision entry | |

## 4. Corrective-action log

Each finding from an audit, incident, drill, external test or failed objective becomes one row. A Claude session may draft the cause and propose the fix (as a task, through a plan change when it changes the plan); closure needs evidence and, for High or Critical, a reviewer other than the fixer.

| CA ID | Source (audit, incident, test) | Finding | Root cause | Action (task ID) | Owner | Due | Evidence of fix (evidence-register ID) | Verified by | Closed |
|---|---|---|---|---|---|---|---|---|---|

## 5. Risk acceptance and exceptions

For a residual risk above appetite (`isms-scope.md` section 5) or a deliberate departure from a control (for example `docs/threat-model.md` section 6, R2 and R18):

| Field | Value |
|---|---|
| Risk or exception ID | |
| What is accepted and why | |
| Compensating controls | |
| Residual score | |
| Expiry or review trigger | |
| Accepted by Casey, date, decision entry | |

## 6. Review triggers

Re-review the scope, SoA and risk scores when: a provider or data class is added, a security incident occurs, an audit or external test reports Critical or High, staff or contractors are added, the service enters a new state, or Gate C is passed.
