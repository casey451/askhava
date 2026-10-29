# Brain: Payments with Stripe

- **Purpose:** the right Stripe objects, calls and events for Ask Hava's introduction fee, and where the current plan meets or misses Stripe's rules.
- **Who reads it:** any session building or reviewing the `payments` crate (charges, saved cards, refunds, disputes, ledger, nightly reconcile), the Confirm page in `portals`, the release path (PICK-022, SEC-030), and any task whose prompt touches FEE-*, PICK-021, PICK-022, PICK-048 or PICK-056.
- **How to use it:** read the section for your task, run the review checklist (section 15) before you open the PR, and cite the source tag (`[P#]`, list in section 16) when you rely on a rule.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec, and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = not backed by a primary source; reasoning only.

## 1. The flow the plan uses (and one wording trap)

The plan does **not** save a card at sign-up. Sign-up creates a Stripe Customer with no card (BIZ-001, FEE-036). A card is entered or chosen on the pick's Confirm page, where "Confirm and pay $<fee>" is one on-session step; saving it is an optional unticked box (FEE-014, FEE-015, D-365). If a task prompt says "card saved at sign-up", the spec wins: build FEE-036.

Flow for one pick with a fee due:

1. Business opens the Confirm review step (PICK-021). Server re-checks the pick and job state (FEE-017) and computes the fee (FEE-001 to FEE-005).
2. Server creates the PaymentIntent: `amount` (server-computed cents), `currency=usd`, `customer`, card only, `description` "Ask Hava introduction service, job <n>" (FEE-031), `statement_descriptor_suffix` (section 4), `metadata` {pick ID, job number, tier, terms and authorization versions, environment}, and `setup_future_usage` only if the save box is ticked (section 2). It sends an idempotency key (section 5).
3. The page mounts the Payment Element with the client secret and calls `stripe.confirmPayment` (or `confirmCardPayment` for a saved card) on the press. If the bank asks for 3D Secure, Stripe.js shows it in the page; the PaymentIntent moves to `requires_action` and then `succeeded` or `requires_payment_method` [P1][P3].
4. The server never trusts the browser's result. It releases details only from a signed `payment_intent.succeeded` event or a PaymentIntent fetched with `status=succeeded` (PICK-022, SEC-030, SEC-039). Stripe says the same: "Don't attempt to handle order fulfillment on the client side ... use webhooks to monitor the `payment_intent.succeeded` event" [P3].
5. A decline returns the PaymentIntent to `requires_payment_method`; the page shows it and the business can try again or use another card for the FEE-020 window. Stripe: "Reuse the same PaymentIntent to continue tracking the customer's purchase." [P3]

Stripe Connect is **not** needed: Ask Hava charges its own fee to its own customer (the business) and pays no one out. Connect is for platforms that route money to sellers [P14]. Collecting job payments for businesses would be "payment facilitation and aggregation", a restricted category [P12]; FEE-009 and TERMS-026 already rule that out.

## 2. SetupIntent or PaymentIntent, on_session or off_session

- **SetupIntent** saves a card without charging it [P2]. Use it only where a card is saved with no charge: the $0 Confirm page's save box (FEE-036 "saving there charging nothing").
- **PaymentIntent with `setup_future_usage`** charges now and saves the card for later [P2]. Use it on the paid Confirm page when the save box is ticked.
- **`on_session`** "lets the bank know you plan to use the card when the customer is available to authenticate". **`off_session`** (the SetupIntent default) "properly authenticates a credit or debit card for off-session payments" and also covers on-session use. Usage "is an optimization, not a hard limit"; a recovery path is still needed [P2].
- **Off-session needs a mandate.** Your terms must give permission to initiate payments, the expected frequency and how amounts are set; saved cards for on-session reuse need explicit consent such as a "save for future use" checkbox [P2]. FEE-015 (save) and the PICK-048 box (auto-confirm) are those agreements.
- **Where off-session is permitted in the plan:** only the PICK-048 auto-confirm charge, only while Stripe's written answer is filed, only for a pick made with a Verify code or a signed-in homeowner session (PICK-056), with its own idempotency key (FEE-016). Everything else is on-session (FEE-014).

## 3. SCA and 3D Secure in the US

- SCA is an EEA, Switzerland and UK rule [P4]. Ask Hava's businesses hold US cards, so SCA does not apply as law [inferred from P4].
- Stripe still triggers 3DS when "requested by an issuer with a soft decline code", by Radar rules, or on request; off-session payments can't do 3DS [P3]. So a US issuer can still demand authentication.
- Off-session charges that need authentication fail with `authentication_required`; the fix is to bring the customer back on-session [P5]. PICK-048 already falls back to the normal Confirm step and window.
- Merchant-initiated transactions (off-session with a mandate, card authenticated when saved) fall outside SCA [P4]. That is why a card used for PICK-048 should be saved with `off_session` (proposal 2, section 14).
- 3DS success makes a liability shift on fraud disputes likely but never guaranteed; respond to every inquiry on a 3DS charge or the liability shift can be lost [P3].

## 4. Statement descriptor and description

- A full descriptor is 5 to 22 Latin characters with at least one letter, none of `< > \ ' " *`, and must reflect the business name. A card prefix is 2 to 10 characters; Stripe joins prefix, `*`, a space and the per-charge suffix [P6].
- **For card payments you can't set `statement_descriptor`; only `statement_descriptor_suffix`.** [P6]
- Suggested: prefix `ASKHAVA`, suffix `JOB 1042` ("ASKHAVA* JOB 1042", 17 characters) [inferred]. "Clear and accurate statement descriptors can reduce chargebacks and disputes." [P6]
- `description` is a separate field shown on receipts and in reports, not on the bank statement [inferred from P6, P11]; FEE-031 fixes its wording.

## 5. Idempotency

From Stripe's API reference [P7]:
- Keys are up to 255 characters; don't put email addresses or personal identifiers in them.
- Stripe saves the status code and body of the first request once the endpoint starts executing, and returns it on retries "including `500` errors". Validation failures and requests that collide with a concurrent one are not saved and may be retried.
- Keys can be pruned after 24 hours; a reused key after that creates a new request.
- A retry with different parameters is an error. All POSTs accept keys; GET and DELETE don't need them.

For Ask Hava: FEE-016's per-attempt keys (`pick-1042-try1`, `try2`) fit these rules (no personal data). Two traps:
- **After 24 hours a key no longer protects.** OPS-016's replay recreates "Stripe charge keys from derived IDs"; a replay more than 24 hours after the original can create a second charge. FEE-017 (check state before every attempt) and FEE-037 (reconcile refunds a second kept charge) are the guards; keep them.
- **One PaymentIntent can succeed only once** [P3]. Creating a new PaymentIntent per press lets two tabs each succeed [inferred]. Proposal 1 (section 14) makes the pick, not the press, own the PaymentIntent.

## 6. Webhooks

**Signature** [P8][P9]: the `Stripe-Signature` header looks like `t=...,v1=...`; verify the HMAC-SHA256 of `"{t}.{raw body}"` with the endpoint's `whsec_` secret against each `v1` value, on the **exact raw body** (a framework that re-parses JSON breaks it). Reject a timestamp outside the tolerance (SEC-016: 5 minutes). While a secret is being rolled, several `v1` values arrive (up to 24 hours). Compare in constant time [inferred; Stripe's libraries do this]. Stripe also asks you to accept webhooks only from its published IP addresses [P8].

**Rust:** Stripe maintains SDKs for Ruby, PHP, Java, Python, Node, .NET and Go only; others are community libraries [P10]. BUILD_BRIEF.md:110 already chooses "Stripe REST API through reqwest behind an adapter trait; ... webhooks verified with HMAC-SHA256". A community crate would need a `docs/decisions.md` entry and cargo-deny (BUILD_BRIEF.md:122).

**Handling** [P8][P9]:
- Return 2xx quickly, then process from a queue; a 3xx counts as a failure.
- The same event can arrive more than once, and sometimes two separate Event objects are made for one change: dedupe by event ID (SEC-017) and also by `data.object.id` plus `type`.
- Order is not guaranteed; act on the object's current state.
- Live-mode retries last up to three days; the List Events API returns only the last 30 days (FEE-029's nightly reconcile is inside that).
- Snapshot payloads can be stale: "fetch the latest resource from the API before taking any critical action" [P9]. Stripe now recommends thin events for new integrations [P9].
- Use a restricted live key with only the permissions the handler needs [P9].

**Events to act on** (names from Stripe's event types reference, https://docs.stripe.com/api/events/types, not fetched in this session except where tagged):

| Event | Ask Hava action |
|---|---|
| `payment_intent.succeeded` [P3] | Re-fetch the PaymentIntent; check livemode, account, amount, pick metadata (SEC-039); build `ReleaseProof`; release once (PICK-022); ledger charge |
| `payment_intent.payment_failed` [P3] | Show on the Confirm page and notify the business (FEE-020); never retry on its own (FEE-016) |
| `charge.refunded`, `refund.failed` [P13] | Ledger refund; on failure retry hourly with an alert (FEE-026) |
| `charge.dispute.created` | Alert Casey, ledger dispute, block refunds on that charge (FEE-026, FEE-027) |
| `charge.dispute.closed`, `charge.dispute.funds_withdrawn`, `charge.dispute.funds_reinstated` | Ledger outcome and money movement |
| `radar.early_fraud_warning.created` [P15] | Alert; see proposal 6 |
| `setup_intent.succeeded`, `payment_method.detached` | Record or remove the saved card and its authorization version (FEE-015, BIZ-051) |
| `payout.paid`, `payout.failed` | Reconcile (section 7) |

## 7. Reconciliation

- Every charge, refund, dispute and fee creates a balance transaction with gross, fee and net [P11].
- The payout reconciliation report matches each automatic payout to the balance transactions it settles, grouped by reporting category (charges, refunds, fees, disputes); it needs automatic payouts on. With manual payouts use the Balance report [P11].
- Put the pick ID and job number in PaymentIntent and refund `metadata`; the itemized report can include metadata columns [P11].
- **Stripe does not return its processing fee when a payment is refunded** [P13]. Every automatic refund (FEE-021 to FEE-025) costs Ask Hava the original fee.
- FEE-030 ("match Stripe to the cent") should compare gross charges, refunds and disputes to Stripe, and the owner money view should also show Stripe fees, which the ledger (FEE-028) has no entry type for (proposal 5).

## 8. Refunds

- Refund through the Refund API on the PaymentIntent or charge, with an idempotency key; it goes to the card that paid [inferred; business terms section 7 says the same].
- A refund can be partial (an `amount`), but every FEE-* refund is a full refund of a fixed fee. Partial refunds are not in the plan [inferred]; do not add them.
- A refund can start `succeeded` or `pending` and later fail, sending `refund.failed` (test card 4000000000005126) [P13]. FEE-026's hourly retry covers it.
- After a dispute opens, don't refund outside the dispute (FEE-026 blocks it). A customer can dispute the full amount even after a partial refund; respond with a "credit issued" response [P16].

## 9. Disputes

- **Fees (US):** $15 per dispute received, plus a $15 countering fee for disputes initiated after June 17, 2025 when you submit evidence, returned if you win [P17]. FEE-027 records only the first $15.
- **One submission:** updating any evidence field submits the whole evidence set; deadline is `evidence_details.due_by`; status values include `warning_needs_response` (an inquiry) and `needs_response` [P18].
- **Limits:** PDF, JPEG or PNG; combined under 4.5 MB and under 50 pages; Stripe can reject with `dispute_evidence_page_limit_exceeded` (handle by `error.code`) [P16].
- **Odds:** even Stripe's best-ranked disputes win about 60% of the time [P16]. Accepting a dispute keeps the fee but isn't an admission [P16].
- **Evidence fields** [P18]: `product_description`, `service_date`, `service_documentation`, `customer_signature`, `customer_communication`, `receipt`, `refund_policy`, `refund_policy_disclosure`, `access_activity_log`, `customer_purchase_ip`, `uncategorized_text`, and others.

**What helps for an introduction fee** (built from FEE-042, never from the vault):
- `product_description`: one paragraph: the fee pays for passing on the request, collecting quotes, the homeowner's choice and the masked call line (TERMS-027); fixed by job size.
- `customer_signature` and `customer_purchase_ip`: the FEE-015 authorization version, the "Confirm and pay" press time, IP and signed-in person.
- `service_date` and `service_documentation`: release time (PICK-022), the business's portal view of the release, and masked-call attempt counts and times with no numbers (CALL logs).
- `refund_policy` and `refund_policy_disclosure`: only the business terms section 7 excerpt, and a screenshot of the refund line on the Confirm page (PICK-021) [P16: "Upload only the relevant ... section"].
- `customer_communication`: the business's own messages about the charge ("Ask about a charge", BIZ-032), relevant excerpts only.
- Visa CE 3.0: a repeat business with earlier undisputed charges may qualify; Stripe flags eligible disputes and adds the prior transactions [P16].

**Never include:** homeowner name, address, phone, email, photos or help-message text (PRIV-027); full card numbers [inferred; PCI]; whole terms of service or links to them [P16]; long email threads [P16].

## 10. Radar, fraud and card testing

- Radar runs on every payment; Radar Plus (paid) adds custom rules and win-likelihood scores [P3][P16].
- Early fraud warnings mean the issuer reported a charge as possibly fraudulent; `radar.early_fraud_warning.created` fires [P15]. [inferred] refunding before a dispute opens avoids the dispute and its fee; proposal 6.
- Collect CVC and postal code to cut fraud-related declines [P19]. The Payment Element collects them by default [inferred].
- Card-testing defence: the Confirm page sits behind business sign-in (SEC-004) and a live pick, so an anonymous attacker can't reach a card form [inferred].

## 11. Declines and retry etiquette

- On-session decline: prompt the person to try again or use another card [P19]. FEE-020 does this.
- Read `outcome.advice_code`: `do_not_try_again` (don't reuse that card for this charge), `try_again_later`, `confirm_card_data` [P19].
- "Card networks limit the number of times you can reattempt a single charge"; Stripe recommends at most eight retries for charges that permit them; extra retries look like fraud [P19]. FEE-016 (no server retries) is well inside this.
- Off-session decline: notify the person to come back [P19]; PICK-048 falls back to the Confirm step.
- Show issuers' reasons only as "declined, try another card"; issuers discuss specifics only with the cardholder [P19].

## 12. Test mode

- Test keys with test cards only; never real card details in live mode; use PaymentMethod IDs, not raw numbers, in code [P20].
- Cards [P20][P3]: generic decline 4000000000000002; insufficient funds 4000000000009995; 3DS always 4000000000003220; off-session needs 3DS unless set up 4000002500003155; saved card that fails on charge `pm_card_chargeCustomerFail`; refund fails 4000000000005126; fraud dispute 4061219946393810; product-not-received dispute 4000000000002685; inquiry 4000000000001976; early fraud warning 4000000000005423; Radar block 4100000000000019.
- Stripe sends no automatic receipt for test payments [P21].
- In sessions use the twin's fake Stripe; `stripe-mock` runs in CI (`docs/for-casey-review.md:269`).

## 13. PCI, restricted businesses, receipts, tax

- **PCI SAQ A.** Stripe's PCI FAQ: "To reduce this to SAQ A, migrate to Stripe Elements, Checkout, or a mobile SDK." [P22]. Payment pages need TLS 1.2 or later; you attest annually from the Dashboard's compliance page [P23]. The January 2025 SAQ A revision removed requirements 6.4.3, 11.6.1 and 12.3.1 and added an eligibility condition that the merchant confirms its site "is not susceptible to attacks from scripts that could affect the merchant's e-commerce system(s)"; PCI SSC FAQ 1588 lists ways to meet it [P24, secondary; PCI SSC text not fetched]. For Ask Hava: the Confirm page loads no third-party script except Stripe.js, with the CSP Stripe lists (`script-src https://js.stripe.com https://*.js.stripe.com`, `frame-src https://js.stripe.com https://*.js.stripe.com https://hooks.stripe.com`, `connect-src https://api.stripe.com`) [P23].
- **Restricted and prohibited (Stripe list, updated 2026-09-22)** [P12]: no category for lead generation, referrals or home services. Closest prohibited items: "Sales of online traffic or engagement", "Sharing cardholder information with another business for payment of cross-sell of products or services", "Telemarketing", "Door-to-door sales". Restricted: "Payment facilitation and aggregation". Ask Hava sells a service to the business that pays, shares no cardholder data, and collects no job money, so none fits on its face [inferred]. The plan already waits for Stripe's written OK before the first charge (`docs/for-casey-review.md:440`, D-302) with the WORD-015 description; keep it.
- **Receipts.** Turn on automatic receipts for successful payments and refunds in Dashboard settings; receipts must show the legal business name, a customer support address, a support email and a privacy policy URL; `receipt_url` links expire after 30 days [P21]. FEE-031 sends Stripe's receipt and Ask Hava's own.
- **Tax.** Stripe Tax is optional and not in the plan; fees are tax-included (FEE-008). For Arizona TPT, check the plan's D-444 and FEE-046; do not turn on any Stripe tax feature without a spec change [inferred].

## 14. What the plan does right, and proposed requirements

**Right already:** one on-session press per charge, no background retries (FEE-016, FEE-036); card fields only in Stripe Elements (FEE-014); release only on a verified success, exactly once (PICK-022, SEC-030, SEC-039); state check before each attempt (FEE-017); timeout handling by asking Stripe (FEE-019, FEE-037); off-session only behind its own authorization, Stripe's written answer and a Verify-proven pick (PICK-048, PICK-056); evidence record per charge (FEE-042); no homeowner data in dispute packs (PRIV-027); signature and replay checks (SEC-016, SEC-017, SEC-020); separate test and live accounts and endpoints (SEC-003, SEC-036); refunds once, blocked on disputed charges (FEE-026).

**Proposed** (need a spec change logged in `docs/decisions.md`; a task may not add them on its own):

1. **One PaymentIntent per pick** (FEE-016): "Each pick owns at most one PaymentIntent, created with an idempotency key derived from the pick ID and reused for every press and card change on that pick; a press after a decline confirms the same PaymentIntent." [P3, P7]
2. **Save for off-session when saving** (FEE-014, PICK-048): "A card saved on the Confirm page is saved with `setup_future_usage=off_session` (a SetupIntent with `usage=off_session` on the $0 page), since the FEE-015 authorization covers auto-confirm." [P2, P4] (BUILD_BRIEF.md:110 says "SetupIntent with off-session use"; D-365 logged that it isn't used in V1; PICK-048 later added one off-session charge.)
3. **Statement descriptor** (FEE-031): "Card charges use the account prefix ASKHAVA and the suffix 'JOB <n>'; a test checks the full descriptor is 22 characters or fewer." [P6]
4. **Dispute economics** (FEE-027): "The ledger records the $15 countering fee when evidence is submitted and its return on a win; evidence is auto-submitted only when the charge has a release and at least one recorded call attempt or a 3DS-authenticated payment, and otherwise the dispute waits for Casey's decision to counter or accept." [P16, P17]
5. **Stripe fees in the money view** (FEE-028, FEE-030, FEE-046): "The nightly reconcile reads each balance transaction's fee and the owner money view shows Stripe processing and dispute fees as their own line; FEE-030's match compares gross charges, refunds and disputes." [P11, P13]
6. **Early fraud warnings** (FEE-026): "`radar.early_fraud_warning.created` alerts Casey; a warned charge whose details were not yet released is refunded at once, and one already released is listed for Casey's decision." [P15; refund policy inferred]
7. **Re-fetch before release** (SEC-039): "A `ReleaseProof` built from a webhook first re-fetches the PaymentIntent from Stripe and uses that state; webhook requests come only from Stripe's published IP ranges." [P8, P9]
8. **Receipt details before Gate C**: "Stripe's public business details (legal name, support address, support email, privacy URL) are set before the first live charge, the support address being the LLC's mailing address, never a home address." [P21]

## 15. Review checklist for payments tasks

- [ ] Money moves only through `payments` (FEE-032); the fee is computed on the server, never taken from the browser.
- [ ] No card number, CVC or full card data reaches Ask Hava's servers, logs or tests; only PaymentMethod IDs, last four and brand (FEE-014) [P23].
- [ ] Every Stripe POST has an idempotency key with no personal data; a repeated press reuses it (FEE-016) [P7].
- [ ] State checked before each attempt (FEE-017); no server-side retry of a charge (FEE-016).
- [ ] Release only from a verified, re-fetched success matching livemode, account, amount and pick (PICK-022, SEC-030, SEC-039).
- [ ] Webhook: raw body, signature, 5-minute tolerance, dedupe by event ID and by object plus type, 2xx before slow work (SEC-016, SEC-017) [P8].
- [ ] Off-session code paths exist only for PICK-048, gated by PICK-056 and the filed Stripe answer.
- [ ] Refunds full, once, never on a disputed charge (FEE-026).
- [ ] Ledger append-only and matches Stripe (FEE-028, FEE-030).
- [ ] Dispute evidence carries IDs and timestamps only, no homeowner fields (PRIV-027).
- [ ] Wording: "introduction fee", never "commission", "lead" or "data access" (TERMS-027).
- [ ] Tests use Stripe test cards or the fake Stripe, never live keys before Gate C (BUILD_BRIEF.md:68); branch coverage on money code at or above 95% (BUILD_BRIEF.md:481).

## 16. Sources and why they are authoritative

All are Stripe's own documentation (it is the payment processor; its docs define what its API does and what it permits), except P24.

- [P1] Authenticate with 3D Secure: https://docs.stripe.com/payments/3d-secure/authentication-flow
- [P2] SetupIntents (usage, mandates, allow_redisplay): https://edge-docs.stripe.com/payments/setup-intents ; save-and-reuse index: https://docs.stripe.com/docs/payments/save-and-reuse (the save-during-payment page, https://docs.stripe.com/payments/save-during-payment, could not be fetched: permission timed out)
- [P3] Payment status updates and fulfillment by webhook: https://docs.stripe.com/payments/payment-intents/verifying-status.md ; 3DS test cards and liability shift: https://docs.stripe.com/payments/3d-secure/authentication-flow
- [P4] SCA exemptions (applicability EEA, Switzerland, UK; merchant-initiated transactions): https://docs.stripe.com/payments/3d-secure/strong-customer-authentication-exemptions
- [P5] Off-session declines and `authentication_required`: https://docs.stripe.com/declines/card
- [P6] Statement descriptors: https://docs.stripe.com/get-started/account/statement-descriptors.md
- [P7] Idempotent requests: https://docs.stripe.com/api/idempotent_requests.md
- [P8] Manage webhook endpoints (signatures, duplicates, IPs, retries, secret rolling): https://docs.stripe.com/webhooks/signature.md
- [P9] Set up events (thin vs snapshot, restricted keys): https://docs.stripe.com/events/set-up-events
- [P10] Server-side SDKs (official languages): https://docs.stripe.com/sdks/server-side
- [P11] Payout reconciliation report: https://docs.stripe.com/reports/payout-reconciliation.md
- [P12] Prohibited and restricted businesses (updated 2026-09-22): https://stripe.com/us/restricted-businesses/
- [P13] Fees on refunded payments: https://support.stripe.com/questions/understanding-fees-for-refunded-payments ; refund-failure test card: https://docs.stripe.com/testing
- [P14] Connect is for multi-party payments (receipts page, Connect section): https://docs.stripe.com/payments/advanced/receipts
- [P15] Early fraud warnings: https://docs.stripe.com/api/radar/early_fraud_warnings.md
- [P16] Dispute evidence best practices: https://docs.stripe.com/disputes/best-practices
- [P17] June 2025 dispute pricing update: https://support.stripe.com/questions/june-2025-pricing-updates-for-disputes
- [P18] The Dispute object: https://docs.stripe.com/api/disputes/object.md
- [P19] Card declines and retries: https://docs.stripe.com/declines/card
- [P20] Testing: https://docs.stripe.com/testing
- [P21] Receipts: https://docs.stripe.com/payments/advanced/receipts
- [P22] PCI compliance FAQ: https://support.stripe.com/questions/pci-compliance-faq
- [P23] Integration security guide (TLS, CSP): https://docs.stripe.com/security/guide
- [P24] SAQ A January 2025 revision and PCI SSC FAQ 1588 (secondary summary; the PCI SSC document library, https://www.pcisecuritystandards.org/document_library/, was not read): https://blog.report-uri.com/pci-dss-4-0-1-saq-a-and-client-side-security-a-plain-english-faq/

Not legal or tax advice; no lawyer or CPA reviewed this file (D-367).
