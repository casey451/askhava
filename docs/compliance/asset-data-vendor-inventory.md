# Asset, data and provider inventory

**Status:** draft (D-2722); the owner completes the cells marked **owner**. Reconciled with the WISP (`docs/legal/information-security-program.md` sections 2, 4b and 5) and `docs/architecture.md` sections 7 and 9 on `main` `f8231346`. **No account IDs, project IDs, key values or key fingerprints go in this file**; secret names below are the setting names already public in the architecture.

## 1. Data classes

| Class | Examples | Where | Retention (requirement) | Protection |
|---|---|---|---|---|
| D1 Homeowner contact | Name, phone, email, address, on-site contact | Contact vault only (PRIV-001) | 180 days after job close unless held; phone with consent records 5 years (PRIV-007, CON-022) | Field encryption, audited reads, listed readers (PRIV-002 to PRIV-004) |
| D2 Homeowner photos | Job photos | Object storage, app-encrypted (PRIV-023) | 30 days after close (PRIV-009) | AES-GCM under the vault key; metadata stripped (PRIV-014) |
| D3 Message bodies at the SMS provider | Texts sent and received | Twilio | Deleted through the API 31 days after receipt (PRIV-027) | Redaction job (T276 DONE) |
| D4 Job text (contact removed) | Request description | Job records | 3 years (PRIV-008) | Scrubbed before storage and AI (PRIV-006) |
| D5 Business and team records | Business name, members, notice addresses | Business records | On leaving, ledger and audit kept 7 years (BIZ-051); the rest of the profile: **owner** | Per-person sign-in (SEC-004); change log (SEC-024) |
| D6 Consent and opt-out | Consent text version, keyword events | Consent log | 5 years after last message (CON-022) | Phone encrypted |
| D7 Money and audit | Ledger, charges, refunds, audit log | Ledger, audit log, off-database stream | 7 years (PRIV-010) | Append-only (FEE-028, OPS-010); hash chain (PRIV-029) |
| D8 Card data | Card numbers | Stripe only | Stripe's | Never on Ask Hava servers (FEE-014); PCI scope **owner** (TECH-10) |
| D9 Operational logs | Request IDs, routes, status codes | Host log store; request log | 30 days (PRIV-029; host retention) | No contact details (PRIV-016) |
| D10 Error events | Stack traces | Sentry | **owner** (set in Sentry) | No default PII plus scrubber (PRIV-017) |
| D11 Backups | Nightly encrypted export | Object storage | 30 days, object lock (OPS-013, PRIV-029) | Encrypted to an offline recipient key (PRIV-024) |
| D12 Secrets | `VAULT_KEY`, `VAULT_INDEX_KEY`, `ID_SECRET`, photo-link key, export key, vendor keys | Host sealed variables; offline paper escrow | Until rotated | Inventory with owner, storage, escrow and rotation trigger in the runbook (PRIV-024, T117); never in the repo |
| D13 Source and build state | Code, plan, runner state | GitHub | Indefinite | Public repo; fake data only (`BUILD_BRIEF.md:66`); gitleaks and PII scan |

## 2. Systems and environments

| System | Environment | Data classes | Access | Notes |
|---|---|---|---|---|
| Build sessions and CI | Build, CI | D13; fake data only | Agents, Actions | No vendor keys in Phase 1 (`docs/architecture.md:370`–`371`) |
| Railway test project (`web`, `worker`, `migrate`, PostgreSQL 16) | Test | Fake data only | Casey sets up; Claude deploys `main` | T135 |
| Railway live project (same services) | Live | D1 to D12 | Casey only; Claude never holds a hosting key (OPS-022) | Live keys entered by Casey at Gate C |
| Object storage bucket | Test and live | D2, D7 stream, D11 | App key write-only, prefix-scoped; photo prefix own key (PRIV-029) | One bucket per project (SEC-003) |
| askhava.com | Live | Public pages | Casey | Domain registrar and DNS provider: **owner** to record |
| Owner endpoints | — | Console access to all of the above | Casey | Baseline: `owner-actions.md` |

## 3. Providers

| Provider | Purpose | Data classes | In WISP s.5 | In architecture s.7 | Last review | Agreement on file |
|---|---|---|---|---|---|---|
| Railway (or Render, OPS-012) | Hosting, database, logs | D1–D12 | Yes | Yes (s.9) | **owner** | **owner** |
| Backblaze B2 | Object storage, backups | D2, D7, D11 | Yes | Yes | **owner** | **owner** |
| Twilio | Texts, masked calls, Verify | D1 (phone), D3 | Yes | Yes | **owner** | **owner** |
| Stripe | Payments | D7, D8; business billing | Yes | Yes | **owner** | **owner** |
| Resend | Email | D1 (email), D5 | Yes | Yes | **owner** | **owner** |
| Lob | Identity letters | D5 (business address) | Yes | Yes | **owner** | **owner** |
| Sentry | Error monitoring | D10 | Yes | No (PRIV-017 only) | **owner** | **owner** |
| Anthropic API | AI reading of scrubbed text | D4 only, once switched on (AI-008) | Yes | Yes | **owner** | Zero-retention agreement or recorded terms (AI-008) |
| Agency license lists | License verification | Public business license data | No | Yes (`license_sources`) | n/a (public data) | n/a |
| GitHub | Source, CI, runner state, rulesets | D13 | **No** | Yes (s.9 CI) | **owner** | **owner** |
| Anthropic (build agents) | Writes and reviews code | D13; fake data only | **No** | — | **owner** | **owner** |
| Google (hand-run test) | Forms, sheet, mail | Hand-run test data | In s.4b and a sentence, not the table | — | **owner** | **owner** |
| Domain registrar and DNS | askhava.com | — | **No** | — | **owner** | **owner** |

## 4. Reconciliation notes (for the WISP's next revision; not edited here)

1. The WISP vendor table (section 5) omits GitHub and the build-agent provider, which hold the source and the runner's tokens though no customer data; and the domain registrar/DNS provider, whose compromise would let an attacker take over the site and email sending.
2. The WISP data inventory (section 4b) lists the host, the backup bucket, Stripe and the hand-run test, but not Twilio (message bodies, 31 days, PRIV-027), Resend, Sentry, Lob or the AI provider, which appear only in the vendor table.
3. Retention for the non-ledger part of D5 (business profile after leaving) and D10 (Sentry events) is not stated in the spec; Casey sets it (or counsel advises) and the privacy policy follows.
4. The WISP keeps its "not reviewed by a lawyer" banner and is not represented as approved; it is updated only after `isms-scope.md` is approved.
