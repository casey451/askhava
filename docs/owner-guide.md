# Owner guide

For Casey. How Laftly works, what to check each day and each week, how to make alerts reach your phone, and where each owner tool lives. Every path below is a row in `docs/architecture.md` section 13 (T123, D-314, D-3891, D-5568).

## 1. How everything works

Laftly is not a contractor and does not perform, bid on or supervise work. It passes a homeowner's request to local businesses and lets her pick one quote.

1. **A homeowner asks.** She sends her job by text or on the website. Before any text is sent, every path shows her the full consent text first.
2. **A job card is made.** Her words are scrubbed of names, phones and addresses, and only the scrubbed text goes to the AI, which fills in the card's fields. Her contact details stay locked in the vault.
3. **Businesses quote.** Businesses in that trade and town get the card and send quotes. Laftly never ranks or rates them.
4. **She picks.** She sees the quotes on her quote page and picks one, or closes the request.
5. **The business pays and gets the job.** The business pays an introduction fee by job size ($25, $75 or $199). Only once the payment succeeds (or a $0 fee applies, such as a first win) are her first name, job address and photos released to that business, with a masked call line. Her phone number is never given out.
6. **Follow-up.** A check-in a few days later asks how it went, and her feedback is kept.

Texting rules the system enforces for you: texts go only between 8 a.m. and 8 p.m. in the stricter of Arizona time and the time zone of the phone's area code. A text that is only an opt-out word, in English or Spanish (including "cancel"), stops all texts to that phone. "Cancel" alone never closes a job. Every text kind has a Spanish version.

Behind the scenes, a health check runs every 5 minutes, a nightly check looks for any difference in the money ledger, and the database is backed up. When something needs you, an alert is raised (section 4).

**Safe mode.** If a High alert that was sent to your phone stays unacknowledged for 24 hours, safe mode pauses charges and releases, and nothing else. Texts keep going; safety texts, STOP, HELP and START replies and pick codes are never paused.

## 2. Daily checks

1. **Sign in** at `/owner/sign-in` (password, then a 6-digit code from your authenticator app, a recovery code or your passkey).
2. **Owner home** (`/owner/home`): read "Alerts sent to your phone" and press "I've seen this alert" on each one. A delivery alert has no button; it clears by itself once texts get through again. Then glance at "Alerts not sent to your phone" and "Sign-ups waiting for you" (each sign-up should get your decision within 4 days).
3. **Help requests** (`/owner/help`): answer anything due. Each request shows its reply-by time.
4. **Problem reports** (`/owner/problem-reports`): sort any new report into a kind.
5. **The 9 a.m. and 3 p.m. pushes**: when something is open (urgent call-backs, help requests due within 24 hours, safety words in a complaint, a restored number, a failing license list), one email reaches your phone at each time. Clear what it lists.

## 3. Weekly checks

1. **Weekly digest** (`/owner/digest`, also sent by email): counts and job or business numbers for the week, the trades and businesses flagged for your monthly review, help requests due in the next 4 days and the week's PROBLEM replies. Open it at least every 4 days or it reminds you again.
2. **Opt-outs each week** on the owner home: you are alerted at 3% of texted homeowners or 5% of texted businesses.
3. **System health** (`/owner/health`): delivery rates, sending limits, timings, payment and AI errors, uptime, last backup, and whether each secret is present (never its value).
4. **Pilot scorecard** (`/owner/scorecard`): read it and enter your minutes for the week.
5. **Money** (`/owner/money`): check what was collected, set aside for sales tax, spent and left; add any expense.

Monthly: run the spot-check of 10 random job cards on the health page, read the intake report (`/owner/intake-report`), download the money CSV and decide on any trade or business the digest flags for review.

## 4. Your alert address and Do Not Disturb

Alerts never come to you as a text. Every alert that pushes to your phone is an **email to your alert address**. A critical safety alert also places **one automated call** from the Laftly number, naming the alert kind and job number with no homeowner detail. HELP notifications are email only, never a call. Every other alert waits on the owner home and in the weekly digest.

The alert address is the `ASKHAVA_ALERT_EMAIL` setting, stored as a secret on the host, never in code or chat. It goes live only at Gate C, with your go.

Set your phone up once:

1. **Make the alert email notify you.** In the Gmail app, keep notifications on for the Primary tab and move the first alert email to Primary if it lands elsewhere. In iPhone Mail, add the alert sender as a VIP and turn on VIP alerts.
2. **Let the Laftly number through Do Not Disturb.** Save the Laftly number as a contact. On an iPhone, go to Settings, Focus, Do Not Disturb (and any other Focus you use, such as Sleep), People, and allow calls from that contact. On Android, add it as a starred contact and allow calls from starred contacts in Do Not Disturb.
3. **Test it.** With Do Not Disturb on, have a test email sent from the alert sender and a test call placed from the Laftly number, and check that both come through.

Only critical safety alerts are meant to break through Do Not Disturb; the emails follow your phone's normal mail settings.

## 5. Where each owner tool lives

The owner home links to four pages: Category settings (`/owner/settings`), Problem reports (`/owner/problem-reports`), AI switch (`/owner/ai`) and Weekly digest (`/owner/digest`). Every other page is reached by typing its path. A change marked "code" needs an authenticator code from the last 5 minutes; get a fresh one at `/owner/step-up`.

| Path | What it is |
|---|---|
| `/owner/sign-in`, `/owner/sign-in/passkey` | Sign in with password and code, or with a passkey. |
| `/owner/step-up` | Enter a fresh code; also "Sign out" (`/owner/sign-out`) and "Sign out everywhere" (`/owner/sign-out-everywhere`). |
| `/owner/home` | Owner home: the alerts and the sections below. |
| `/owner/alert-ack` | "I've seen this alert" on the owner home. |
| `/owner/call-homeowner` | "Call from the Laftly number" on a critical alert about a text: we ring you, read you the safety text, then connect you; she sees our number, never yours (code). |
| `/owner/pause` | Pause or resume everything, a town, a trade, charging or texting. |
| `/owner/fee-mode` | Switch the introduction fee between tiers and a flat fee per job type (code). |
| `/owner/texting` | Homeowner texting on or off, the launch channel, and recording a number suspension or its end. |
| `/owner/banner` | Hide the status banner. |
| `/owner/settings` | Category settings: one switch per category (code). |
| `/owner/category` | Each category switch on the settings page. |
| `/owner/opt-out` | Record an opt-out you were told about, on the settings page. |
| `/owner/stripe-answer` | Save Stripe's written answer, on the settings page. |
| `/owner/safety-call` | The safety voice call switch, off at launch; on needs the attorney approval. |
| `/owner/spanish-conversations` | The Spanish conversations switch, off at launch; on needs the professional review. |
| `/owner/business-texting` | The business texting switch, off by default; on needs the provider's yes. |
| `/owner/ai`, `/owner/ai/terms`, `/owner/ai/policy`, `/owner/ai/switch` | The AI switch page: what it still needs, the provider terms and policy version forms, and the switch. |
| `/owner/problem-reports`, `/owner/problem-complaint` | PROBLEM reports, newest first, and the form that sorts one into a kind. |
| `/owner/help`, `/owner/help/reply`, `/owner/help/close` | Open help requests with reply-by times; reply (by text, to homeowners texting allows) and close or reopen. |
| `/owner/digest` | The weekly digest. |
| `/owner/release` | Re-send a release that already went out, by job number. |
| `/owner/refunds/{pick}`, `/owner/refunds/{pick}/refund`, `/owner/refunds/{pick}/evidence` | Refund review for one pick: refund, or send your own evidence on a dispute (code). |
| `/owner/audit-log` | Every action and who took it, oldest first. |
| `/owner/health` | System health and the monthly spot-check. |
| `/owner/scorecard`, `/owner/scorecard/minutes` | Pilot scorecard and your minutes this week. |
| `/owner/intake-report` | Monthly intake report: cards with no quote and quotes that needed more information. |
| `/owner/money`, `/owner/money/csv`, `/owner/money/expense` | Money view, the monthly CSV and the expense form. |
