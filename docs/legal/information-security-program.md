Draft prepared with AI research; not reviewed by a lawyer.
Version 0.2 (draft, fix round 13) · October 28, 2026 · Internal document

# Ask Hava Written Information Security Program

A short written program for Ask Hava LLC, modeled on the FTC Safeguards Rule (16 C.F.R. Part 314) in its small-business form. Memo 2 (V.B) found it uncertain whether that rule applies to a quote-request service; Ask Hava follows this light version anyway because most of the controls are already in the build. The incident steps are in the Ask Hava Incident Response Plan.

## 1. Who is responsible

- **Qualified individual:** the Ask Hava owner. There is no other staff and no backup person (OPS-002).
- The owner reviews this program once a year, by [date one year after launch], and after any incident or any change to the systems or vendors below.

## 2. What we protect

| Data | Where it lives | Protection |
|---|---|---|
| Homeowner names, phone numbers, emails, addresses, on-site contacts, photos and message text | The contact vault only (PRIV-001) | Field-by-field AES-GCM encryption under a key kept apart from the database credentials (PRIV-002); every read logged (PRIV-003); only the messaging and release code can read it (PRIV-004) |
| Job descriptions with contact details removed | Job records | No contact details (PRIV-006, PRIV-008) |
| Business and team records | Business records | Signed-in, per-person access (SEC-004); every change logged (SEC-024) |
| Card data | Stripe only | Never touches Ask Hava's servers (FEE-014) |
| Consent and opt-out records | Consent log | Phone number encrypted; kept 5 years (CON-022, PRIV-007) |
| Ledger, payment and audit records | Ledger and audit log | Append-only (FEE-028, OPS-010); kept 7 years (PRIV-010) |

## 3. Risk assessment (yearly)

Each review lists the main risks and checks each control below still works. The starting list comes from `docs/threat-model.md`: account takeover of the owner or a business portal, contact details leaking to the wrong business, a scrubber miss before an AI call, a vendor breach, a lost backup, a texting opt-out missed, and photo metadata. Each review notes any new system, vendor or data type.

## 4. Controls

1. **Access:** owner sign-in with an argon2id password plus a passkey or TOTP, sessions of at most 12 hours, no email-only recovery (SEC-005, SEC-007); businesses sign in per person (SEC-004); homeowners with a texted code (SEC-008).
2. **Least data:** no Social Security numbers, driver's licenses or ID images (PRIV-012); the homeowner's phone number is never shared (PRIV-020); photos stripped of metadata at upload (PRIV-014).
3. **Encryption:** vault fields encrypted (PRIV-002); backups encrypted (PRIV-018); HTTPS with HSTS (SEC-012).
4. **Secrets:** no key in code, logs, fixtures or plain environment variables; secret scanning fails the build (SEC-001); separate test and live projects (SEC-003).
5. **Logging:** request IDs only, never phone numbers (PRIV-016); error monitoring without personal data (PRIV-017); an append-only audit log copied off the database as it happens (OPS-010).
6. **AI:** only scrubbed text, never photos; no AI call on live data until either the zero-retention agreement is signed or the owner switches on "AI on scrubbed text" after recording the provider's current API terms, with the privacy policy updated the same day (AI-001 to AI-008, D-412).
7. **Retention and deletion:** contact details purged 180 days after a job closes, photos at 30 days (PRIV-007, PRIV-009).
8. **Testing:** security tests on every change (SEC-019, SEC-020); dependency and static scans (SEC-021); a vulnerability scan before launch (SEC-023).
9. **Backups and recovery:** nightly encrypted export with a monthly test restore (OPS-013).

## 4a. Does the FTC Safeguards Rule apply? (reviewed yearly)

Ask Hava closely matches the rule's "finder" example (16 C.F.R. 314.2(h)(2)(xiii)), so it is treated as a financial institution. Whether a homeowner with one request is a "customer" with a continuing relationship is unclear, so this program follows the rule anyway. With fewer than 5,000 consumers' information, 16 C.F.R. 314.6 removes only four requirements: the written risk assessment's details (314.4(b)(1)), continuous monitoring or yearly penetration tests and twice-yearly vulnerability scans (314.4(d)(2)), the written incident-response plan (314.4(h)) and the yearly report (314.4(i)). Every other part of 314.4 still applies. Ask Hava keeps the risk assessment, the incident plan and the yearly report anyway; those are voluntary controls kept as policy. This review is repeated yearly and whenever the service changes.

## 4b. Data inventory

| System | What it holds | Who can see it | How long | Main control |
|---|---|---|---|---|
| Hand-run test: Google Forms and Sheet | Requests; first name, job address and email | Owner only | Names, addresses and emails deleted 90 days after the test | Two-step sign-in; sharing off |
| Hand-run test: Gmail | Requests, quotes, business emails | Owner only | Same | Two-step sign-in; no forwarding; AI features off |
| Stripe | Business payments; card data held by Stripe | Owner sees payment details, not card numbers | Financial-record schedule | Two-step sign-in |
| Live service: database and contact vault | As in the privacy policy | The app's vault readers only (architecture section 8) | As in the privacy policy | Field encryption; audit log |

## 5. Vendors

| Provider | What it does | Checked |
|---|---|---|
| Twilio | Texts, masked calls, codes | Yearly: security page or SOC 2 report; data-processing terms |
| Resend | Email | Same |
| Stripe | Payments | Same |
| Railway | Hosting and database | Same |
| Sentry | Error monitoring | Same |
| Lob | Identity letters | Same |
| Anthropic | AI reading (only once switched on) | Same, plus the zero-retention agreement or the recorded API terms (AI-008) |

Each provider gets only what it needs. A new provider is added to this list before it receives any data. Before a provider handles customer information, Ask Hava checks it can keep the information safe, uses terms that require it to do so where the Safeguards Rule applies, and reviews it again yearly based on how sensitive the information is. Google is added for the hand-run test.

## 6. Training

The owner re-reads this program and the incident plan at each yearly review, and before giving any other person access to any system.

## 7. Yearly report

At each review the owner writes a short note: what changed, the risks reviewed, any incident, and what will change. The note is kept with this program.
