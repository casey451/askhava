# Brain: SMS compliance (TCPA, CTIA, 10DLC, carrier filtering)

- **Purpose:** what the law, the carriers and Twilio require of Ask Hava's texts, and where the current plan meets or misses it.
- **Who reads it:** any session building or reviewing `messaging`, `intake` (first reply, opt-out, HELP), `site` (SMS terms, privacy, campaign copy), quiet-hours code, and any task whose prompt touches CON-*, TXT-*, QH-* or TERMS-011 to TERMS-014.
- **How to use it:** read the section for your task, run the review checklist (section 12) before you open the PR, and cite the source tag (`[S#]`, list in section 13) when you rely on a rule.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec, and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = not backed by a primary source; reasoning only.

## 1. Which rules actually bind Ask Hava

Three layers, from the one that bites first:

1. **Carriers and Twilio (contract).** Twilio's Acceptable Use Policy, A2P 10DLC registration and carrier filtering decide whether a single text is delivered. A refused campaign or a suspended number stops the service the same day. This is the layer most likely to hurt Ask Hava. [S12][S13][S14][S16]
2. **CTIA Messaging Principles and Best Practices (May 2023 edition).** Voluntary industry guidance, but carriers and Twilio enforce it through their terms. It is the latest edition found as of 2026-10-29. [S10]
3. **The TCPA and FCC rules (federal law).** Private suits with $500 to $1,500 per text in damages. [S1][S2]

**TCPA reach after Facebook v. Duguid.** The TCPA's consent rule for texts covers texts sent with an "automatic telephone dialing system" (or a prerecorded/artificial voice). The Supreme Court held in 2021 that an autodialer must use a random or sequential number generator to store or produce numbers [S5]. Texting people who wrote first, from a database, probably is not an autodialer call [inferred]. Do not rely on that: the FCC's revocation rules, state laws and carriers apply anyway, and the spec already treats every text as needing consent. Since *McLaughlin Chiropractic v. McKesson* (2025) district courts are not bound by FCC interpretations of the TCPA, so FCC rulings are strong evidence, not a guarantee [S6].

## 2. Consent: what Ask Hava has and why it is enough

**Consent types (CTIA 5.1, Exhibit II)** [S10]:

| Type | Consent CTIA expects | Ask Hava texts of this type |
|---|---|---|
| Conversational | Implied: "If the Consumer initiates the conversation and the Non-Consumer simply responds, then no additional permission is expected." | Direct replies to the homeowner's own message (intake questions, LIST, HELP answers) |
| Informational | Express: the consumer agrees to texts for a specific informational purpose, for example when giving their number | Quote updates, pick, who's-calling, check-ins over the following days |
| Promotional | Express written (checkbox, signed form) | **None. Ask Hava sends no promotional texts (TXT-008, TXT-009).** |

**TCPA "prior express consent".** For informational (non-telemarketing) texts the TCPA needs prior express consent, not prior express written consent [S2]. The FCC has long treated a person who gives their number for a purpose as consenting to calls about that purpose [inferred: this is the FCC's 1992 Order principle as later applied; the full order was not read for this file]. The 2015 Omnibus Ruling (FCC 15-72) held that an "on demand" text sent in response to a consumer's request is not subject to TCPA liability [S7].

**What a homeowner's inbound text gives Ask Hava.** Her first text asks Ask Hava for quotes on her own job. The immediate reply is conversational. The later texts over the next days are informational and tied to the purpose she started. The consent text (CON-031) then discloses "Automated texts about your request", frequency, rates, STOP and HELP. That is express consent for informational texts about her own request, given by the person who will receive them, to the sender who sends them [inferred from S7, S10, S13]. It does **not** cover marketing, texts about other jobs she did not start, or texts to tenants or on-site contacts (CON-021, TXT-009).

**Rules that follow** (already in the spec): no reply is required to continue (CON-004, CON-027); the web chat consents only by typing a mobile number and its Verify code (CON-005, CON-025); consent is to Ask Hava only and is never shared (CON-023) [S10 5.1.4, S12].

**The one-to-one consent rule is dead.** The FCC's 2023 rule (consent to one seller at a time, "logically and topically associated") was vacated by the Eleventh Circuit in *Insurance Marketing Coalition v. FCC* (No. 24-10277, January 2025) [S8], and the FCC then deleted it and restored the prior definition of prior express written consent [S9]. It mattered only for telemarketing; Ask Hava never relied on it. CON-023 (consent only to Ask Hava) is stricter than the old rule anyway.

## 3. Revocation: the 2024 to 2026 FCC changes (status on 2026-10-29)

- **2024 order (FCC 24-24).** Consumers may revoke by any reasonable means; revocation honored within 10 business days; a one-time confirmation text that "merely confirms" is allowed, presumed fine if sent within 5 minutes (47 CFR 64.1200(a)(12)) [S2][S3]. The "revoke-all" part (one revocation covers every unrelated robocall and robotext from that caller) was delayed several times, last to January 31, 2027, and never took effect [S4].
- **2026 order (FCC 26-67, adopted September 30, 2026, FR Doc. 2026-21821, published October 26, 2026, effective November 25, 2026).** Verified against the Federal Register public-inspection copy [S3] and a law-firm summary [S4]. It amends 64.1200(a)(10):
  - Callers may designate exclusive revocation methods (a reply text with any of the standard words, a key-press opt-out, or a designated website or phone number), disclosed in every message. Senders that designate none must honor revocation by any reasonable means.
  - The standard words are stop, quit, end, revoke, opt out, cancel and unsubscribe; a sender may show only STOP but must honor all seven.
  - A revocation made in reply to an informational text may be read as covering only that category; a revocation in reply to marketing covers all marketing.
  - The 10-business-day limit stays.
- **FNPRM (FR Doc. 2026-20820)**, comments due November 25, 2026, replies December 28, 2026: proposes 7 business days, mandatory two-way texting and a one-step revoke-everything method. Not law. [S4]

**Ask Hava against the 2026 rule.** CON-008, CON-010, CON-013, CON-032 and CON-035 are stricter than the rule: Ask Hava designates no exclusive method (D-372), honors natural-language and Spanish revocations, applies one opt-out to every text and automated call, and acts at once. Using the category-specific option would be allowed but is not planned; do not add it without a spec change.

**Plan note:** CON-032 cites "91 FR 67214". The FR document number, adoption date and effective date were verified; the page number 67214 was **not** (the Federal Register HTML page could not be fetched). CON-032 already requires a re-check before Gate C.

## 4. Opt-out, HELP, frequency, sender ID (CTIA and Twilio)

- **Opt-out.** Honor STOP and the normal words (stop, end, unsubscribe, cancel, quit); send one final confirmation and nothing after it (CTIA 5.1.3) [S10]. Twilio's default opt-out words on long codes are STOP, UNSUBSCRIBE, END, QUIT, STOPALL, REVOKE, OPTOUT and CANCEL (REVOKE and OPTOUT added May 13, 2025) [S15]. "OPT OUT" with a space is in the FCC list but **not** a Twilio keyword: the app must catch it and send the confirmation itself, as CON-030 already says.
- **Twilio Advanced Opt-Out** [S16]: STOP, START, UNSTOP and HELP are reserved and can't be removed; you can add keywords and language-specific pairs (Spanish supported; Twilio doesn't translate, and a keyword can't sit in two languages for one country); Twilio blocks later sends to an opted-out number with error 21610 and sends its own confirmation, so the app must not send a second one; the webhook carries `OptOutType` (START, STOP, HELP). Once Advanced Opt-Out is on, only Twilio support can turn it off.
- **HELP.** The reply must name the brand and give support contact (email, website or phone) [S13]. CON-018 does.
- **Frequency and rates.** The opt-in confirmation must carry the program name, "Message and data rates may apply" (that wording, verbatim, for the US), help info, opt-out instructions and a frequency disclosure [S13][S10 5.1.2.1]. CON-031 says "Msg & data rates may apply". [inferred] carriers accept the abbreviation in messages (it is the common form); the campaign's message-flow field should use the full words.
- **Sender ID.** The first message must identify the organization [S10 5.1.1]. WORD-008 starts every text "Ask Hava:".
- **Quiet hours.** The federal 8 a.m. to 9 p.m. limit applies to telephone solicitations only (47 CFR 64.1200(c)(1)) [S2]; CTIA sets no time-of-day rule [S10]. Arizona's 2023 amendment (HB 2498) brought texts into its do-not-call law, again for solicitations [S11]. QH-001 (8 a.m. to 8 p.m. in both Arizona and area-code time) is stricter than any rule found; it is good practice, not a legal floor.

## 5. 10DLC registration

- **What it is.** The Campaign Registry (TCR) is the hub where a Campaign Service Provider (Twilio) registers a **brand** (the business, verified by EIN; the EIN must match the legal name) and its **campaigns** (one per use case); carriers compare the description to real traffic [S17].
- **Brand.** Ask Hava LLC with its EIN. A business with an EIN can't register as a sole proprietor [S18]. A Low Volume Standard brand skips secondary vetting, gets trust score 0 and 12 messages per second in total; T-Mobile caps a non-Russell-3000 Low Volume Standard brand at 2,000 segments a day [S19][S20]. That is far above pilot volume [inferred]. Use the corporate email domain, not Gmail [S13].
- **Use case.** Standard use cases include CUSTOMER_CARE, ACCOUNT_NOTIFICATION and MIXED; LOW_VOLUME is a separate low-volume use case [S13]. TERMS-011 picks "Customer Care, Low-Volume Standard". [inferred] that pairing (Low Volume Standard brand, Customer Care campaign) is allowed; confirm in the Twilio Console at registration.
- **Description.** Say who sends, who receives and why; match samples and brand; no PII; the real business as sender [S13][S18]. Good shape: "Ask Hava LLC sends automated texts only to homeowners who text Ask Hava, or verify their number in the askhava.com chat, to ask for quotes on a job at their own home. Texts are about that request only: intake questions, confirmations, quote updates with a link to their private quote page, their pick and who will call them. No marketing. Businesses get no texts on this number; they are notified by email." (built from TERMS-011, TERMS-012, D-424).
- **Message flow (opt-in).** One flow per opt-in method: inbound text (keyword-free; say where the number is shown) and web chat (mobile number plus Verify code). Include service name, "Message and data rates may apply", frequency, support contact and opt-out. Gated or unpublished opt-in screens need public screenshots [S13][S18].
- **Samples.** 2 to 5 real messages, brand name in each, variables in brackets, STOP language in at least one [S13]. Tick "embedded links" (`has_embedded_links`) and, if any text carries a phone number, "embedded phone numbers" (`has_embedded_phone`) [S13].
- **Privacy policy and terms.** State that mobile information and opt-in consent are not shared or sold to third parties for marketing; Twilio's guide requires the phrase "text messaging originator opt-in data and consent" [S13]. SMS terms should carry program name and description, rates, frequency, support contact, HELP and STOP (bold), a privacy link and the carrier-liability line [S13]. `docs/legal/sms-terms.md` sections 1 to 10 and privacy-policy line 56 have all of these.
- **What gets rejected as lead generation.** Error **30951** (campaign) means "third-party lead generation or MLM content detected": collecting consumer information "to sell or distribute to other businesses as sales leads", acting "as an affiliate or intermediary", or reusing consent for unrelated businesses. **"The rejected campaign is not eligible for resubmission."** [S12]. Error **30871** (verification, toll-free) is "Disallowed: Third-party Lead Generation"; causes include a website, privacy policy or terms that "suggest you buy, sell, or share consumer or mobile opt-in data"; it can be corrected and resubmitted [S21]. Twilio's forbidden-categories page: "Any business whose terms or privacy policy mentions sharing or selling consumer data/opt-in information is considered noncompliant." [S14]

**Samples to submit** (real templates, TERMS-013, plus proposal 2 in section 10; variables in brackets as Twilio asks [S13]):

1. Consent text, CON-031: "Ask Hava: Got it. Next, a few quick questions. Ask Hava is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: askhava.com/terms Privacy: askhava.com/privacy"
2. First quote, TXT-016: "Ask Hava: Your first quote for request [n] is in, and more may come. Free for you. The business you pick pays Ask Hava a set introduction fee. Can't open links? Reply LIST to get your quotes by text. See them here: [link] Terms (18+): askhava.com/terms"
3. Pick confirmation, PICK-045: "Ask Hava: Done. [Business] has your pick. We'll text you when they accept. You can still pick another quote on your page."
4. Who's calling, TXT-027 (proposed addition; it carries the masked number).
5. STOP reply, CON-009.

**If a campaign is refused.** Read the error code first. 30951 can't be resubmitted [S12]; under CON-033, only with Casey's go, the homeowner stream moves to toll-free verification with the same content, and until then the service runs in web-and-email mode (TXT-021). A 30871 toll-free refusal can be corrected and resubmitted [S21]. Never register a second 10DLC campaign with softer wording for the same traffic, and never use a second number to get around a suspension (TXT-022) [inferred from S12].

**Risk, stated plainly.** Ask Hava is paid per introduction, and at the homeowner's direction it gives one business her first name and job address (PICK-022, TERMS-025, privacy policy line 36 and 42). A vetter reading the privacy policy can match that to the 30951 wording "distribute to other businesses". The honest distinctions are real: texts go only to the person who asked, from Ask Hava, about her own job; no phone number, opt-in or consent is ever passed on (CON-023, PICK-023); details go to one business she picked, after she ticks a box (PICK-049). Whether a carrier vetter accepts that is **unknown** [inferred]. The plan's answer (ask Twilio in writing first, D-305, D-424, TERMS-014; web-and-email mode if refused, TXT-021) is the right one. **Never word the campaign to hide the model** (TERMS-012 bans words, not facts): a description that misstates traffic breaks Twilio's terms and is the surest route to suspension [inferred from S12, S17].

## 6. Carrier content filtering

- **Forbidden categories** (Twilio, all number types; page last updated August 21, 2023): high-risk financial, third-party lead generation, debt collection or relief, get-rich-quick, cannabis/CBD/kratom (even 2FA), prescription drugs, gambling, phishing, SHAFT (Sex, Hate, Alcohol, Firearms, Tobacco) and vapes, academic fraud [S14]. No Ask Hava trade is in these. Watch free text that could quote a homeowner (AI-worded questions, AI-019): it must never echo a slur or firearm detail [inferred].
- **URLs.** Shared public shorteners "will lead to filtering"; shortened links must be company-branded [S14]. Links must represent the registered brand; no third-party redirects [S13]. TXT-006 (only askhava.com, no shortener) meets this. CTIA asks that linked sites name the owner and give contact info including a mailing address [S10 5.3.2]: askhava.com needs the LLC mailing address live before registration (sms-terms section 10 still shows "[mailing address]").
- **Phone numbers in the body.** CTIA: no numbers that route to unpublished numbers "unless the owner ... of such phone numbers is unambiguously indicated" [S10 5.3.3]. TXT-027 and TXT-030 put the masked call number in the text next to the business name.

**Mistakes that get a sender filtered or suspended** (each tied to a rule above):

- A text whose content is not in the registered campaign (a business notice, a promotion, a "refer a friend") [S13][S17].
- A link through bit.ly or any shared shortener, or a link that redirects off askhava.com [S14][S13].
- A phone number in the body with no owner named next to it [S10 5.3.3].
- Sending again after STOP, or sending a second confirmation after Twilio's [S16][S3].
- Consent reused for another purpose or passed to another business [S12][S10 5.1.4].
- A rising opt-out or complaint rate; CON-024 alerts at 3% of texted homeowners [inferred threshold, from the spec].
- Registering one campaign and sending a different mix of traffic, including from a test number on the live campaign (SEC-003 keeps test and live numbers apart) [S17].

## 7. Recordkeeping

CTIA 5.1.2 lists the opt-in record: timestamp, consent medium, the consent language, the campaign, IP address, the number and the person's identifier; keep opt-in and opt-out records so opt-outs are honored [S10]. The TCPA has a 4-year limitation period (28 U.S.C. 1658, applied to the TCPA in *Giovanniello v. ALM Media*, 2d Cir. 2013) [S22]. CON-005 (time, message ID or chat session, channel, consent-text version) and CON-022 (5 years after the last message) cover this; 5 years exceeds 4.

**Reassigned numbers.** The FCC holds callers liable for robocalls to a reassigned wireless number when the new user has not consented, with a limited one-call exception [S7]. Ask Hava texts a number only while its request is live (days to weeks) and never texts past customers first (TXT-009), so the exposure is small [inferred]. CTIA asks senders to process carrier deactivation files regularly [S10 5.1.5]; Twilio's delivery errors for a dead number should end texting to it (TXT-015) [inferred].

**What to keep for a complaint or suit** [inferred from S10, S22]: the inbound message ID and time that opened the request, the consent-text version and its send time, every outbound template ID with time and status (TXT-019), every opt-out and HELP event with its channel, and the campaign registration as submitted.

## 8. Spanish-language texting

No federal rule requires Spanish [inferred]. What does apply: an opt-out in Spanish is a "reasonable means" of revocation and must be honored (CON-008 includes parar, alto, cancelar, baja) [inferred from S2 and S3]; Twilio Advanced Opt-Out supports Spanish keyword and reply pairs that you must translate yourself [S16]. Disclosures should be in the language of the conversation [inferred]. TXT-039 keeps Spanish conversations off at launch with a fixed set of Spanish safety, help and opt-out texts on; CON-031 has a Spanish consent text that is GSM-7. A letter outside the GSM-7 set turns the whole text into UCS-2, with 70-character segments instead of 160. é and ¿ are in GSM-7, so CON-031's "Términos" is fine; í, Í and ú are not, so the Spanish TXT-031 line ("responda el número y SÍ") goes out as UCS-2 [inferred from the GSM 03.38 character table]. This is a cost and length issue, not a compliance one.

## 9. What the requirements already do right

| Rule | Requirement |
|---|---|
| Consent text first on every path, with name, frequency, rates, STOP, HELP, terms and privacy | CON-001, CON-002, CON-031 |
| No consent bundled with a purchase; no reply needed | CON-004, CON-027, sms-terms section 2 |
| Web opt-in proven by Verify code, wrong numbers get nothing | CON-025, TXT-038 |
| All seven FCC words plus Spanish, natural language, one opt-out for everything, one confirmation within 5 minutes that merely confirms | CON-008 to CON-010, CON-013, CON-030, CON-035 |
| "can stop by at 3" is not an opt-out | CON-011 |
| Never retry 21610 | CON-014 |
| HELP at any hour, even after opt-out | CON-018, QH-004 |
| Consent never shared; privacy phrase present | CON-023, TERMS-009, TERMS-010, privacy line 56 |
| Campaign covers only homeowner traffic; samples are real templates; business texting needs its own campaign | TERMS-011, TERMS-013, TXT-035 |
| Ask Twilio before registering; register only once the site is live | TERMS-014, D-305 |
| No shorteners, askhava.com links only, full links | TXT-005, TXT-006 |
| No homeowner name or address in any text to a business | TXT-036 |
| Quiet hours stricter than law | QH-001 to QH-009 |
| Records kept 5 years | CON-022 |
| Opt-out rate alert | CON-024 |
| Suspension handled without a second number | TXT-022, CON-033 |

## 10. Gaps and risks, each as a proposed requirement

These are proposals. A task may not add them on its own; they need a spec change logged in `docs/decisions.md`.

1. **CON-001 vs TERMS-011/TERMS-013 (conflict).** CON-001 sends a business-intent first message "one text with the business sign-up link" from the homeowner number, while TERMS-011 tells carriers "Businesses get no texts on this number" and TERMS-013 fails the build if any business template can be sent from that number. Proposed: "A first message matching the business-intent list gets no text: it opens no job, is listed in the weekly digest, and the business sign-up link appears only on askhava.com and in the HELP reply; or else TERMS-011's description says 'A person who texts asking to join as a business gets one reply with the sign-up link.' and that template is one of the TERMS-013 samples."
2. **Embedded phone number missing from the samples.** TXT-027 and TXT-030 carry the masked number, but TERMS-013's samples (consent text, first quote, pick confirmation, STOP reply) carry none. Proposed (TERMS-013): "The homeowner campaign samples add the who's-calling text (TXT-027) with its masked number, and registration ticks 'Messages will include embedded links' and 'Messages will include phone numbers'." [S13]
3. **Privacy-policy sharing sentence and 30871/30951.** Proposed (TERMS-025, TERMS-014): "Before registration, the privacy policy keeps the details-sharing sentence (TERMS-025) under its own heading 'When you pick a business', separate from the texting section, and the texting section says 'Your phone number, your texting permission and your messages are never shared with any business or third party.'; the wording Twilio's written answer asks for replaces it, logged in docs/decisions.md." [S14][S21]
4. **Mailing address on askhava.com before registration.** Proposed (TERMS-014): "The campaign is registered only after askhava.com and the SMS terms show the Ask Hava LLC mailing address." [S10 5.3.2]
5. **Opt-out evidence after bodies are deleted.** TXT-020 and PRIV-027 delete bodies after 30 to 31 days. Proposed (CON-035): "Each opt-out event also stores how it was detected (the keyword matched, or 'natural language' with the matched phrase pattern ID), kept with the consent log." [S10 5.1.2]
6. **Spanish disclosure when Spanish turns on.** Proposed (TXT-039): "Before 'Spanish conversations' is turned on, the homeowner campaign description says texts are in English or Spanish and a Spanish sample is added, or the campaign is updated in the Console if Twilio requires it." [inferred]
7. **Segment check on every Spanish template.** Proposed (TXT-001 test): "The template test reports each template's encoding (GSM-7 or UCS-2) and segment count, and fails if the consent text, HELP reply or opt-out confirmation needs UCS-2." [inferred]

## 11. Pre-registration checklist (Casey's hands; nothing is sent without his go, BUILD_BRIEF.md:61)

1. LLC formed, EIN letter in hand, legal name matches the IRS letter exactly [S17].
2. askhava.com live with request page, how-it-works, SMS terms, privacy policy, terms, help page and mailing address; no "under construction" page [S18].
3. Twilio's written answer to the D-424 question filed; wording changes it asks for made and logged (TERMS-014).
4. Brand: Low Volume Standard (or Standard if volume needs it), corporate email domain [S19].
5. Campaign: Customer Care; description as in section 5; two message flows (inbound text, web chat with Verify) with public screenshots of the chat start screen [S13].
6. Samples: real templates from TERMS-013 plus the who's-calling text; tick embedded links and phone numbers.
7. Advanced Opt-Out configured as CON-030, with Spanish keyword and reply pairs; test that "opt out" (two words) gets exactly one confirmation from the app [S16].
8. Opt-in confirmation, HELP and STOP replies in Twilio match CON-031, CON-018, CON-009 word for word.
9. Screenshot everything submitted; keep the TCR campaign ID with the consent-text version.

## 12. Review checklist for any task that touches texting

- [ ] Every new text starts "Ask Hava:", has English and Spanish templates, and is only about the recipient's own request (WORD-008, TXT-001, TXT-008).
- [ ] No promotional content, no cross-sell, no "save this number" (TXT-008).
- [ ] Links are full askhava.com links; no shortener; no third-party redirect (TXT-006) [S14].
- [ ] A new kind of text sent from the homeowner number goes to a homeowner only; if it changes the campaign's traffic, the campaign description and samples are updated first (TERMS-011, TERMS-013) [S17].
- [ ] No homeowner name, address, email or photo link in a text to anyone else (TXT-036).
- [ ] Suppression list checked before every send, whatever Twilio blocks (CON-035); 21610 never retried (CON-014).
- [ ] Opt-out detector unchanged or re-tested: seven FCC words, Spanish words, natural language, "stop by at 3" not an opt-out (CON-008 to CON-011).
- [ ] Exactly one opt-out confirmation, within 5 minutes, nothing else in it; none when Twilio already sent it (CON-009, CON-030) [S3][S16].
- [ ] Quiet hours applied, including to retries (QH-001, QH-008); any new any-hour text is listed in QH-004.
- [ ] Segment count checked with a GSM-7 counter; no curly quotes.
- [ ] Consent and opt-out events logged with time, channel and version (CON-005, CON-022).
- [ ] Fake phone numbers only in tests and fixtures (BUILD_BRIEF.md:66).
- [ ] No claim "tested" without a run in this repo (BUILD_BRIEF.md:57).

## 13. Sources and why they are authoritative

Primary law and regulation:
- [S1] 47 U.S.C. 227 (the TCPA itself), Cornell LII: https://www.law.cornell.edu/uscode/text/47/227 ; GovInfo official text: https://www.govinfo.gov/content/pkg/USCODE-2022-title47/html/USCODE-2022-title47-chap5-subchapII-partI-sec227.htm
- [S2] 47 CFR 64.1200, the FCC's TCPA rules, Cornell LII: https://www.law.cornell.edu/cfr/text/47/64.1200 (eCFR: https://www.ecfr.gov/current/title-47/chapter-I/subchapter-B/part-64/subpart-L ; not fetched, permission timed out).
- [S3] FCC 26-67 Report and Order, FR Doc. 2026-21821, Federal Register public-inspection copy (official pre-publication text): https://public-inspection.federalregister.gov/2026-21821.pdf ; FNPRM: https://public-inspection.federalregister.gov/2026-21820.pdf ; FCC order: https://docs.fcc.gov/public/attachments/FCC-26-67A1.pdf (not fetched). The Federal Register HTML page was blocked (permission timed out).
- [S4] Summaries of the 2024 to 2026 revocation history (secondary, law firms): https://commlawgroup.com/2026/fccs-new-tcpa-consent-revocation-rules-on-category-specific-opt-outs/ ; https://www.burr.com/telephone-consumer-protection-act/the-fcc-delays-effective-date-of-tcpa-revoke-all-rule-until-january-31-2027
- [S5] *Facebook, Inc. v. Duguid*, 592 U.S. 395 (2021), Supreme Court opinion: https://www.supremecourt.gov/opinions/20pdf/592us2r23_1bn2.pdf
- [S6] *McLaughlin Chiropractic Assocs. v. McKesson Corp.*, No. 23-1226 (June 2025), docket: https://www.supremecourt.gov/docket/docketfiles/html/public/23-1226.html ; summary: https://www.gibsondunn.com/supreme-court-holds-that-district-courts-are-not-bound-by-agency-interpretations-in-enforcement-proceedings/
- [S7] FCC 15-72 Omnibus Declaratory Ruling, Federal Register summary (80 FR 61129): https://www.govinfo.gov/content/pkg/FR-2015-10-09/html/2015-25682.htm
- [S8] *Insurance Marketing Coalition v. FCC*, 11th Cir. No. 24-10277 (Jan. 2025), court filings: https://epic.org/documents/insurance-marketing-coalition-v-fcc/ ; summary: https://www.mofo.com/resources/insights/250130-eleventh-circuit-vacates-fcc-s-tcpa-one-to-one-consent-rule
- [S9] FCC repeal of the one-to-one rule (secondary; order number not confirmed): https://www.womblebonddickinson.com/us/node/21214
- [S11] Arizona HB 2498 (2023), session law: https://www.azleg.gov/legtext/56leg/1R/laws/0041.htm ; summary: https://www.insideprivacy.com/advertising-marketing/tcpa/state-telemarketing-updates-arizona-washington-and-maryland/
- [S22] *Giovanniello v. ALM Media*, 2d Cir. 2013 (4-year limit under 28 U.S.C. 1658): https://caselaw.findlaw.com/court/us-2nd-circuit/1641310.html

Industry and carrier rules (binding through contracts):
- [S10] CTIA Messaging Principles and Best Practices, May 2023 (the wireless industry association's guidance carriers enforce): copy read at https://assets.ecourtdate.com/files/ctia-best-practices.pdf ; CTIA cites it at https://api.ctia.org/docs/default-source/default-document-library/230523-ctia-messaging-principles-and-best-practices-final.pdf (CTIA's own link, per its 2025 testimony: https://apps.oregonlegislature.gov/liz/2025R1/Downloads/PublicTestimonyDocument/151671).
- [S17] The Campaign Registry FAQ (the registry carriers use): https://www.campaignregistry.com/?p=1148

Twilio (Ask Hava's provider; its pages state what it will accept):
- [S12] Error 30951: https://www.twilio.com/docs/api/errors/30951
- [S13] A2P 10DLC Campaign Onboarding Guide: https://support.twilio.com/hc/en-us/articles/11847054539547-A2P-10DLC-Campaign-Onboarding-Guide
- [S14] Forbidden message categories (US and Canada): https://support.twilio.com/hc/en-us/articles/360045004974-Forbidden-Message-Categories-for-SMS-and-MMS-in-the-US-and-Canada
- [S15] Opt-out keywords added per FCC ruling (May 13, 2025): https://www.twilio.com/en-us/changelog/opt-out-additional-keywords-added-per-fcc-ruling
- [S16] Advanced Opt-Out: https://www.twilio.com/docs/messaging/tutorials/advanced-opt-out
- [S18] Improving your chances of 10DLC approval (Twilio blog, March 28, 2025): https://twilio.com/en-us/blog/improving-your-chances-of-a2p10dlc-registration-approval
- [S19] Throughput and trust scores: https://support.twilio.com/hc/en-us/articles/1260803225669-Message-throughput-MPS-and-Trust-Scores-for-A2P-10DLC-in-the-US
- [S20] T-Mobile daily limits: https://support.twilio.com/hc/en-us/articles/1260804800549-T-Mobile-Daily-Message-Limits-for-Long-Code-Messaging-with-A2P-10DLC
- [S21] Error 30871: https://www.twilio.com/docs/api/errors/30871

Not legal advice; no lawyer reviewed this file (D-367).
