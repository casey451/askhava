# Account checklist

Casey's hands only: nothing here is signed up for, bought, sent or registered without his
go (`BUILD_BRIEF.md:61`), and Claude never creates an account or accepts terms for him.
Brand: Laftly, Laftly LLC, laftly.com (D-3347). Each item says where its key goes: a
**network secret** (with the host it is locked to) or a **test-project variable** on the
app host (`BUILD_BRIEF.md:184`). No key, password or recovery code is ever written in this
file, a chat, a commit or a plain env var. Every secret below is in the key inventory.

## Part 1: test accounts

Needed for Phase 1 and Phase 2. Test keys only; no live key, live number or live Railway
project before Gate C (`BUILD_BRIEF.md:68`).

### Railway test project

- Create the test project; turn on GitHub autodeploy from `main` with "Wait for CI".
- Add PostgreSQL 16. `ASKHAVA_DATABASE_URL` and `ASKHAVA_MIGRATION_DATABASE_URL` are
  test-project variables (sealed).
- For each Railway project (test now, live at Gate C): add a throwaway PostgreSQL 16 service
  for the monthly test restore (T648); its admin URL is `ASKHAVA_RESTORE_ADMIN_URL`, a sealed
  variable on `worker` only.
- For each Railway project: run the bootstrap with `ASKHAVA_BACKUP_PASSWORD` set, which gives
  the `askhava_backup` role its login (T109); `ASKHAVA_BACKUP_DATABASE_URL` is a sealed
  variable on `worker` only.

### GitHub

- The `protect-main` ruleset with no bypass actors. Hardware-key second factor.

### Twilio (test number)

- A Twilio sub-account for the test project with a restricted API key. The Account SID,
  auth token and Verify service SID are test-project variables.
- Buy one test number. Set its incoming-message webhook to `/webhooks/twilio/sms` with a
  connection-override fragment asking for the most retries Twilio allows on 5xx responses,
  connection failures and read timeouts, in the syntax of Twilio's "Webhooks connection
  overrides" page (https://twilio.com/docs/usage/webhooks/webhooks-connection-overrides), or
  set the same through the Webhooks configuration API that page recommends. Twilio retries
  by default only on a connection failure, once, never on a 500 or a read timeout, so the
  inbound routes' "roll back so Twilio sends it again" (D-1307, D-1751) holds only with
  this override (D-2763).
- Set the Fallback URL to the same route. The fragment is not signed, so nothing in the app
  changes. No console status-callback URL is needed: each text asks for its own (T377).
- The same for the voice webhook: `/webhooks/twilio/voice` with the same override and the
  same route as its Fallback URL.

### SendGrid for Twilio Verify email (test)

Twilio Verify's email channel needs a Twilio SendGrid email integration on the Verify
service (https://twilio.com/docs/verify/email) (D-2769). Set before T131 checks the email
channel:

- a SendGrid account;
- domain authentication for the sending domain;
- an API key kept as a secret (entered only in the Verify service, never in the app);
- the code email template;
- the Verify service's email integration.

### Stripe (test mode)

- Test-mode restricted key: `ASKHAVA_STRIPE_SECRET_KEY` and `ASKHAVA_STRIPE_PUBLISHABLE_KEY`
  are test-project variables.
- Webhook endpoint `/webhooks/stripe`; signing secret `ASKHAVA_STRIPE_WEBHOOK_SECRET`. Enabled
  events: `payment_intent.succeeded`, `charge.updated`, `charge.refund.updated`,
  `charge.dispute.created`, `charge.dispute.updated`, `charge.dispute.funds_withdrawn` and
  `charge.dispute.closed` (T381, D-2771).

### Resend, Sentry, storage, AI provider (test)

- Resend: API key `ASKHAVA_RESEND_API_KEY` and its webhook signing secret, test-project
  variables.
- Sentry: `ASKHAVA_SENTRY_DSN`, test-project variable; PII off.
- Storage: a write-only key for one bucket, a network secret locked to the host.
- AI provider: `ASKHAVA_AI_API_KEY`, test-project variable; fake data only (AI-008).

## Part 2: business accounts

Needed only for Gate C (`BUILD_BRIEF.md:182`): LLC with a Maricopa County registered agent,
EIN, Google Workspace, insurance, Twilio brand and campaign registration and Stripe
activation. A zero-retention agreement is not needed for Gate C (D-412, D-414).

### Before any live key (OPS-042)

Every item is done and ticked before any live key is entered:

- [ ] Hardware-key second factor on the host, GitHub, Cloudflare, the registrar, Stripe,
  Twilio, Resend, the AI provider, Sentry and the storage account.
- [ ] Registrar lock on laftly.com.
- [ ] A dedicated recovery email account used for nothing else.
- [ ] Recovery codes and the key escrow on paper, apart from the password manager.
- [ ] Twilio sub-accounts with restricted keys, geo permissions, Verify Fraud Guard and a
  spend cap.
- [ ] Stripe restricted keys.
- [ ] A usage email alert on the Railway workspace at twice the projected monthly bill and
  no hard usage limit, since a hard limit takes every workload offline.
- [ ] A rehearsed break-glass procedure (below).

### Break-glass

Rehearsed once before Gate C, date written here:

- Loss of every owner factor: the paper recovery codes and the recovery email account.
- Loss of host access: the host's account recovery, then a restore from the export.
- Loss of `VAULT_KEY`: the paper escrow is the only recovery copy.

### Release record (OPS-043)

- [ ] The live service deploys only a commit SHA named in a release record that Casey signs
  off after an independent Opus security review of the release diff.
- [ ] A protected `release` branch that only Casey moves to that SHA, since the host's
  manual deploy takes the branch's latest commit.
- [ ] The live project's automatic source connection stays off.
- [ ] The `protect-main` ruleset has no bypass actors, including administrators.
- [ ] The review record is linked from the handover pack.

### Stripe activation (OPS-044)

- Activate with Laftly LLC and the EIN; live restricted key and webhook secret are sealed
  live-project variables.
- Public business details before the first live charge: legal name, support address, support
  email and privacy URL; the support address is the LLC's mailing address, never a home
  address.
- Live webhook endpoint `/webhooks/stripe` with the same enabled events as Part 1, including
  `charge.dispute.updated` and `charge.dispute.funds_withdrawn`.

### Twilio (live number) and SendGrid (live)

- Live sub-account with restricted keys; keys are sealed live-project variables.
- The live number's incoming-message webhook is `/webhooks/twilio/sms` with the same
  connection-override fragment (or the Webhooks configuration API), the Fallback URL set to
  the same route, no console status-callback URL, and the same for the voice webhook.
- SendGrid: the account, domain authentication for the sending domain, an API key kept as a
  secret, the code email template and the Verify service's email integration, set before
  Gate C.

### Texting campaign registration (TERMS-014, TERMS-032)

- [ ] Casey sends the written eligibility question (`docs/handoff/Twilio_question.md`) to
  Twilio, and only if its answer is unclear to one alternative provider, and files the
  written answer before any registration.
- [ ] Registered only once the full site is live, including the request page, privacy
  policy and SMS terms, and only after laftly.com and the SMS terms show the Laftly LLC
  mailing address (a P.O. box or registered private mailbox).
- [ ] Check `docs/legal/twilio-campaign.md`'s description and samples against Twilio's
  December 9, 2026 rejection codes 30977, 30984, 30988, 30965, 30987, 30966 and 30978.
- [ ] Use a public screenshot or evidence link of the call-to-action page as it will be at
  launch (no number is public before Gate C, WEB-017); confirm no placeholder is left
  unfilled.
- [ ] The homeowner campaign is registered even if the business answer is no, except after a
  30951 refusal (CON-033). A 30951 answer or refusal means no registration and web-and-email
  mode, back to Casey (D-5278).
- [ ] Business texting stays off unless the filed answer is yes (TXT-035).

### ADOR ruling request (OPS-048)

The private taxpayer ruling request (A.R.S. 42-2101); no TPT license is taken on assumption.

- [ ] Made in writing in Laftly LLC's name and signed.
- [ ] Describes the introduction fee in the TERMS-027 words: payment for the introduction
  service.
- [ ] Attaches the business terms and a Confirm-page screenshot.
- [ ] States that the question is under no audit, protest or appeal and filed with no other
  taxing authority.
- [ ] The FEE-046 set-aside stays until the request is filed.

### Email, AI and consent records

- [ ] Google Postmaster Tools: laftly.com registered before the first real business email
  (SEC-050).
- [ ] AI provider (AI-008), recorded before the AI switch first goes on: the current
  commercial API terms URL, date checked, that API inputs are not used for training, the
  retention period, the flagged-content retention period (how long the provider may keep text
  its safety systems flag), and a Console spend limit of no more than 3 times the expected
  monthly AI cost.
- [ ] Consent-revocation check (CON-032): the rule's text and effective date re-checked and
  logged in `docs/decisions.md` (last re-checked November 21, 2026, D-5238). Each row's
  test-design entry passing in the latest run on `main` is confirmed from the nightly before
  Gate C.

| CON-032 item | Proved by |
| --- | --- |
| The seven words are conclusive (CON-008) | TD-CON-04 |
| Any reasonable revocation is honored (CON-010, CON-035) | TD-CON-32 |
| An opt-out is processed at once | TD-CON-20 |
| One opt-out covers every text and automated call (CON-013, TXT-024) | TD-CON-05 |
| One confirmation within 5 minutes that merely confirms (CON-009) | TD-CON-05 |
| Every channel feeds one suppression list (CON-035) | TD-CON-20 |
| Texts disclose at least STOP (CON-002) | TD-CON-01 |

## Key inventory (PRIV-024)

Owner Casey for every row; escrow is sealed on paper, apart from the password manager. The
runbook's inventory (`docs/runbook.md`) holds the rotation steps.

| Secret | Storage | Escrow | Rotation trigger |
| --- | --- | --- | --- |
| `VAULT_KEY` | sealed host variable | paper | exposure; yearly |
| `VAULT_INDEX_KEY` | sealed host variable | paper | with `VAULT_KEY` |
| `ID_SECRET` | sealed host variable | paper | exposure |
| the photo-link key | under `VAULT_KEY` | paper | with `VAULT_KEY` |
| the export key | private half off the host | paper | exposure; yearly |
| `ASKHAVA_RESTORE_ADMIN_URL` | sealed on `worker` only | paper | exposure |
| `ASKHAVA_BACKUP_PASSWORD` | bootstrap run only | paper | exposure |
| `ASKHAVA_BACKUP_DATABASE_URL` | sealed on `worker` only | paper | exposure |
| vendor keys: Twilio, Stripe, Resend, SendGrid, AI provider, Sentry, storage | sealed host variable or vendor console | paper | exposure; staff change |

## Providers that receive personal data

For the privacy notice's provider list: Railway (host), Twilio (phone numbers, texts),
SendGrid (email addresses, for Verify email codes), Resend (email addresses), Stripe
(business billing), the AI provider (scrubbed text only) and the storage provider
(encrypted export).
