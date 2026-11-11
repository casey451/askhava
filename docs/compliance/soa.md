# Statement of Applicability (skeleton)

**Status:** draft — **OWNER APPROVAL REQUIRED** for every applicability choice and exclusion (D-2740). Control references and short titles follow ISO/IEC 27001:2022 Annex A; the standard's text is not reproduced, and a licensed copy is needed before any certification work.

**Status values.** `Not started` — no design. `Designed` — written in a spec, plan or draft. `Designed, CI-tested` — code and a passing test in the build environment (`reconciliation.md`, "Test evidence used"). `Owner action` — only Casey or an outside party can supply it. **No row says "implemented" or "operating"**: operating evidence from the test or live environment does not exist yet. Operating evidence is filled only from `evidence-register.md` rows.

**Abbreviations.** Risk: `R-nn` = `docs/risk-register.md`; `TM Rn` = `docs/threat-model.md` section 6. Owner: C = Casey; T = the named task (build agents, reviewed). Op. ev. = operating evidence.

## 5 Organizational controls

| Ref | Short title | Appl. | Reason | Risk | Tasks / requirements | Owner | Design evidence | Op. ev. | Status |
|---|---|---|---|---|---|---|---|---|---|
| 5.1 | Security policies | Yes | Personal data and payments | — | WISP | C | `docs/legal/information-security-program.md` | Not yet | Designed (unapproved draft) |
| 5.2 | Roles and responsibilities | Yes | Required for any ISMS | R-11 | — | C | `README.md` roles | Not yet | Owner action |
| 5.3 | Segregation of duties | Yes | Agents build; owner approves | R-14, R-37, R-13 | N-01 | C | `docs/governance.md` s.1–2; `.github/CODEOWNERS` | Not yet | Designed |
| 5.4 | Management responsibilities | Yes | Owner accountability | — | — | C | `audit-and-review.md` | Not yet | Owner action |
| 5.5 | Contact with authorities | Yes | Breach notice | — | OPS-ISMS-04 | C | `docs/legal/incident-response-plan.md` s.5 | Not yet | Designed (needs counsel) |
| 5.6 | Special interest groups | Yes | Vendor and advisory feeds | — | — | C | — | Not yet | Not started |
| 5.7 | Threat intelligence | Yes | Dependency advisories | — | Dependabot, cargo-audit | C | `.github/dependabot.yml`; `ci.yml:290` | Build env only | Designed, CI-tested |
| 5.8 | Security in project management | Yes | Every task carries security review | — | `kit/tasks.json` reviewer_model | C | `docs/governance.md` | Build env only | Designed, CI-tested |
| 5.9 | Asset inventory | Yes | — | — | — | C | `asset-data-vendor-inventory.md`; WISP 4b | Not yet | Designed |
| 5.10 | Acceptable use | Yes | Owner devices and accounts | — | OPS-ISMS-05 | C | — | Not yet | Not started |
| 5.11 | Return of assets | Yes, when staff exist | No staff today | — | OPS-ISMS-08 | C | — | Not yet | Not started |
| 5.12 | Classification | Yes | Vault vs non-vault data | — | PRIV-001 | C | `asset-data-vendor-inventory.md` data classes | Not yet | Designed |
| 5.13 | Labelling | Yes, light | Document status lines | — | — | C | `README.md` document control | Not yet | Designed |
| 5.14 | Information transfer | Yes | Release to businesses, vendor APIs | TM R2 | PRIV-027, SEC-012 | T | `docs/spec/privacy.md`; `tests/security/tests/no_phone_released.rs` | Not yet | Designed, CI-tested |
| 5.15 | Access control | Yes | Portals, vault readers | — | SEC-004, PRIV-004; T088 | T | `tests/security/tests/auth_matrix.rs`, `vault_readers.rs` | Not yet | Designed, CI-tested |
| 5.16 | Identity management | Yes | Per-person business sign-in; agents vs owner | R-14 | SEC-004; N-01 | C | `docs/spec/security.md` | Not yet | Designed |
| 5.17 | Authentication information | Yes | Codes, links, passkeys, recovery codes | TM R1, R2, R18 | SEC-005, SEC-007, SEC-008 | T | `tests/security/tests/owner_auth.rs` | Not yet | Designed, CI-tested |
| 5.18 | Access rights | Yes | Owner, bot and vendor accounts | R-37 | TECH-02, TECH-17 | C | — | Not yet | Owner action |
| 5.19 | Supplier relationships | Yes | Ten-plus providers | R-10 | OPS-ISMS-02 | C | WISP s.5 | Not yet | Designed |
| 5.20 | Supplier agreements | Yes | Processing terms | — | OPS-ISMS-02; AI-008 | C | — | Not yet | Owner action |
| 5.21 | ICT supply chain | Yes | Crates, npm, Actions | R-36 | T345, T121; N-03 | T | `ci.yml` pins; `kit/allowed_crates.txt` | Build env only | Designed, CI-tested |
| 5.22 | Supplier monitoring and change | Yes | Yearly recheck | — | WISP s.5 | C | WISP s.5 | Not yet | Designed |
| 5.23 | Cloud services | Yes | Hosting, storage, SaaS | — | OPS-012, PRIV-029 | C | `docs/architecture.md` s.9 | Not yet | Designed |
| 5.24 | Incident planning | Yes | — | — | T117 | C | `docs/legal/incident-response-plan.md` | Not yet | Designed (needs counsel) |
| 5.25 | Event assessment | Yes | Alerts and safe mode | — | T105, T204 | C | OPS-001, OPS-023 | Not yet | Designed |
| 5.26 | Incident response | Yes | — | — | OPS-021; T117 | C | Incident plan s.3 | Not yet | Designed |
| 5.27 | Learning from incidents | Yes | — | — | — | C | Incident plan s.7; `audit-and-review.md` | Not yet | Designed |
| 5.28 | Evidence collection | Yes | Log preservation | — | OPS-021, OPS-010 | C | Off-database stream | Not yet | Designed, CI-tested (stream) |
| 5.29 | Security during disruption | Yes | Safe mode, texting switch | — | OPS-001; T105; T279 | T | `docs/spec/ops-recovery.md` | Not yet | Designed |
| 5.30 | ICT continuity | Yes | Restore and replay | R-11 | T109, T110, T206, T139 | C | OPS-013 to OPS-016 | Not yet | Designed (queued) |
| 5.31 | Legal and contractual requirements | Yes | Privacy, SMS, payment, licensing law | — | OPS-ISMS-01 | C + counsel | WISP 4a (AI draft) | Not yet | Owner action |
| 5.32 | Intellectual property | Yes | Licenses of dependencies | — | cargo-deny licenses | T | `ci.yml:290`–`295` | Build env only | Designed, CI-tested |
| 5.33 | Protection of records | Yes | 7-year ledger and audit | — | PRIV-010, OPS-010 | T | append-only audit log | Not yet | Designed, CI-tested |
| 5.34 | Privacy and PII | Yes | Core of the service | TM R1–R20 | `docs/spec/privacy.md` | T + C | vault, purge, sentinel tests | Not yet | Designed, CI-tested |
| 5.35 | Independent review | Yes | — | R-13 | T125, T140 (AI, not independent); TECH-15 | C | — | Not yet | Owner action |
| 5.36 | Compliance with policies | Yes | — | — | `Ci`, `Governance` | C | `docs/governance.md` s.2 | Build env only | Designed, CI-tested |
| 5.37 | Documented procedures | Yes | Runbook | — | T117, T123 | T | — | Not yet | Designed (queued) |

## 6 People controls

| Ref | Short title | Appl. | Reason | Risk | Tasks / requirements | Owner | Design evidence | Op. ev. | Status |
|---|---|---|---|---|---|---|---|---|---|
| 6.1 | Screening | Not now | No staff; applies before first hire | — | OPS-ISMS-08 | C | — | — | Proposed exclusion until staff |
| 6.2 | Terms of employment | Not now | Same | — | OPS-ISMS-08 | C | — | — | Proposed exclusion until staff |
| 6.3 | Awareness and training | Yes | Owner; future staff | — | WISP s.6 | C | WISP s.6 | Not yet | Owner action |
| 6.4 | Disciplinary process | Not now | No staff | — | — | C | — | — | Proposed exclusion until staff |
| 6.5 | After termination | Yes, when staff exist | Revocation drill | — | OPS-ISMS-08 | C | — | Not yet | Not started |
| 6.6 | Confidentiality agreements | Yes | Contractors, advisers | — | — | C | — | Not yet | Not started |
| 6.7 | Remote working | Yes | Owner works remotely | — | OPS-ISMS-05 | C | — | Not yet | Owner action |
| 6.8 | Event reporting | Yes | Alerts and vendor notices | — | T204 | C | Incident plan s.2 | Not yet | Designed |

## 7 Physical controls

No premises; providers' facilities are inherited through 5.19 to 5.23. Owner endpoints are covered by 7.7 to 7.10, 7.14 and 8.1.

| Ref | Short title | Appl. | Reason | Owner | Status |
|---|---|---|---|---|---|
| 7.1 | Physical perimeters | No (proposed) | No office; hosted by providers | C | Proposed exclusion |
| 7.2 | Physical entry | No (proposed) | Same | C | Proposed exclusion |
| 7.3 | Offices, rooms, facilities | No (proposed) | Same | C | Proposed exclusion |
| 7.4 | Physical monitoring | No (proposed) | Same | C | Proposed exclusion |
| 7.5 | Physical and environmental threats | No (proposed) | Inherited from providers | C | Proposed exclusion |
| 7.6 | Secure areas | No (proposed) | Same | C | Proposed exclusion |
| 7.7 | Clear desk and screen | Yes | Owner devices | C | Owner action |
| 7.8 | Equipment siting | Yes, light | Owner devices at home | C | Owner action |
| 7.9 | Assets off-premises | Yes | Laptop and phone travel | C | Owner action |
| 7.10 | Storage media | Yes | Paper key escrow (PRIV-024), exports | C | Designed (T117 inventory) |
| 7.11 | Supporting utilities | No (proposed) | Inherited from providers | C | Proposed exclusion |
| 7.12 | Cabling | No (proposed) | Same | C | Proposed exclusion |
| 7.13 | Equipment maintenance | Yes, light | Device updates (OPS-ISMS-05) | C | Owner action |
| 7.14 | Secure disposal | Yes | Old devices, paper escrow | C | Not started |

## 8 Technological controls

| Ref | Short title | Appl. | Reason | Risk | Tasks / requirements | Owner | Design evidence | Op. ev. | Status |
|---|---|---|---|---|---|---|---|---|---|
| 8.1 | User endpoint devices | Yes | Owner laptop and phone | R-11 | OPS-ISMS-05 | C | — | Not yet | Owner action |
| 8.2 | Privileged access | Yes | Owner portal; vendor consoles; bot tokens | R-14, R-37 | T086 DONE; N-01 | C | `tests/security/tests/owner_auth.rs` | Not yet | Designed, CI-tested (app) |
| 8.3 | Access restriction | Yes | Vault readers, per-business data | — | PRIV-004; T088 | T | `vault_readers.rs`, `auth_matrix.rs` | Not yet | Designed, CI-tested |
| 8.4 | Access to source code | Yes | Public repo; write access | R-14, R-37 | rulesets; N-01 | C | `docs/governance.md` s.2 | Build env only | Designed |
| 8.5 | Secure authentication | Yes | — | TM R1, R2, R18 | SEC-004 to SEC-008; T272, T346 | T | `owner_auth.rs`, `business_signin.rs`, `step_up.rs` | Not yet | Designed, CI-tested |
| 8.6 | Capacity management | Yes | Peaks, AI cost | — | T297, T236 | T | — | Not yet | Designed (queued) |
| 8.7 | Malware protection | Yes, light | Uploads; owner devices | — | PRIV-025 decode limits | T + C | `docs/spec/privacy.md` | Not yet | Designed |
| 8.8 | Technical vulnerabilities | Yes | — | R-36 | cargo-audit, npm audit; T138 ZAP | C | `ci.yml:290`–`307` | Build env only | Designed, CI-tested |
| 8.9 | Configuration management | Yes | Host settings table | — | T135 (`docs/deploy.md`) | T | — | Not yet | Designed (queued) |
| 8.10 | Information deletion | Yes | Retention schedule | — | PRIV-007, PRIV-009; T092; N-02 | T | `retention_purge.rs` | Not yet | Designed, CI-tested (app) |
| 8.11 | Data masking | Yes | Scrubbed text to AI; masked calls | — | AI-001; PRIV-020 | T | `scrubbed_text_compile_fail.rs` | Not yet | Designed, CI-tested |
| 8.12 | Data leakage prevention | Yes | No contact before paid pick | — | PRIV-027; T120; N-02 | T | `no_phone_released.rs`, `sentinel_db_logs.rs` | Not yet | Designed, CI-tested |
| 8.13 | Information backup | Yes | — | — | T109, T206, T139 | T + C | OPS-013 | Not yet | Designed (queued) |
| 8.14 | Redundancy | Yes, light | Single host; restore is the plan | R-11 | OPS-012 | C | — | Not yet | Owner decision |
| 8.15 | Logging | Yes | — | — | PRIV-016, OPS-010, OPS-011 | T | `offdb_stream.rs`, `logging.rs` | Not yet | Designed, CI-tested |
| 8.16 | Monitoring | Yes | Alerts, chain check | — | T105, T204, T366, T367 | T + C | `CHAIN_ALERT` | Not yet | Designed (partly queued) |
| 8.17 | Clock synchronization | Yes, inherited | Host clocks | — | — | C | — | Not yet | Inherited (to confirm) |
| 8.18 | Privileged utilities | Yes, light | DB console, migrate service | — | SEC-052 | C | `docs/spec/security.md:56` | Not yet | Designed |
| 8.19 | Software on operational systems | Yes | Deploy only from `main` | — | OPS-022; T135 | C | `docs/spec/ops-recovery.md:26` | Not yet | Designed |
| 8.20 | Network security | Yes, inherited + app | HTTPS, HSTS | — | SEC-012; T362 | T | `headers.rs` | Not yet | Designed, CI-tested (headers) |
| 8.21 | Network services | Yes | Webhook signatures | — | SEC-049 | T | `webhook_forgery.rs`, `replay.rs` | Not yet | Designed, CI-tested |
| 8.22 | Network segregation | Yes | Test and live separated | — | SEC-003 | C | `docs/architecture.md:377` | Not yet | Designed |
| 8.23 | Web filtering | No (proposed) | No staff browsing estate | — | — | C | — | — | Proposed exclusion |
| 8.24 | Cryptography | Yes | Vault, photos, export, links | — | PRIV-002, PRIV-023, PRIV-024; T354 | T + C | `vault.rs`; TD-PRIV-09 | Not yet | Designed, CI-tested (app) |
| 8.25 | Secure development life cycle | Yes | — | — | `BUILD_BRIEF.md` s.12 | T | `docs/governance.md` | Build env only | Designed, CI-tested |
| 8.26 | Application security requirements | Yes | ASVS L2 | — | `docs/spec/security.md` | T | `docs/threat-model.md` s.5 | Build env only | Designed, CI-tested |
| 8.27 | Secure architecture | Yes | Module layers, vault isolation | — | `kit/layers.json` | T | `docs/architecture.md` s.3–6 | Build env only | Designed, CI-tested |
| 8.28 | Secure coding | Yes | — | — | clippy, `check_rust_rules.py` | T | `ci.yml` | Build env only | Designed, CI-tested |
| 8.29 | Security testing | Yes | — | — | `tests/security/`; T138; TECH-15 | T + C | `tests/security/tests/` | Build env only | Designed, CI-tested |
| 8.30 | Outsourced development | Yes | AI agents write the code | R-13, R-20 | `docs/governance.md` | C | reviewer_model per task | Build env only | Designed |
| 8.31 | Separate dev, test, prod | Yes | — | — | SEC-003; architecture s.9 | C | `docs/architecture.md:366`–`377` | Not yet | Designed |
| 8.32 | Change management | Yes | — | R-07 | `Governance`, `kit/plan.lock` | C | `docs/governance.md` s.1–3 | Build env only | Designed, CI-tested |
| 8.33 | Test information | Yes | Fake data only | — | `BUILD_BRIEF.md:66`; PII scan | T | `ci.yml:113` | Build env only | Designed, CI-tested |
| 8.34 | Protection during audit testing | Yes | Pentest and ZAP on test project only | — | T138; TECH-15 | C | — | Not yet | Designed |
