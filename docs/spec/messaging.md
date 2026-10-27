# Spec: Texts and templates

Prefix: `TXT` · Modules: `messaging` · Format and checks: [index.md](index.md)

- **TXT-001** Every text kind has an English and a Spanish template, and a test fails if any text kind lacks a Spanish template. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:88`, `BUILD_BRIEF.md:416`.
- **TXT-002** A homeowner whose language is Spanish gets every text from the Spanish template. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:266`.
- **TXT-003** Spanish templates are marked as drafts for native-speaker review. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:266`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:146`.
- **TXT-004** Every text is built from a fixed template selected by template ID and language; the AI never writes texts to homeowners. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:286`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:187`.
- **TXT-005** Texts use plain words, carry no meaning only through emoji, and show full links. Source: `BUILD_BRIEF.md:415`.
- **TXT-006** Every link inside a text points to askhava.com, and no public link shortener is ever used. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:29`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:263`.
- **TXT-007** The ROC complaint link in texts is askhava.com/help/roc-complaint, which links onward to the ROC. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:117`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:91`.
- **TXT-008** Homeowner texts are only about the homeowner's own request, with no promotions, cross-selling or "save this number" wording. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:181`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:262`.
- **TXT-009** No text is sent to a past customer unless they write first. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:262`.
- **TXT-010** No template says "reply CANCEL"; texts tell homeowners to "Reply CLOSE" plus the job number. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:199`.
- **TXT-011** The outbox never sends the same message twice: every send carries an idempotency key derived from what it identifies. Source: `BUILD_BRIEF.md:140`, `BUILD_BRIEF.md:149`.
- **TXT-012** Inbound messages and delivery-status notices are de-duplicated by Twilio's message ID. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:201`.
- **TXT-013** Delivery status (queued, delivered, failed, undelivered) is tracked for every text. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:187`.
- **TXT-014** Important texts (quote link, confirm request, who's calling) are retried up to 3 times over 30 minutes, then sent by email and shown in the portal. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:187`.
- **TXT-015** Every failed text is retried, and when a homeowner's texts fail 3 times with no email on file, Casey gets a call-back alert. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:211`.
- **TXT-016** The homeowner is texted when the first quote arrives, with a note that more may come and that they can pick any time. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:82`.
- **TXT-017** Quote-count update texts (for example "You now have 4 quotes") go out at most once per 3 hours per job, measured from the delivery time of the previous update (D-012). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:84`.
- **TXT-018** The first status text offers "Want updates by email too?". Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:209`.
- **TXT-019** The message log stores a vault reference instead of the phone number, plus kind, template ID, language, request ID, sent time, Twilio status and attempts. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:95`.
- **TXT-020** Message bodies are reduced to their template ID 30 days after sending, while the log entry is kept for the consent-log period (D-013). Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:95`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:438`, `BUILD_BRIEF.md:356`.
- **TXT-021** A texting on/off switch in the owner portal (web-and-email mode) stops all outbound texts, and updates then go by email and the quote page. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:177`, `BUILD_BRIEF.md:464`.
- **TXT-022** A suspended Twilio number pauses all texting and switches to email plus the portal, and no second number is ever used to get around a suspension. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:187`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:267`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:75`.
- **TXT-023** During a texting suspension, everyone who gave no email (new texters and open jobs needing an update) goes on Casey's call list in the owner portal. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:209`.
