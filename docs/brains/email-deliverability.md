# Brain: email deliverability (Gmail, Yahoo, Outlook, Resend)

- **Purpose:** what makes Ask Hava's email reach a business owner's inbox and phone, what Gmail, Yahoo and Outlook require, how Resend's webhooks, suppressions and inbound mail work, and where the current plan meets or misses that.
- **Who reads it:** any session building or reviewing the email adapter in `messaging`, `privacy::api::read_inbound_email()`, business notices (CARD-020 to CARD-022, PICK-026, PICK-037, FEE-031), homeowner emails in web-and-email mode (TXT-034), DNS records, or any task whose prompt names SEC-025, SEC-028, BIZ-040 to BIZ-045 or PRIV-026.
- **How to use it:** read section 1 for the rules that bind, sections 3 to 6 for the code you are writing, then run the checklist in section 10. Cite the tag (`[M1]`, list in section 11) when a choice rests on it.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = reasoning, not backed by a source read for this file.

## 1. What the mailbox providers require

**Gmail** (Google's Email sender guidelines) [M1][M2]:
- **All senders** (from 1 February 2024): SPF or DKIM; valid forward and reverse DNS; TLS; RFC 5322 formatting; "Keep spam rates reported in Postmaster Tools below 0.30%."
- **Bulk senders** (about 5,000 or more messages a day to personal Gmail accounts): SPF **and** DKIM, a DMARC record (`p=none` is enough), and the From domain aligned with the SPF or DKIM domain. "Marketing messages and subscribed messages must support one-click unsubscribe" plus a visible unsubscribe link in the body.
- "Transactional messages are excluded from this requirement" (one-click unsubscribe) [M2]. Google's subscription guidelines say messages sent "for legal or other reasons, such as an explicit action or request by the user, aren't subscription messages" (password resets, receipts, one-time codes), but also that "newsletters and some notifications" count as subscription messages; unsubscribes must be honored "within 48 hours" [M3].
- All mail from one primary domain counts toward the 5,000, subdomains included; bulk status "doesn't have an expiry date" [M2].
- Recommended: spam rate under 0.10%; "Start with a low sending volume to engaged users, and slowly increase the volume over time"; "Messages of the same category should have the same From: email address"; "Don't mix different types of content in the same message" [M1].

**Yahoo** [Y1]: all senders need SPF or DKIM, a spam rate under 0.3%, valid PTR and RFC 5321/5322 compliance. Bulk senders add SPF and DKIM, DMARC at least `p=none` with relaxed alignment, a working one-click List-Unsubscribe (RFC 8058 POST "highly recommended") for marketing and subscribed mail, and "Honor unsubscribes within 2 days." Yahoo advises against sending bulk mail from the IPs used for transactional mail and alerts [Y1].

**Outlook.com** (Hotmail, Live, Outlook): from 5 May 2025, senders of over 5,000 messages a day need SPF, DKIM and DMARC; non-compliant mail goes to Junk, with rejection to follow; Safe Senders doesn't override it [O1].

**What this means for Ask Hava.** On Resend's free tier Ask Hava sends at most 100 emails a day (SEC-025, [R8]), so it is nowhere near bulk-sender status. SEC-025 already meets the bulk rules anyway (SPF, DKIM, aligned DMARC moving to reject). Meeting them early costs nothing and protects the day volume grows [inferred].

## 2. Who sets these rules and why they are credible

- **Google, Yahoo and Microsoft** run the mailboxes; their pages are the rules their filters enforce [M1][Y1][O1].
- **M3AAWG** (Messaging, Malware and Mobile Anti-Abuse Working Group) is the industry body of mailbox providers and senders; its Sender Best Common Practices (27 August 2026 edition) is the shared reference for bounce, complaint and warm-up handling [K1].
- **IETF RFC 8058** defines one-click unsubscribe [I1].
- **The FTC** enforces CAN-SPAM [F1].
- **Resend** is Ask Hava's provider (SEC-025); its docs say what it will do [R1]-[R9]. **Svix** runs Resend's webhook delivery and documents the signature scheme [R2].
- **Al Iverson** (Spam Resource blog) is a long-time deliverability practitioner [L1; credibility note inferred].

## 3. Domain, sender and authentication

- **Dedicated sending subdomain.** Resend recommends a subdomain such as `notifications.acme.com` over the root, for "reputation isolation and sending purpose transparency", and warns against lookalike domains [R9]. M3AAWG likewise suggests distinct subdomains by purpose [K1]. SEC-025 uses `notify.askhava.com` with one fixed sender, "Ask Hava" <jobs@notify.askhava.com>, and a monitored Reply-To on askhava.com. Keep that one From address for every business notice [M1].
- **DMARC progression.** Start at `p=none` and read the aggregate (`rua`) reports; `quarantine` can hide problems because mail "often still arrives"; `reject` makes failures "visible, loud, and immediate"; `pct` "is not widely followed by mailbox providers", so don't rely on it for a gradual rollout [R7]. SEC-025 sets this order and requires `p=reject` before the first real business email.
- The DMARC record at `_dmarc.askhava.com` also governs subdomains unless an `sp=` tag or a subdomain record says otherwise, and a `reject` policy there also hits any other service that sends as `@askhava.com` (a help-desk reply, a Google Workspace mailbox, a form tool) [inferred: RFC 7489 was not read for this file]. List every service that sends as askhava.com in the DMARC reports before moving past `none`.
- No BIMI, no dedicated IP (SEC-025). On a shared IP the provider's pool reputation matters; Resend's subdomain and suppression handling are what Ask Hava controls [inferred].

## 4. Keeping notices transactional

**The law.** CAN-SPAM classifies a message by its "primary purpose" [F1]. A message is transactional or relationship content when it only "facilitates, completes, or confirms a commercial transaction that the recipient already has agreed to", gives safety or security information, "notifies the recipient about a change in the terms or features of a membership, subscription, account", covers employment, or "delivers goods or services as part of a transaction that the recipient already has agreed to"; the FTC "views these categories narrowly" [F1]. A mixed message is commercial if "the subject line would lead the recipient to think it's a commercial message" or the transactional part isn't at the beginning [F1]. Even transactional mail "may not contain false or misleading routing information"; penalties run up to $53,088 per email [F1].

**Why Ask Hava's notices are transactional** [inferred: an application of [F1], not a legal opinion]:
- A business opted in at sign-up, accepted the business terms and ticked the job types it wants (BIZ-004, BIZ-019). A **job card** delivers the service it signed up for: a job it may quote. A **pick, Confirm reminder, release or receipt** (PICK-037, FEE-031) completes or confirms a transaction it started by quoting. **Account notices** (BIZ-031, BIZ-040, BIZ-044) report the state of its account.
- Google's own test points the same way: these follow "an explicit action or request by the user" [M3]. Job cards are the closest call, because they go to everyone in a trade who can pause them, which looks like Google's "some notifications" [M3]. Section 6 proposes handling that with an easy stop.
- Homeowner emails in web-and-email mode (TXT-034) are about her own request; pick codes are one-time codes, which Google lists as non-subscription [M3].

**Wording that keeps them transactional:**
- The subject states the job or account fact: "Ask Hava: New plumbing job in Lake Havasu City. Quote by 4 p.m." (CARD-020). Never a pitch ("More jobs waiting for you!").
- The transactional content comes first: the card, the pick, the receipt. Footer last (SEC-028).
- No promotion inside a notice (SEC-028): no "refer a business", "invite a friend", "new feature", discount, price-lock offer, survey, Facebook-group invite or results post. Recruiting and any newsletter go as separate emails with one-click unsubscribe (SEC-028, BIZ-048).
- Watch PICK-054's "Know someone who needs quotes?" line: keep it off any homeowner email whose subject or opening is about her request, or put it after the request content and keep it to one line [inferred from the FTC's placement test, F1].
- Same From address, same layout, every notice [M1].

## 5. Bounces, complaints and suppression

- **Resend's events** [R3]: `email.delivered` (handed to the recipient's server), `email.delivery_delayed` ("temporary issue", may be retried), `email.bounced` ("permanently rejected"), `email.complained` (marked as spam), `email.failed`, `email.suppressed`, `email.received`. Opens and clicks exist but SEC-025 turns tracking off.
- **Hard bounce:** "A permanent failure indicates that the message should not be retried" [K1]. BIZ-040 marks the address unverified, banners the portal, emails the owner's other verified addresses and lists the business in the digest. CARD-019's count already leaves out a business whose every card notice hard-bounced.
- **Soft bounce:** "re-queue the message and retry later"; don't treat it as permanent [K1]. M3AAWG removes an address that "bounces consecutively at least two times over two weeks or more" [K1]. The spec has no rule for repeated temporary failures (P-EMAIL-2).
- **Complaints from Gmail are invisible.** Resend notes Gmail and Google Workspace "may not send a complaint event" [R4]; Gmail sends no per-recipient complaint reports, only an aggregate spam rate in Postmaster Tools [B1]. Postmaster Tools may show no data on low-volume days [M4]. So BIZ-040's complaint path fires mostly for Yahoo and Microsoft addresses. Ask Hava's real early warning is behavioral: the BIZ-042 inbox check, BIZ-044's missed-cards email, BIZ-045's per-address status and the quote rate per business [inferred].
- **Resend's suppression list** is team-wide: a suppressed address is skipped "across all your domains and subdomains" until removed; removing it without fixing the cause gets it "automatically re-suppressed" [R4]. So when a business fixes its mailbox and re-verifies (BIZ-040), Ask Hava must remove the suppression through the API first, or the verification email itself is silently skipped (P-EMAIL-3).

## 6. Resend webhooks, inbound mail and unsubscribe headers

**Verifying webhooks** (BIZ-040, PRIV-026):
- Verify against the **raw** request body: "The cryptographic signature is sensitive to even the slightest change" [R1]. Headers: `svix-id`, `svix-timestamp`, `svix-signature` [R1].
- The scheme: sign `${svix_id}.${svix_timestamp}.${body}` with HMAC-SHA256, key = the secret without `whsec_`, base64-decoded; the header holds space-separated `v1,<base64>` entries; match any, with a constant-time comparison; reject a timestamp outside your tolerance [R2]. Svix names no number; 5 minutes is a common choice [inferred].
- Resend retries failed deliveries with exponential backoff up to two 10-hour waits, then disables the endpoint and emails the team [R5]. So: dedupe on `svix-id`, return 2xx only after the event is stored, and alert Casey (OPS) if events stop arriving [inferred]. Apply status changes by event time, so a late `delivered` never overwrites an earlier `bounced` [inferred].
- The signing secret is a secret: never in code, logs or plain env vars (CLAUDE.md rule 2; PRIV-024's key inventory).

**Inbound mail** (PRIV-026, SEC-025's Reply-To, BIZ-032):
- Resend receives mail for "any address at that domain" once the domain's MX record points to it, then POSTs an `email.received` webhook; full content and attachments may need a follow-up API call, and attachments come as temporary download URLs [R6].
- Resend's page does not state how long it keeps received mail [R6]. PRIV-026 requires the mailbox to keep nothing beyond 7 days: confirm Resend's retention or delete each message through the API after storing it (P-EMAIL-7).
- If the monitored help address lives at `askhava.com` and reaches the app through Resend, the root domain's MX must point to Resend, and no other mailbox provider can host `@askhava.com` [inferred from R6]. Decide this before DNS is set.
- PRIV-026 records SPF, DKIM and DMARC results for each inbound message and lets a failing sender only opt out. Keep it: inbound From addresses are trivially forged [inferred].

**List-Unsubscribe headers:**
- RFC 8058: `List-Unsubscribe` "MUST contain one HTTPS URI"; `List-Unsubscribe-Post` must be exactly `List-Unsubscribe=One-Click`; both headers must be covered by a valid DKIM signature; the provider's POST carries no cookies or auth, and the sender "MUST NOT return an HTTPS redirect" [I1]. M3AAWG also says to process unsubscribe requests sent to the From and Reply-To addresses [K1].
- Not required for Ask Hava's transactional mail [M2]. Recommended for job-card and open-cards emails anyway, mapped to "stop job-card emails to this address" (never picks, money or account notices): a business that wants fewer emails gets a stop button in Gmail instead of reaching for "Report spam" [inferred] (P-EMAIL-5). That URL must work repeatedly and without a session, unlike SEC-025's single-use action links [inferred from I1].

## 7. Content, links and warm-up

- **Plain-text part and size:** every email has one and stays under 100 KB (SEC-025, A11Y-019). Gmail clips long messages, hiding the footer and links [inferred].
- **Links:** only to askhava.com (SEC-025), never a shortener: shared shortener domains carry other senders' reputation into your message [L1]. Open and click tracking stay off (SEC-025); click tracking rewrites links through a shared tracking domain [L1].
- **Warm-up:** "Start low and slow"; M3AAWG suggests "6 weeks of warm-up as an average" for a new domain, warming a new subdomain "even when the parent domain already has a reputation", spreading sends through the day, and watching deferrals; "Artificial warming practices are not recommended" [K1]. Google says start with engaged recipients [M1]. `notify.askhava.com` is new at launch. Its first mail is naturally gentle: verification codes and sample cards to businesses that just signed up (BIZ-040, BIZ-042) [inferred]. Don't send a backlog of cards to every business on day one; P-EMAIL-8 asks for a watch on deferrals.
- **Capacity:** at 100 a day, 5 jobs to 10 businesses plus open-cards emails and picks can hit the cap. SEC-025's 70% alert and OPS-038 deferral exist for this; buy the paid plan ($20 a month for 50,000, no daily cap [R8]) before the projected month.

## 8. Getting a phone to alert (push versus email)

- **Gmail app:** "By default, notifications are turned on for any email messages in your Primary label"; a "High priority only" level limits alerts to mail Gmail judges important [M5]. Mail sorted into Promotions or Updates is outside the Primary default, so it doesn't alert [inferred from M5]. BIZ-042's "move it to the Primary tab" step is right.
- **iPhone Mail:** alerts can be set per account, or limited to VIP senders with VIP alerts turned on [P2]. BIZ-042's "add to VIP and turn on VIP alerts" step is right.
- **Focus, Do Not Disturb and Sleep** decide whether any alert sounds; BIZ-042's test emergency email checks this on the person's real phone.
- **Web push** (BIZ-041): on iOS and iPadOS 16.4 or later it works only for web apps added to the Home Screen, permission must follow a tap, it uses the standard Push API, Notifications API and service workers through Apple's push service, and it integrates with Focus [P1]. Push is the one channel where Ask Hava, not the mail filter, decides that a notice appears [inferred]. Email stays the fallback (BIZ-041).

## 9. Requirement map and proposed requirements

| Topic | Covered by | Gap |
|---|---|---|
| Provider, subdomain, one sender, Reply-To, SPF/DKIM/DMARC progression, no tracking, plain text, size, signed links | SEC-025 | DMARC on other askhava.com senders; root MX choice (P-EMAIL-7) |
| Footer, no promotions | SEC-028 | no test for promotional phrases (P-EMAIL-6) |
| Hard bounce and complaint | BIZ-040, CARD-019 | soft bounces (P-EMAIL-2); suppression removal (P-EMAIL-3); Gmail complaints invisible (P-EMAIL-1) |
| Inbound | PRIV-026 | provider retention (P-EMAIL-7) |
| Web-and-email mode | TXT-034, PICK-029 | none beyond the above |
| Inbox placement help | BIZ-042, BIZ-043, BIZ-044, BIZ-045 | none |
| Webhook verification | PRIV-026 (inbound) | outbound-event webhook rules (P-EMAIL-4) |
| Unsubscribe | SEC-028 (promotional only) | one-click stop on job cards (P-EMAIL-5) |
| Warm-up | none | P-EMAIL-8 |

**Proposed requirement sentences** (for Casey's decision; not yet in the spec):
- **P-EMAIL-1** askhava.com is registered in Google Postmaster Tools before the first real business email, its spam-rate and compliance dashboards are checked in the monthly review, and every `email.complained` event raises an OPS alert to Casey.
- **P-EMAIL-2** An address whose notices end in a temporary failure on 2 or more sends spread over at least 14 days is handled as a hard bounce under BIZ-040.
- **P-EMAIL-3** Before a verification email goes to an address that was suppressed for a bounce or complaint, the app removes that address from the provider's suppression list and records the removal in the audit log.
- **P-EMAIL-4** Every provider webhook is verified against the raw body with a constant-time comparison and a 5-minute timestamp tolerance, deduplicated by its message ID, acknowledged only after it is stored, applied in event-time order, and an OPS alert fires when no webhook has arrived for 24 hours while emails were sent.
- **P-EMAIL-5** Job-card and open-cards emails carry `List-Unsubscribe` (an HTTPS URI and a mailto) and `List-Unsubscribe-Post: List-Unsubscribe=One-Click`, both DKIM-signed, whose request stops job-card emails to that one address within 48 hours, emails the owner that it happened, and never stops picks, money or account notices.
- **P-EMAIL-6** A template test fails any business or homeowner notice that contains a recruiting, referral, discount, survey, new-feature or social-media invitation phrase from a list kept in `docs/ai_smell.yaml` or its own file.
- **P-EMAIL-7** Before DNS is set, a decision records where inbound mail is received (root domain or a subdomain) and which services send as askhava.com, and the inbound handler deletes each received message from the provider after storing it, or the runbook records the provider's written retention period, which must be 7 days or less.
- **P-EMAIL-8** For the first 6 weeks after notify.askhava.com starts sending, the owner portal shows daily sends, deferrals, bounces and complaints, and an alert fires when deferrals exceed 5% of a day's sends.

## 10. Review checklist (run before the PR)

- [ ] From is "Ask Hava" <jobs@notify.askhava.com>; Reply-To is the monitored help address; never no-reply.
- [ ] Subject states the job or account fact; transactional content first; no promotion, referral or recruiting line.
- [ ] Footer per SEC-028; "Manage email notices" link works.
- [ ] Plain-text part present; under 100 KB; links only to askhava.com; no shortener, pixel or click redirect.
- [ ] Action links signed, single-use, bound to one person and object (SEC-025); any unsubscribe URL is the reusable exception.
- [ ] Webhooks verified on the raw body, constant-time, timestamp checked, deduped, stored before 2xx.
- [ ] Bounce, complaint and suppressed events update the address status (BIZ-040, BIZ-045); re-verification clears the provider suppression.
- [ ] No homeowner name, address, email or phone in any subject, body, URL or push (SEC-034).
- [ ] Spanish template present for any business notice that has one in English (BIZ-018).
- [ ] Tests run against a fake provider; no live Resend key before Gate C (CLAUDE.md rule 5).

## 11. Sources

Mailbox providers:
- [M1] Google, Email sender guidelines: https://support.google.com/mail/answer/81126?hl=en
- [M2] Google, Email sender guidelines FAQ: https://support.google.com/mail/answer/14229414?hl=en-GB
- [M3] Google, Email subscription guidelines for senders: https://support.google.com/mail/answer/15263077?hl=en
- [M4] Google, Postmaster Tools dashboards: https://support.google.com/mail/answer/14668346
- [M5] Google, Change Gmail notifications: https://support.google.com/mail/answer/1075549?hl=en
- [Y1] Yahoo Sender Hub, best practices: https://senders.yahooinc.com/best-practices
- [O1] Microsoft announcement (page body did not load for this session): https://techcommunity.microsoft.com/blog/outlook/strengthening-email-ecosystem-outlook%E2%80%99s-new-requirements-for-high%E2%80%90volume-senders/4399730 ; summary read (secondary): https://petri.com/microsoft-outlook-email-authentication-rules/

Standards, industry, law:
- [I1] RFC 8058, Signaling One-Click Functionality for List Email Headers: https://datatracker.ietf.org/doc/html/rfc8058
- [K1] M3AAWG Sender Best Common Practices, 27 August 2026: https://www.m3aawg.org/sites/default/files/doc_files/m3aawg-sender-best-common-practices-aug-27-2026.pdf
- [F1] FTC, CAN-SPAM Act: A Compliance Guide for Business: https://www.ftc.gov/business-guidance/resources/can-spam-act-compliance-guide-business

Resend and Svix:
- [R1] Verify webhook requests: https://resend.com/docs/webhooks/verify-webhooks-requests
- [R2] Svix, verifying webhooks manually: https://docs.svix.com/receiving/verifying-payloads/how-manual
- [R3] Webhook event types: https://www.resend.com/docs/dashboard/webhooks/event-types
- [R4] Email suppressions: https://resend.com/docs/dashboard/emails/email-suppressions
- [R5] Webhook retries and replays: https://resend.com/docs/webhooks/retries-and-replays
- [R6] Receiving emails: https://www.resend.com/docs/dashboard/receiving/introduction
- [R7] Resend blog, DMARC policy modes: https://resend.com/blog/dmarc-policy-modes.md
- [R8] Resend pricing (knowledge base): https://resend.com/docs/knowledge-base/what-is-resend-pricing
- [R9] Subdomain or root domain: https://resend.com/docs/knowledge-base/is-it-better-to-send-emails-from-a-subdomain-or-the-root-domain

Practitioners and platforms (secondary):
- [B1] Bento docs, spam complaints and feedback loops: https://bentonow.com/docs/deliverability/complaints
- [L1] Al Iverson, Spam Resource, "Beware the link shorteners" (2021): https://www.spamresource.com/2021/07/beware-link-shorteners-or-at-least-be.html
- [P1] WebKit blog, Web Push for Web Apps on iOS and iPadOS (2023): https://webkit.org/?p=13878
- [P2] TechRepublic, iOS Mail notification and VIP settings (iOS 12 era; check current iOS wording): https://www.techrepublic.com/article/how-to-set-up-email-notifications-in-ios-12/

Not legal advice; no lawyer reviewed this file (D-367).
