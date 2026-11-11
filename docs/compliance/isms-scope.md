# ISMS scope (draft)

**Status:** draft — **OWNER APPROVAL REQUIRED** (D-2740). Not in force until Casey records approval as a decision naming this file's commit SHA.

## 1. Organization

Ask Hava LLC, a quote-request introduction service for homeowners and local businesses, operated by one person (the owner, Casey). No employees or contractors today. Software is written by AI build agents (Claude) under the owner's direction (`BUILD_BRIEF.md`, `docs/governance.md`).

## 2. In scope

| Area | Included | Notes |
|---|---|---|
| People | The owner; any future staff or contractor before they receive access | Build agents are treated as a supplier-run development process (Annex A 8.30), not as staff |
| Information | Homeowner contact details, photos and messages; business and team records; consent and opt-out records; ledger, payment and audit records; job text with contact details removed | Data classes in `asset-data-vendor-inventory.md` |
| Application | The Rust workspace (`app/`), migrations, web and worker services, the `migrate` service | `docs/architecture.md` sections 1, 7, 9 |
| Environments | Build and CI (fake data only), Railway test project (fake data only), Railway live project (real data, after Gate C) | `docs/architecture.md:366`–`377` |
| Source and change control | The GitHub repository, branch rulesets, Actions workflows, the runner and its state branch | `docs/governance.md` sections 1, 2, 5 |
| Providers | Hosting, database, object storage, messaging, voice, email, letters, payments, error monitoring, AI, source hosting and build agents | `asset-data-vendor-inventory.md` |
| Recovery | Nightly encrypted export, off-database stream, restore and replay | OPS-013 to OPS-016 |
| Website | askhava.com pages served by the app | Domain registrar and DNS to be recorded by the owner |
| Owner endpoints | The owner's laptop, phone, password manager and hardware keys | Baseline in `owner-actions.md` |

## 3. Out of scope (proposed exclusions, owner to confirm)

| Exclusion | Reason |
|---|---|
| Providers' own data centres and internal controls | Inherited; managed through supplier assessment (Annex A 5.19 to 5.23), not audited by Ask Hava |
| Businesses' own systems after a paid release | Outside Ask Hava's control; the business terms govern their use of released details |
| The hand-run test (forms, spreadsheet, mail) once its data is deleted | Pre-launch and time-limited (`docs/legal/information-security-program.md:48`–`49`); in scope until its 90-day deletion is done |
| Physical premises | No office; the owner works from home with company devices covered as endpoints |

## 4. Locations

The owner's home office (endpoints only) and the providers' hosted regions. Region details are recorded per provider by the owner; none are stored here.

## 5. Risk method (proposal)

Keeps `docs/risk-register.md` (R-IDs and history) and `docs/threat-model.md` section 6 as the sources; adds scoring only.

- **Likelihood** 1 to 3: 1 unlikely in a year; 2 possible in a year; 3 expected in a year.
- **Impact** 1 to 3: 1 inconvenience, no personal data; 2 one customer's data exposed or a wrong charge; 3 many customers, money loss, legal notice or service stop over a day.
- **Score** likelihood × impact. **Appetite (proposal):** residual score 1 to 2 accepted by the risk owner; 3 to 4 needs a dated treatment or a signed acceptance with a review date; 6 to 9 is not accepted for launch.
- **Each risk records** inherent score, treatment (task IDs), residual score, owner, review date, and Casey's acceptance where residual stays above 2.
- **Reassess** at each management review and on: a new provider, a new data class, a security incident, a Critical or High audit finding, or a change to an approved scope line.

Where the score lives (a column added to the register, or a separate table here linking R-IDs) is Casey's call; nothing is moved or renumbered.

## 6. Approval

| Field | Value |
|---|---|
| Approved by | (Casey) |
| Decision | (D-### naming this file's SHA) |
| Next review | (one year after approval, or on a trigger in `audit-and-review.md`) |
