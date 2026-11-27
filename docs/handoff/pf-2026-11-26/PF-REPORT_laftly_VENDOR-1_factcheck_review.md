# Laftly — VENDOR-1 fact check

**Date:** November 26, 2026  
**Preflight verdict:** HOLD — plan changes required  
**Scope:** T129, T130, T131, T132, T133, T134, T135 and their account provisioning dependencies.

**Answered 10 of 10; 3 vendor families need a public URL; 1 vendor family requires real delivery for full end-to-end acceptance.**

This is a documentation fact check, not an executed integration test. No accounts were created, credentials accessed, messages sent or transactions initiated. Some individual vendor behaviors remain unconfirmed. The following preserves the preceding review's findings and source URLs; no new verification was performed for this Markdown export.

## 1. Twilio

**BLOCKER — T129, T130 and T131**

The current Twilio documentation introduces an additional problem not identified in the supplied review brief: new test credentials cannot be created through the new Twilio console. Previously created legacy credentials continue to work.

| Question | Answer | Vendor documentation | Evidence / notes for our plan |
|---|---|---|---|
| Q1 — SMS creation | Supported with existing test credentials | [Test credentials](https://www.twilio.com/docs/iam/test-credentials) | Simulated Messages creation, parameter validation and magic-number errors. |
| Q1 — StatusCallback delivery | Unsupported | [Test credentials](https://www.twilio.com/docs/iam/test-credentials) | Test operations do not trigger status callbacks. Passing the parameter does not prove delivery. |
| Q1 — Voice creation | Partly supported | [Test credentials](https://www.twilio.com/docs/iam/test-credentials) | Simulated call creation; no actual telephone connection. |
| Q1 — Answering-machine detection | Unsupported as a behavioral test | [Test credentials](https://www.twilio.com/docs/iam/test-credentials) | No TwiML execution; cannot prove detection, bridging or masked-call script execution. |
| Q1 — Verify SMS, voice and email | Unsupported | [Error 20008](https://www.twilio.com/docs/api/errors/20008) | Unsupported resources return Forbidden; T131 cannot pass its intended Verify sandbox tests using these credentials. |
| Q1 — Lookup | Partly supported | [Magic Numbers for Lookup](https://www.twilio.com/docs/lookup/magic-numbers-for-lookup) | Identity Match, SIM Swap, Reassigned Number and SMS Pumping Risk packages. Confirm which package Laftly requires. |
| Q1 — Message body redaction | Unsupported | [Test credentials](https://www.twilio.com/docs/iam/test-credentials) | Test credentials cannot perform the message update operation required by T275. |
| Q1 — Obtaining new test credentials | Unsupported in new console | [Test credentials](https://www.twilio.com/docs/iam/test-credentials) | Account checkpoint must establish whether legacy credentials exist. |
| Q2 — Alternative testing | Partly supported | [Twilio testing guidance](https://help.twilio.com/articles/223183808-Why-aren-t-my-test-credentials-working-) | Separate trial project uses actual service operations, not a simulated carrier. |
| Q2 — Verify costs | Usage-based | [Verify pricing](https://www.twilio.com/en-us/verify/pricing) | Documented US pricing in preceding review: $0.05 per successful verification + $0.0083 per SMS. Requires explicit authorization before real delivery or spending. |
| Q3 — Local webhook testing | Supported through public tunnel | [Webhooks FAQ](https://static0.twilio.com/docs/usage/webhooks/webhooks-faq) | Publicly accessible web URL required. Railway suitable but not technically mandatory. |

### Required plan changes

| Task | Severity | Required correction |
|---|---|---|
| T129 | Blocker | Separate simulated message creation from actual callbacks and message-body redaction. |
| T130 | Blocker | Existing fallback only covers real texts when test credentials are unavailable. It does not authorize Verify, voice or other live operations. |
| T131 | Blocker | Remove assumption that test credentials support Verify. |
| T131 | Major | Separate call creation parameter tests from actual AMD, masked calling and TwiML execution. |
| T122 / account checklist | Blocker | Determine whether legacy credentials are available before scheduling vendor contract runs. |
| T135 | Major | Provide public test endpoint before actual webhook delivery tests. |

The Verify trial workflow still involves verified recipients and real verification messages. Fraud Guard is a fraud-prevention feature, not a substitute for an isolated Verify sandbox.

**Do not silently use regular Twilio credentials.** That would violate the supplied `BUILD_BRIEF.md` rules.

## 2. Stripe

**MAJOR — T132 requires a distinction between stripe-mock and the real Stripe sandbox.**

Stripe has the most comprehensive documented sandbox of the vendors reviewed. Most payment lifecycle behavior can be tested without moving real money.

| Question | Answer | Vendor documentation | Notes for our plan |
|---|---|---|---|
| Q4 — PaymentIntent lifecycle | Supported | [Testing](https://docs.stripe.com/testing) | Use real Stripe sandbox; no funds move. |
| Q4 — Off-session confirmation | Supported | [Save and reuse](https://docs.stripe.com/payments/save-and-reuse) | Test saved methods and successful `off_session=true` collection. |
| Q4 — Authentication required | Supported | [Confirm PaymentIntent](https://docs.stripe.com/api/payment_intents/confirm) | Exercise `requires_action` and on-session recovery. |
| Q4 — Off-session authentication failure | Supported | [Save and reuse](https://docs.stripe.com/payments/save-and-reuse) | Assert `authentication_required` and resulting status. |
| Q4 — Full and partial refunds | Supported | [Create refund](https://docs.stripe.com/api/refunds/create) | Test full, partial, repeated and excessive refunds. |
| Q4 — Disputes | Supported | [Testing disputes](https://docs.stripe.com/testing#disputes) | Test dispute creation and resolution. |
| Q4 — Statement descriptor suffix and description | Supported | [Create PaymentIntent](https://docs.stripe.com/api/payment_intents/create) | Verify stored values and descriptor length; use `statement_descriptor_suffix` for cards. |
| Q4 — Local webhooks | Supported | [Stripe webhooks](https://docs.stripe.com/webhooks) | `stripe listen` forwards locally without public inbound endpoint. |
| Q4 — Server-side 3DS confirmation | Supported | [Create PaymentIntent](https://docs.stripe.com/api/payment_intents/create) | Supports planned `use_stripe_sdk=true` correction. |
| Q5 — stripe-mock behavior | Unsupported for full lifecycle | [Official stripe-mock repository](https://github.com/stripe/stripe-mock) | Stateless; cannot prove lifecycle, persistence or genuine webhook delivery. |

### Required Stripe test cards

Official synthetic test cards, not real payment cards. Source: [Stripe test card documentation](https://docs.stripe.com/testing).

| Test card | Expected scenario |
|---|---|
| `4242 4242 4242 4242` | Ordinary successful payment |
| `4000 0025 0000 3155` | Authentication required unless correctly set up for future off-session payments |
| `4000 0027 6000 3184` | Authentication always required |
| `4000 0038 0000 0446` | Already set up for successful off-session use |
| `4000 0000 0000 3220` | 3DS2 authentication required |
| `4000 0084 0000 0027` | US-issued 3DS-required test card |
| `4000 0082 6000 3178` | Authentication followed by insufficient funds |
| `4000 0000 0000 0259` | Synchronous fraudulent dispute |
| `4061 2199 4639 3810` | Asynchronous fraudulent dispute |
| `4000 0000 0000 2685` | Product-not-received dispute |

For dispute resolution, submit `winning_evidence` or `losing_evidence` through documented evidence fields.

### Required test separation

| Test | stripe-mock | Real Stripe sandbox |
|---|---|---|
| Request parameter names | Yes | Yes |
| Response deserialization | Yes | Yes |
| PaymentIntent state transitions | No | Yes |
| Authorization and capture | No | Yes |
| Off-session recovery | No | Yes |
| 3DS authentication lifecycle | No | Yes |
| Refund accounting | No | Yes |
| Dispute resolution | No | Yes |
| Actual Stripe webhook events | No | Yes |
| Ledger reconciliation | No | Yes |

**T132 should not receive full acceptance based on stripe-mock results.** A canned `latest_charge` field does not prove that the adapter correctly retrieves the charge belonging to a completed payment.

## 3. Resend

**MAJOR — Current documentation resolves the suppression-removal API uncertainty in T133.**

Resend announced managed suppression-list API support on July 28, 2026. Adding, listing and removing suppressed addresses are officially supported.

| Question | Answer | Vendor documentation | Notes |
|---|---|---|---|
| Q6 — Successful delivery | Supported | [Send test emails](https://resend.com/docs/dashboard/emails/send-test-emails) | `delivered [at] resend.dev` |
| Q6 — Hard bounce | Supported | [Send test emails](https://resend.com/docs/dashboard/emails/send-test-emails) | `bounced [at] resend.dev` |
| Q6 — Spam complaint | Supported | [Send test emails](https://resend.com/docs/dashboard/emails/send-test-emails) | `complained [at] resend.dev` |
| Q6 — Suppressed email | Supported | [Send test emails](https://resend.com/docs/dashboard/emails/send-test-emails) | `suppressed [at] resend.dev` |
| Q6 — Delivery delayed | Partly supported | [Test email documentation](https://resend.com/docs/dashboard/emails/send-test-emails) | No deterministic delayed-event test address confirmed. |
| Q6 — Failed email | Partly supported | [Email metrics API](https://resend.com/changelog/email-metrics-api) | Event exists; deterministic simulation unconfirmed. |
| Q6 — Suppression add/list/remove | Supported | [Email Suppressions, July 28, 2026](https://resend.com/changelog/suppression-list-support) | Implement actual API. |
| Q6 — Remove by email address | Supported | [Remove suppression](https://resend.com/docs/api-reference/suppressions/remove-suppression) | Remove by ID or email. |
| Q6 — Local webhook receiver | Partly supported | [Resend CLI](https://resend.com/docs/cli) | CLI forwarding still requires publicly accessible URL. |
| Q6 — Test email quotas | Limited | [Send test emails](https://resend.com/docs/dashboard/emails/send-test-emails) | Test emails count toward sending quota; bound runs. |

### Confirmed test addresses

| Address | Intended event |
|---|---|
| `delivered [at] resend.dev` | `email.delivered` |
| `bounced [at] resend.dev` | `email.bounced` |
| `complained [at] resend.dev` | `email.complained` |
| `suppressed [at] resend.dev` | `email.suppressed` |

Unique plus-address labels are supported on the first three addresses. The suppressed test address does not currently support labels.

### Required T133 changes

The intended endpoint is confirmed: `DELETE /suppressions/:suppression`. The identifier can be an email address or suppression ID.

T133 should no longer treat the absence of a suppression-removal API as the expected vendor limitation. Retain handling for actual authorization failures, unavailable features and unexpected responses.

The suppression list applies across the entire team. Isolate the test team from production so automated tests cannot modify real suppression records. Consider the documented `suppression.added`, `suppression.removed` and `email.suppressed` events for reconciliation and auditing.

## 4. Lob

**BLOCKER — Lob is missing from the supplied account checklist.**

Lob provides a genuine test environment suitable for automated printed-mail adapter testing.

| Question | Answer | Vendor documentation | Notes |
|---|---|---|---|
| Q7 — Test letters and postcards | Supported | [Lob API documentation](https://docs.lob.com/) | No printing, mailing or real address verification. |
| Q7 — Digital proofs | Supported | [Lob API keys](https://help.lob.com/account-management/api-keys) | Verify digital proof, thumbnails and resource fields. |
| Q7 — Test-mode charges | No charges | [Test and live environments](https://docs.lob.com/#section/Test-and-Live-Environments) | No physical mailing costs. |
| Q7 — Webhook notifications | Partly supported | [Using webhooks](https://help.lob.com/print-and-mail/getting-data-and-results/using-webhooks) | Subscribe to supported creation/rendering events. |
| Q7 — Postal tracking events | Unsupported in test mode | [Tracking your mail](https://help.lob.com/print-and-mail/getting-data-and-results/tracking-your-mail) | No genuine USPS events from test mail. |
| Q7 — Address verification | Simulated only | [Lob API documentation](https://docs.lob.com/#tag/US-Verifications) | Special fixture inputs simulate outcomes. |
| Q7 — Webhook receiver | Public HTTPS required | [Using webhooks](https://help.lob.com/print-and-mail/getting-data-and-results/using-webhooks) | Railway or approved tunnel. |

### Critical webhook detail

Lob's webhook debugger uses a different signing key from the normal subscribed webhook secret. A successful debugger event is not sufficient evidence that Laftly validates actual vendor-generated webhook signatures.

T133 should test the distinction and confirm the `Lob-Signature-Timestamp` unit from a captured test-mode webhook before changing timestamp parsing. The exact unit was not conclusively established from documentation inspected.

### Required account checklist additions

| Configuration item | Required |
|---|---|
| Lob account | Yes |
| Secret Test API Key | Yes |
| Separate test webhook subscription | Yes |
| Webhook signing secret | Yes |
| Public HTTPS webhook endpoint | Yes |
| Live API key | No — prohibited before Gate C |
| Real printed letter | No |

## 5. Backblaze B2

**MAJOR — Write-only access and immutable storage must be tested separately.**

B2 provides bucket-scoped and prefix-scoped application keys with granular capabilities.

| Question | Answer | Vendor documentation | Notes |
|---|---|---|---|
| Q8 — Bucket-scoped key | Supported | [Create and manage app keys](https://www.backblaze.com/docs/cloud-storage-create-and-manage-app-keys) | Isolate test bucket. |
| Q8 — Read-only/write-only | Supported | [Create and manage app keys](https://www.backblaze.com/docs/cloud-storage-create-and-manage-app-keys) | Separate credentials. |
| Q8 — Prefix restriction | Supported | [Application keys](https://www.backblaze.com/docs/cloud-storage-application-keys) | `namePrefix` for audit, export and photos. |
| Q8 — No permanent deletion | Supported through capability restrictions | [Application key capabilities](https://www.backblaze.com/docs/cloud-storage-application-key-capabilities) | Do not grant `deleteFiles` to audit/export writers. |
| Q8 — Write-only immutability | Partly supported | [Application key capabilities](https://www.backblaze.com/docs/cloud-storage-application-key-capabilities) | Write access can include `b2_hide_file`; verify Object Lock separately. |
| Q8 — Dedicated sandbox | Not confirmed | [Developer quickstart](https://www.backblaze.com/docs/cloud-storage-developer-quick-start-guide) | Dedicated test bucket with fake data is recommended isolation, not confirmed sandbox product. |

### Recommended T134 credential separation

| Key purpose | Prefix | Upload | Read | List | Delete |
|---|---|---|---|---|---|
| Audit writer | `audit/` | Yes | No | No | No |
| Export writer | `exports/` | Yes | No | No | No |
| Photo service | `photos/` | Yes* | Yes | Yes | Yes |
| Restore reader | `exports/` | No | Yes | As required | No |

*Grant photo upload only if required. Verify capability names against the selected B2 Native or S3-compatible API.*

### Required negative tests

- Audit key cannot read or permanently delete audit objects.
- Export key cannot access photo objects.
- Photo key cannot access audit or export prefixes.
- Object Lock prevents unauthorized deletion of protected versions.
- A prohibited operation is not incorrectly interpreted as a missing object.
- Versioning, retention and restore behavior remain effective.

**A successful upload is not sufficient to prove compliance with PRIV-029.**

## 6. Sentry

**Supported — No architectural blocker identified.**

| Question | Answer | Vendor documentation | Notes |
|---|---|---|---|
| Q9 — Free account | Supported | [Pricing and billing](https://docs.sentry.io/pricing/) | Free Developer plan suitable for bounded testing. |
| Q9 — Synthetic test events | Supported | [SDK quickstart](https://docs.sentry.io/platforms/python/) | Trigger synthetic exception and confirm receipt. |
| Q9 — Server-side scrubbing | Supported | [Server-side data scrubbing](https://docs.sentry.io/security-legal-pii/scrubbing/server-side-scrubbing/) | Configure and verify project-specific rules. |
| Q9 — Project-level scrubbing rules | Supported | [Update a project](https://docs.sentry.io/api/projects/update-a-project/) | Verify sensitive synthetic fields are removed before storage. |

Server-side scrubbing is defense in depth, not authorization to transmit customer PII. Scrub sensitive content locally before sending events and configure Sentry's scrubbing independently.

## 7. Cross-vendor execution summary

Q10 — Where tests can actually run.

| Vendor | Keys needed | Test-mode limits | Runs from CI (outbound only)? | Needs public URL? | Needs real device/person? |
|---|---|---|---|---|---|
| Twilio Messaging | Test Account SID, Test Auth Token | Simulated creation; no callbacks or persistent updates | Yes, with existing test credentials | Actual callbacks | Real SMS delivery |
| Twilio Voice | Test Account SID, Test Auth Token | Simulated creation; no actual execution | Partially | Actual voice webhooks | Complete telephone testing |
| Twilio Verify | Account SID, Auth Token, Verify Service SID | No compatible test-credential sandbox | API requests only, authorized regular credentials | Not for basic API requests | Delivery and code receipt |
| Twilio Lookup | Test Account SID, Test Auth Token | Supported magic-number packages | Yes | No | No |
| Stripe | Sandbox secret key, publishable key, webhook signing secret | Simulated financial transactions | Yes, including CLI forwarding | No for CLI testing | No |
| Resend | API Key, webhook signing secret | Test addresses and quotas | API-only tests | Actual webhook delivery | No for documented test addresses |
| Lob | Secret Test API Key, webhook signing secret | No physical mail or postal tracking | API-only tests | Actual webhook delivery | No for test-mode mail |
| Backblaze B2 | Application Key ID, Application Key | Dedicated test resources; real storage operations | Yes | No | No |
| Sentry | Project DSN; Auth Token for management API | Quotas; real event ingestion | Yes | No | No |

The technical ability to run a test from GitHub Actions does not mean Laftly's build rules allow it. The supplied `BUILD_BRIEF_EXCERPT.md`, lines 514–519, restricts GitHub Actions CI to stand-ins and no vendor credentials. Vendor-facing tests must use approved build VM network secrets or Railway test-project variables unless that rule is formally changed.

### Recommended execution order

1. **Account checkpoint:** confirm credential availability and add Lob.
2. **Build VM outbound contract tests:** Stripe, B2, Sentry, Resend, Lob and available Twilio simulations.
3. **T135 Railway test deployment:** public HTTPS endpoints and isolated test-project variables.
4. **Vendor webhook integration tests:** Resend, Lob and Stripe deployed-endpoint verification.
5. **Owner authorization gate:** any real Twilio SMS, voice or Verify delivery.
6. **Phase 2 acceptance:** evidence-backed results with unresolved tests explicitly recorded.

This avoids the existing dependency problem where vendor tests require credentials or endpoints unavailable until T135.

### Proposed acceptance classifications

| Result | Meaning |
|---|---|
| `PASS_VENDOR` | Executed against actual vendor test service and passed. |
| `PASS_STAND_IN` | Executed against Laftly's stand-in only. |
| `NOT_RUN_CREDENTIALS` | Required authorized credentials unavailable. |
| `NOT_RUN_ENDPOINT` | Required public endpoint unavailable. |
| `NOT_RUN_VENDOR_LIMITATION` | Vendor test environment cannot simulate behavior. |
| `OWNER_AUTH_REQUIRED` | Actual delivery, expenditure or explicit permission required. |
| `FAIL_VENDOR` | Executed against vendor and failed. |

These are proposed classifications, not existing repository statuses. Review against the runner's current acceptance schema before adoption.

## 8. Couldn't confirm

1. Whether Laftly's Twilio account already has functioning legacy test credentials.
2. Whether every T129 `StatusCallback` parameter combination is accepted by the simulated Messages endpoint.
3. Whether T131's exact combination of AMD parameters, callback events and call script is accepted by the simulated Calls endpoint.
4. A genuine no-delivery Verify sandbox or Fraud Guard test mode supporting required SMS, voice and email lifecycle.
5. A deterministic Resend address generating `email.delivery_delayed`.
6. A deterministic Resend address generating `email.failed`.
7. Whether labeled Resend hard-bounce test addresses persist in managed suppression list exactly like real hard bounces.
8. Whether Stripe always returns the exact `next_action.type` expected by T132 for Laftly's API version and confirmation request.
9. Exhaustive Lob test-mode webhook event matrix.
10. Exact unit of `Lob-Signature-Timestamp` for actual test-mode deliveries.
11. A dedicated B2 sandbox guaranteeing no storage-related costs.
12. Precise minimum B2 capabilities needed by Laftly's selected client.
13. Sentry account-specific entitlement to every advanced scrubbing feature.
14. Actual adapter behavior, since supplied archive contains planning excerpts rather than complete implementations.

## Final disposition

**HOLD**

| Metric | Result |
|---|---|
| Vendor families reviewed | 6 |
| Questions addressed | 10/10 |
| Integration tests executed | 0 |
| Public webhook dependencies | 3 |

| Vendor | Disposition |
|---|---|
| Twilio | Blocker — acceptance criteria assume unsupported test-credential capabilities. |
| Stripe | Major — distinguish mock compatibility tests from genuine sandbox behavior. |
| Resend | Suppression-removal API officially supported; update plan. |
| Lob | Blocker — missing account provisioning dependency. |
| Backblaze B2 | Major — verify immutability independently of write-only permissions. |
| Sentry | No documented architectural blocker. |

**Recommendation:** Correct Phase 2 task definitions, account checklist and execution dependencies before releasing affected tasks to autonomous build agents. Otherwise agents risk spending substantial time trying to satisfy impossible acceptance criteria, repeatedly retrying unsupported operations or weakening tests to obtain a pass.

No documented vendor limitation should be resolved by silently switching to live credentials or reporting a stand-in result as a real vendor pass.
