# Security objectives (draft)

**Status:** draft — **OWNER APPROVAL REQUIRED** for every target (D-2740). Targets apply from Gate C unless a row says "now". Each month's figures go in one scorecard row below; the source column says where the figure comes from, so a figure without a source is not recorded.

## Objectives

| # | Objective | Measure | Target (proposal) | Source of the figure | Starts |
|---|---|---|---|---|---|
| O-1 | Privileged accounts use a phishing-resistant or app second factor | Owner, GitHub, host, Stripe, Twilio, storage, email, monitoring and AI consoles with second factor and offline recovery / all such accounts | 100% | Owner's account review (opaque labels in the private store) | Before Gate C |
| O-2 | Backups restore | Monthly test restore succeeded, with measured restore time and data-loss window within the approved RTO/RPO | 12 of 12 months; first at T139 | OPS-013 digest line; T139 record | Gate C |
| O-3 | High findings fixed fast | Critical/High vulnerability or audit finding: days from report to fix merged (or signed exception) | Critical 7 days, High 30 days, Medium 90 days | `cargo audit`, `npm audit`, Dependabot, ZAP, audit dispositions | Now |
| O-4 | Audit trail unbroken | Nights the audit hash chain verified / nights | 100%, each break investigated within 1 day | `ops.audit_chain_broken` alerts (none = pass) | Gate C |
| O-5 | No personal data where it must not be | PII-scan and gitleaks hits on `main`; sentinel hits (T120, N-02) | 0 | `Ci` runs; sentinel reports | Now |
| O-6 | Access reviewed | Quarterly review of every account, token and key holder done, with removals recorded | 4 per year | `audit-and-review.md` records | Gate C |
| O-7 | Suppliers reviewed | Providers with a current yearly review / providers holding customer data | 100% | `asset-data-vendor-inventory.md` "last review" column | Before Gate C |
| O-8 | Alerts reach a human | Critical alerts acknowledged by Casey within 24 hours (else safe mode, OPS-001) | 100% | Ops alert records | Gate C |

## Reporting cadence

- **Monthly** (first business day): one scorecard row, written by a Claude session from the sources above and checked by Casey.
- **Quarterly:** access review (O-6).
- **Yearly:** management review (`audit-and-review.md`) re-sets the targets; also the WISP's yearly review (`docs/legal/information-security-program.md:11`).

## Scorecard

| Month | O-1 | O-2 | O-3 | O-4 | O-5 | O-6 | O-7 | O-8 | Notes | Checked by |
|---|---|---|---|---|---|---|---|---|---|---|
| (first row after approval) | | | | | | | | | | |
