# Brain: Terms, privacy and consumer-protection law for a two-sided local service (Arizona, United States)

- **Purpose:** the laws, rules and vendor terms that shape Ask Hava's homeowner terms, business terms, privacy policy, SMS terms, email notice terms and incident plan, and where the current drafts and requirements meet or miss them.
- **Who reads it:** any session drafting or checking a file in `docs/legal/`, or a task touching TERMS-*, PRIV-*, WORD-*, CON-005, CON-007, CON-021, OPS-020, OPS-021, JOB-017, PICK-016 or PICK-049; in particular T111 (homeowner and business terms), T112 (privacy policy and SMS terms), T113 (Twilio campaign description) and T114 (terms-match-code checks).
- **How to use it:** read the section for your task, run the review checklist (section 17) before you open the PR, and cite the source tag (`[L#]`, list in section 18) when you rely on a rule.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec, and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = not backed by a primary source; reasoning only. Anything here about how a court would read a clause is `[inferred]`.

**This is not legal advice.** It is engineering-facing research by an AI. No lawyer has reviewed it (D-367), and the drafts it checks say the same at their top (TERMS-028).

## 1. The stance in one paragraph

Ask Hava sells one thing: an introduction service, paid for by the business the homeowner picks (WORD-023). Every document in `docs/legal/` has to say that one thing the same way, never promise more than the code does, and keep a record that proves who agreed to what. Three bodies of law do most of the work: the FTC Act and Arizona's Consumer Fraud Act (what you say must be true and not misleading), the Arizona Registrar of Contractors statutes (who may say they do construction work), and the electronic-signature acts (when a tap counts as a signature and what record proves it). The rest are narrower: breach notice, CAN-SPAM, COPPA, texting, accessibility, vendor terms and sales tax.

## 2. Truthful claims: FTC Act, Arizona Consumer Fraud Act, dark patterns, reviews

- **FTC Act section 5** makes "unfair or deceptive acts or practices in or affecting commerce" unlawful; an act is unfair only if it causes substantial injury "not reasonably avoidable by consumers themselves" and not outweighed by benefits [L1].
- **Deception** has three parts: a representation, omission or practice "likely to mislead"; judged from a consumer acting "reasonably in the circumstances"; and material, meaning "likely to affect the consumer's conduct" [L2]. The FTC reads "the entire mosaic, rather than each tile separately", and "Written disclosures or fine print may be insufficient to correct a misleading representation" [L2]. For Ask Hava: a terms clause cannot fix a page that says otherwise. The not-a-contractor statement belongs on the pages (WORD-002), not only in the terms, which the plan already does.
- **Arizona Consumer Fraud Act**: A.R.S. 44-1522(A) bans "any deception, deceptive or unfair act or practice, fraud, false pretense, false promise, misrepresentation" in connection with the sale or advertisement of merchandise, and a violation occurs "whether or not any person has in fact been misled, deceived or damaged thereby" [L5]. "Merchandise" includes "services" and "intangibles" [L5], so both sides of Ask Hava are covered: the free service to homeowners (advertised) and the paid introduction to businesses.
- **Dark patterns.** The FTC staff report groups them as design that induces false beliefs, hides or delays material information, leads to unauthorized charges, or obscures privacy choices [L3]. Its lines that bear on Ask Hava: don't bury "key limitations of the product or service in dense Terms of Service documents"; include "any unavoidable and mandatory fees in the upfront, advertised price"; get "the express informed consent of the accountholder to any charges" through "an affirmative, unambiguous act"; make cancellation and opt-outs "at least as easy to use" as signing up; and it lists a "pre-checked box" as a trick [L3]. The Confirm page's fee, tier and refund line before the press (PICK-021), the one suppression list fed by every channel (CON-035) and the unticked save-card box (FEE-015) already follow this. PICK-049's share box, "ticked by default and required to pick", does not (proposal 4).
- **Consumer reviews rule (16 CFR part 465, 2024)** bans fake reviews (465.2), buying reviews (465.4), undisclosed insider reviews (465.5) and review suppression (465.7) [L4]. Under 465.7(b) a business may not misrepresent that the reviews shown "represent most or all the reviews submitted" when reviews are suppressed "based upon their ratings or their negative sentiment"; removing reviews for neutral reasons applied to all (another person's personal information, abusive or discriminatory content, reviews believed fake, reviews unrelated to the service) is allowed [L4]. JOB-017 (no reward, no ticked feedback left out for being negative) meets the core rule; proposal 14 adds the allowed-removal list and an insider rule.
- **HomeAdvisor.** The FTC's 2023 order against misleading lead claims (ready-to-hire, free-that-isn't-free, unsupported win rates) is covered in `contractor-lead-experience.md` section 1 [C1][C2]; WORD-026 and WORD-030 carry it into the plan. Don't repeat it in the terms; do keep every business document free of the banned claims (`scripts/check_legal_docs.py` scans for them).

## 3. "Ask Hava is not a contractor": the Registrar of Contractors statutes

- **A.R.S. 32-1151**: no person may engage in the business of contracting, submit a bid, or "act as, offer to act as, or purport to have the capacity of a contractor" without a license, unless an exemption applies [L7].
- **A.R.S. 32-1165**: it is "a class 1 misdemeanor for any person to advertise that the person is able to perform any service" regulated by the Registrar without a license, except as section 32-1121(A) allows [L8]. This is why WORD-022's banned list ("our pros", "we'll send a plumber", "book", "hire through us") matters legally and not only for tone: copy that offers to perform the work is the advertising the statute names [inferred: application of L7, L8].
- **The handyman exemption, A.R.S. 32-1121(A)(14)**: one project whose aggregate price, labor and materials, is "less than $1,000", "of a casual or minor nature", with no "local building permit", not part of a larger operation or split to evade, and the person's advertising must include "not a licensed contractor" [L6]. 32-1121(D) takes fire safety and gas connections out of the exemption [L6], and 32-1121(C) lets the Attorney General treat a person without the exemption under 44-1522 [L6].
- **How the plan meets it:** LIC-024 puts "Not a licensed contractor. Only for jobs under $1,000 total with no permit." on every handyman card (the advertising disclosure); LIC-025's per-quote certification restates every exemption condition, including gas, smoke-alarm and sprinkler work; LIC-019 refuses $1,000.00 and up; business terms section 2 repeats it. The terms are the right place for the certification because the certification is the business's own promise, while the card line is the public disclosure [inferred].
- **Ask Hava's own position:** it never bids, prices, performs or supervises (WORD-011, TERMS-026), so it does not "purport to have the capacity of a contractor" [inferred]. Any feature that sets prices, schedules visits or picks a business for the homeowner would erode that position; it is a spec change, never a task's choice.

## 4. What each document in `docs/legal/` must contain

| Document | Must contain (plan) | Why (law or vendor) | Gaps found |
|---|---|---|---|
| Homeowner terms | 18+, Arizona law, no arbitration in effect, not an emergency service, WORD-002, no guarantee of the work, warranty disclaimer, $100 cap with carve-outs, no personal-injury waiver, e-signature clause (TERMS-001 to TERMS-004, WEB-026) | Deception rules [L2][L5]; e-sign acts [L13][L14]; Arizona Constitution art. 18 sec. 6 says the right to recover damages for injuries "shall never be abrogated" [L25] | No severability, entire-agreement, assignment or notices clause (proposal 5); acceptance by a text pick not described (proposal 1); section 5 says the Registrar "takes complaints at askhava.com/help/roc-complaint", which reads as if the Registrar sits at Ask Hava's address; say Ask Hava's help page links to the Registrar's form [inferred] |
| Business terms | WORD-002 business form, TERMS-026 independence paragraph, card authorization, handyman certification, refunds as coded, 72-hour no-refund, non-discrimination, data-use limit, charge contact (TERMS-005 to TERMS-007, TERMS-019, TERMS-022, TERMS-024, TERMS-026, TERMS-027) | Deception and charge consent [L1][L3]; Stripe holds the user responsible for taxes and for the consents needed to share data with Stripe [L20] | No severability or general section (proposal 5); no termination clause beyond pauses (proposal 6); cap can be $0 for a business whose only win was free (proposal 7); no plain "no promise of jobs" line (proposal 8); change notice has no lead time (proposal 9) |
| Privacy policy | TERMS-008 to TERMS-010, TERMS-020, TERMS-025, TERMS-031, AI-018, processor list (PRIV-027), children, Do Not Track | Promises in a privacy policy are representations under section 5 [inferred from L1, L2]; Twilio's policy bans transferring consent [L21]; Stripe 4.5 consents [L20] | Says "see and change" but offers no copy of the data (proposal 11); help-email deletion has no identity check (proposal 11) |
| SMS terms | CON-005 wording, frequency, rates, HELP, STOP, the mailing address (TERMS-032) | CTIA and Twilio; see `sms-compliance.md` [S10] | None beyond that brain's proposals |
| Business email notice terms | Consent to notices, what is sent, records kept, recruiting emails | CAN-SPAM [L16] | Recruiting emails don't say they are advertisements (proposal 16) |
| Accessibility statement | Target, known limits, contact, relay line (A11Y-020) | DOJ guidance [L18] | None |
| Information security program | Safeguards Rule analysis, inventory, vendors | 16 CFR 314 [L12] | EIN not listed as sensitive (proposal 13) |
| Incident response plan | 45-day notice, pause and preserve first (OPS-020, OPS-021) | A.R.S. 18-551, 18-552 [L9][L10] | Section 10 below |

## 5. Usual clauses: present, missing, or deliberately left out

| Clause | Homeowner terms | Business terms | Note |
|---|---|---|---|
| Limitation of liability | $100 cap, gross negligence and intentional misconduct carved out (TERMS-004) | Fees paid in prior 12 months | A cap that can be $0 risks being read as no remedy at all [inferred]; proposal 7 |
| Warranty disclaimer | Yes (section 9) | Yes (section 10) | |
| Personal-injury carve-out | Yes | Not stated | Business claims are commercial; [inferred] not needed, but harmless to add |
| Dispute resolution | Write first, Mohave County courts, small claims | Mohave County courts, small claims | Small claims in Arizona justice courts covers claims up to $5,000 [L26], above both caps. No informal step for businesses; proposal 5 |
| Arbitration and class waiver | OPTIONAL, OFF (TERMS-002, D-388) | None | Leave off; Casey's decision |
| Governing law | Arizona | Arizona | |
| Modification notice | Posted, told before it applies to a sent request | Emailed before it applies | No lead time; proposal 9 |
| Severability, entire agreement, assignment, no waiver, notices | Missing | Missing | Proposal 5 |
| Independent-contractor disclaimer | WORD-002 | TERMS-026 (thorough) | |
| No guarantee of work | "doesn't guarantee the work" (TERMS-004) | "no warranty ... about the number of job requests" | Proposal 8 makes it plain |
| Refund terms matching code | n/a | TERMS-005, TERMS-019 | Section 8 |
| Non-discrimination | n/a | TERMS-006 | Ask Hava's own routing should never use a protected trait [inferred]; it uses trade, town and size only (CARD-001, BIZ-010) |
| Termination | Not stated | Pauses and removals only | Proposal 6 |
| Indemnity | None (right for consumers [inferred]) | Business indemnifies, except Ask Hava's sole negligence | |

## 6. Electronic assent and the consent record

- **The law.** ESIGN: a record or signature "may not be denied legal effect ... solely because it is in electronic form" [L13]. Arizona's UETA, A.R.S. 44-7007: a contract formed by an electronic record "cannot be denied legal effect and enforceability" for that reason, and "An electronic signature satisfies any law that requires a signature" [L14]. ESIGN's consumer-disclosure consent (7001(c)) applies only when a law requires information to be given in writing [L13]; no law requires Ask Hava's terms in writing, so the plan's tap-to-accept is enough [inferred].
- **When a tap binds.** The Ninth Circuit (which covers Arizona) requires "reasonably conspicuous notice of the terms" and an action that "unambiguously manifests ... assent" [L15]. It faulted "a tiny gray font considerably smaller than the font used in the surrounding website elements", said "the fact that a hyperlink is present must be readily apparent", and that "Simply underscoring words or phrases ... will often be insufficient" [L15]. PICK-016 puts "By picking, you agree to the Terms" beside the Pick button in body-size text with an "underlined link": size is right; the link should also differ in color from the line around it (proposal 3).
- **Text picks.** TXT-031 and PICK-046 let a homeowner pick by replying "2 YES" after a text ending "By picking, you agree to the Terms: askhava.com/terms". That is acceptance by a reply to a text. TERMS-001 says the terms are accepted "only at the pick (PICK-016), never by a reply to a text". Read literally, the two conflict; the intent seems to be that the consent text or first message never accepts the terms [inferred]. Proposal 1 rewords TERMS-001, and the homeowner terms section 8 should describe both ways of picking.
- **What a record must prove.** Where a law requires a record to be kept, an electronic one counts if it "accurately reflects the information" and "remains accessible for later reference" (A.R.S. 44-7012) and is "capable of being accurately reproduced" (ESIGN 7001(d)) [L13][L14]. To prove assent later, Ask Hava must be able to show the exact text the person agreed to, not only a version number [inferred from L13, L15]. TERMS-016 and PICK-016 store version, time and request ID. Proposal 2 adds the text hash, language, channel, the assent line shown and an immutable copy of each version.
- **Texting consent** (CON-005, CON-007, CON-021) is separate from terms assent and is covered in `sms-compliance.md` section 7 [S10]. Twilio requires senders "to retain proof of all consents" at least until consent is withdrawn [L21]; CON-022's 5 years exceeds that.

## 7. Plain-language drafting

- No Arizona statute sets a reading level for consumer contracts [inferred: none found]. The federal Plain Writing Act binds only executive agencies, but its definition is the usual yardstick: writing that is "clear, concise, well-organized" and suited to its audience [L24].
- The plan's own bar is stricter than any law: WORD-024 (15-word sentences), WORD-029 (20-word limit, grade 8 homeowner, grade 9 business in site copy and templates), WORD-027 (no AI smell), WORD-028 cold read. Fixed legal sentences are exempt by requirement ID.
- Why it matters legally: a term buried in dense text is one the FTC calls a dark pattern [L3], and a court asks whether notice was reasonably conspicuous [L15]. Short, plain clauses make both easier to show [inferred].
- `scripts/check_copy.py` does not scan `docs/legal/` today. Proposal 15 sets a measured grade for the legal drafts.

## 8. How a terms-match-code test works (TERMS-005, TERMS-019, T114)

A terms-match-code test makes the document and the code read the same source of truth, so neither can drift alone [inferred]. It should:

1. Read each number the business terms state (72 hours, 96 hours, 2 attempts, 3 hours apart, 8 a.m. to 8 p.m., 4 business hours, 60 minutes, 1 business hour retry window, $25, $75, $199, $500.00, $1,500.00, 4 months, $75 cap, 90 days, 180 days, 6 months) from the drafted text by a fixed pattern, and compare each with the constant or `catalog.setting` default the code uses.
2. Map each numbered refund in business terms section 7 to one code path (FEE-021 to FEE-025, FEE-022's conditions one by one) and fail if a code path has no terms line or a terms line has no code path.
3. Assert the negatives: no refund for a business that never called within 72 hours (TERMS-019), no partial refund, no refund because work wasn't done or paid for (TERMS-026).
4. Run on every PR that touches `payments`, `picks`, `catalog` fee rules or `docs/legal/business-terms.md`.
5. Extend the same pattern to the privacy policy's retention numbers (180 days, 30 days, 3 years, 5 years, 7 years) against PRIV-007 to PRIV-010 and CON-022 (proposal 12).

## 9. Privacy without a comprehensive Arizona privacy law

- Arizona has not enacted a comprehensive consumer privacy act of the California or Colorado kind [inferred: none found in A.R.S.; the IAPP tracker at https://iapp.org/resources/article/us-state-privacy-legislation-tracker did not render its state list when fetched]. So no statute gives an Arizona homeowner a right to access, delete or port data.
- **Why PRIV-011 (delete) and PRIV-021 (review and change) are still right:** (a) every promise in the privacy policy is a representation the FTC and the Arizona Attorney General can enforce if untrue [inferred from L1, L2, L5]; (b) homeowners from states with such laws may use the service for an Arizona property [inferred]; (c) data not kept cannot leak, which shrinks section 10's exposure [inferred]; (d) Twilio and Stripe require the consents and opt-outs the plan already keeps [L20][L21].
- **What is missing:** a copy of one's own data, and an identity check when deletion is asked by email (the policy offers both portal and email). Proposal 11.
- **Retention.** The plan's periods (PRIV-007 to PRIV-010, CON-022) each have a reason: 180 days for contact detail covers complaints and disputes; 5 years for consent exceeds the TCPA's 4-year limit [S10]; 7 years for ledger and audit records. Terms-acceptance records have no period set. An action on a written contract in Arizona must be brought within six years after it accrues (A.R.S. 12-548, worded for debt) [L27]; proposal 17 keeps acceptance records 7 years, matching the ledger [inferred].

## 10. Breach response: the incident plan checked against A.R.S. 18-551 and 18-552

**What the law covers.** "Personal information" is a first name or initial and last name with a "specified data element" (Social Security number, driver license number, a private key used to sign, a card or account number with its code, health insurance ID, medical information, passport number, an IRS taxpayer identification number or IP PIN, biometric data used to log in), or a username or email "in combination with a password or security question and answer" [L9]. Only "unencrypted and unredacted computerized personal information" counts, and data is encrypted only if unreadable "without using a confidential process or key" [L9][L10].

**What the law requires** [L10]: notice to each individual "within forty-five days after the determination" of a breach; paused while law enforcement says notice would impede an investigation; contents: "The approximate date of the breach", a brief description of the information, the three largest credit bureaus' toll-free numbers and addresses, and the FTC's toll-free number, address and website; methods: written notice, email, or telephone with direct contact and "not a prerecorded message"; substitute notice (a letter to the Attorney General and a website posting for at least 45 days) only above $50,000 in cost, 100,000 people, or without contact details; if "more than one thousand individuals" must be notified, also the three largest credit bureaus, the Attorney General and the director of the Arizona Department of Homeland Security; a vendor that holds data it doesn't own must tell the owner "as soon as practicable". Only the Attorney General enforces, for knowing and willful violations, up to the lesser of $10,000 per affected individual or their total loss, capped at $500,000 per breach [L10]. The Attorney General takes notices through an online form or a PDF form [L11].

**The plan against it** (`docs/legal/incident-response-plan.md`):

| Point | Plan | Verdict |
|---|---|---|
| 45-day notice (OPS-020) | "within 45 days" | Meets [L10] |
| Over 1,000 people | Credit bureaus, AG, ADOHS director | Meets; the statute counts individuals notified, not only Arizona residents [L10] |
| Contents | Date, description, bureau and FTC contacts | Meets [L10] |
| Method | "by email or text" | Text is not a listed method [L10]; proposal 10 |
| Substitute notice | Missing | Proposal 10 |
| Law-enforcement delay | Missing | Proposal 10 |
| AG notice route | Not named | Use the AG's form [L11] |
| Vendor duty | "Tell any vendor involved" | Add: processors must tell Ask Hava as soon as practicable [L10] |
| Scope | Homeowner vault "probably not" notifiable | Agreed: names, phones, addresses and photos are not specified data elements [L9]. But the owner's account (email plus password, SEC-005) is covered, and a sole proprietor's EIN collected under PRIV-012 is an IRS taxpayer identification number tied to a person [inferred from L9]; proposal 13 |
| FTC Safeguards notice | 500 or more people, 30 days | Meets 16 CFR 314.4(j): "no later than 30 days after discovery" [L12] |

The Attorney General's form asks whether the reporter is subject to Gramm-Leach-Bliley Title V, which may exempt it [L11]. The security program treats Ask Hava as a Safeguards Rule "finder". Don't rely on that exemption; notify under 18-552 anyway, as the plan already does [inferred].

## 11. CAN-SPAM for business emails

- CAN-SPAM's opt-out, 10-business-day and identifier-and-postal-address duties apply to commercial email; the false-header ban also reaches transactional or relationship messages [L16]. Job cards, pick emails and receipts are transactional; `email-deliverability.md` section 4 explains why and how to keep them so [F1].
- **Recruiting emails** to businesses that have not signed up (business email notice terms section 6) are commercial. The FTC lists: no false headers, no deceptive subject lines, "Identify the message as an ad", tell recipients where you are, how to opt out, honor opt-outs "within 10 business days", and monitor anyone sending for you; up to $53,088 per email [L16]. The draft covers identity, postal address and opt-out (honored at once) but not the ad disclosure; proposal 16.

## 12. Children: COPPA and INT-038

- COPPA does not require a general-audience service to ask ages; one that screens "in a neutral fashion may rely on the age information its users enter" [L17]. Once an operator knows a user is under 13, it "must either meet COPPA's notice and parental consent requirements or delete the child's information"; after a one-time reply it must delete the child's contact details and not re-contact [L17].
- The plan refuses under-18 senders on their own stated age (INT-038), gives one reply, closes without a card, and deletes under-13 data except the consent or opt-out record; TERMS-002 and the privacy policy section 12 say the same. Keeping a phone number only to honor a STOP is a narrow use that serves the child [inferred]; it is the one retention the policy names.
- Why 18 and not 13: a minor can disaffirm most contracts, and the pick is an acceptance of terms [inferred]. CON-025's "must be 18 or older" start-screen line is a statement, not an age gate, which keeps the screening neutral [inferred from L17].

## 13. Texting and accessibility (pointers)

- **Texting (TCPA, CTIA, 10DLC):** `sms-compliance.md`. Its proposals adopted in D-490 became TERMS-029 to TERMS-032.
- **Accessibility:** `accessibility-forms.md`. The DOJ has no web regulation for private businesses under Title III; it says businesses must make online offerings accessible and names WCAG and Section 508 as "helpful guidance" [L18]. Its 2024 Title II rule for state and local governments adopts WCAG 2.1 Level AA, with compliance dates now April 26, 2027 and April 26, 2028 [L19]. A11Y-001 (WCAG 2.2 AA) sits above that reference.

## 14. Vendor terms the documents must not contradict

- **Stripe Services Agreement** [L20]: the user "must obtain all necessary rights and consents" for Stripe to handle third-party data (4.5), so the privacy policy names Stripe (it does); the user has "sole responsibility and liability" for which taxes apply (7.3(b)(i)), so FEE-008 and FEE-046 are Ask Hava's to get right; Stripe "is not obligated to provide support to Customers" (1.3), so the business terms name Ask Hava, not Stripe, as the charge contact (TERMS-022). Payment-flow rules are in `payments-stripe.md`.
- **Twilio Messaging Policy and AUP** [L21]: consent may not be bought, sold, rented or transferred, and "only applies to the recipient that provided that consent"; every message must "clearly identify you"; the customer answers for its end users' compliance. TERMS-009, TERMS-010, CON-021 and WORD-008 match.

## 15. Sales tax on the introduction fee (D-444, FEE-008, FEE-046)

- **The risk.** In LR21-003 ADOR taxed a recurring data subscription (compiled contact data) as a rental of tangible personal property, finding that data delivered electronically is still taxable and that the contract's "limited use license" label had "no bearing on taxability" [L23]. If the introduction fee were read as a sale or rental of the homeowner's name and address, it could be taxable, and since fees are tax-included (FEE-008), the tax would come out of the $25, $75 or $199 [inferred]. TERMS-027 and WORD-026 frame the fee as a service for exactly this reason (D-378).
- **What a private taxpayer ruling request contains** (A.R.S. 42-2101) [L22]: in writing; the taxpayer's name, address and identifying number "if applicable"; all facts "relevant to the requested ruling"; whether the issue is under audit, protest or appeal or filed with another taxing authority; a signature. ADOR must meet within 30 days unless waived and rule within 90 days, or explain why not. A ruling binds only for that taxpayer on those facts, and protects only if the facts given were accurate and unchanged [L22]. An anonymous filing by a representative becomes a "taxpayer information ruling" with weaker protection [L22].
- **For Ask Hava** [inferred]: file in the LLC's name; describe the flow in the TERMS-027 words; attach the business terms and a Confirm-page screenshot; ask about each classification D-444 lists. Keep FEE-046's set-aside until the answer is filed. Proposal 18.

## 16. Requirement map and proposed requirements

**Right already:** one description of the business everywhere (WORD-023); not-a-contractor on the pages, not only in the terms (WORD-002, WORD-022) [L2][L7][L8]; handyman disclosure and certification (LIC-024, LIC-025) [L6]; fee wording as a service (TERMS-027) [L23]; no arbitration in effect (TERMS-002); personal-injury claims never waived (TERMS-004) [L25]; e-signature clause with A.R.S. 44-7007 (TERMS-001) [L14]; acceptance logged (TERMS-016, PICK-016); opt-out as easy as opt-in (CON-035) [L3]; unticked save-card box (FEE-015) [L3]; no reward and no negative-feedback suppression (JOB-017) [L4]; consent never shared (TERMS-009, TERMS-010) [L21]; retention periods named (TERMS-020, PRIV-007 to PRIV-010); 45-day breach notice and pause-first (OPS-020, OPS-021) [L10]; under-18 refusal and under-13 deletion (INT-038) [L17]; legal drafts checked by script (TERMS-028).

**Proposed** (need a spec change logged in `docs/decisions.md`; a task may not add them on its own):

1. **Text picks accept the terms** (TERMS-001, PICK-016, TXT-031): "The homeowner terms are accepted only by a pick, on the quote page or by a '<number> YES' reply to a text that ends 'By picking, you agree to the Terms: askhava.com/terms', and never by the first text or any other reply; the homeowner terms describe both ways." [L14][L15]
2. **Assent record** (TERMS-016, PICK-016, BIZ-030, PICK-021): "Each acceptance stores the document, version, SHA-256 of the exact published text, language, channel (quote page, text pick, business sign-up, Confirm page), the assent line shown, time, and the request, pick or business-person ID (and the inbound message ID for a text pick); each published version's full text is stored once and never changed, and a test fails if a published version's hash changes." [L13][L14]
3. **Visible terms link** (PICK-016): "The Terms link beside the Pick button is underlined and in the site's link color, distinct from the sentence around it, at body size or larger; the Gate B script checks it on a phone." [L15]
4. **No pre-ticked required box** (PICK-049): "The share line at the pick is either a plain statement beside the Pick button or a required box that starts unticked; no box on any page starts ticked." [L3]
5. **General clauses** (TERMS-028, both terms): "Both terms end with a General section holding severability ('If any part of these terms can't be enforced, the rest still applies.'), entire agreement naming the privacy policy, SMS terms and email notice terms, no waiver, assignment (Ask Hava may assign to a successor with notice; a business may not assign without Ask Hava's written OK), notices by email, and, in the business terms, a write-first step with 30 days to resolve before court except small claims; `check_legal_docs.py` checks the severability sentence in both." [inferred]
6. **Ending a business account** (business terms section 11, LIC-025, TERMS-006, TERMS-024): "The business terms list when Ask Hava may end an account (a false certification or statement, misuse of homeowner details, discrimination, unpaid charges, a confirmed LIC-025 breach), say the business is emailed the reason, and say what survives (fees owed, the data-use limit, indemnity)." [inferred]
7. **Business liability floor** (business terms section 10): "Ask Hava's total liability to a business is the greater of the fees it paid in the 12 months before the claim or $100." [inferred]
8. **No promise of jobs** (business terms, WORD-030): "The business terms say 'Ask Hava doesn't promise any number of job cards, quotes, picks or jobs.'" [L1][C1]
9. **Change notice** (both terms): "A change to the business terms is emailed at least 14 days before it applies (a fee change still needs the business's OK); a change to the homeowner terms applies only to requests sent after its effective date, and the version in effect at the pick governs that request." [inferred from L3]
10. **Breach notice method and paths** (OPS-020, incident plan section 5): "For a breach covered by A.R.S. 18-552, notice goes by email or letter (or a live call), never by text alone; the plan holds the substitute-notice path (letter to the Attorney General and a 45-day website posting), the law-enforcement delay, and the Attorney General's online form." [L10][L11]
11. **Copy and verified requests** (PRIV-011, PRIV-021): "The homeowner portal offers 'Download my data', a file of the person's own requests, quotes, messages and consent records still held; a delete or copy request by email or form is acted on only after a Verify code to the number on file; each is answered within 30 days." [inferred from L1, L2]
12. **Terms-match-code scope** (TERMS-005, T114): "The terms-match-code test also checks every number in the privacy policy's retention section against PRIV-007 to PRIV-010 and CON-022, and every fee, tier and window number in the business terms and BIZ-030 against the catalog settings." [inferred]
13. **EIN** (PRIV-012): "Sign-up identifies a business by ROC number or city license only and never asks for an EIN; if an EIN is ever kept, it is a vault field and the incident plan lists it as a specified data element." [L9]
14. **Feedback moderation** (JOB-017): "Ticked feedback is left off askhava.com only for another person's personal details, abusive or discriminatory content, content believed fake, or content unrelated to the service, each removal logged with its reason; no feedback from the Ask Hava owner, a relative or a business is shown; and no page says the shown feedback is all feedback received." [L4]
15. **Reading grade for legal drafts** (TERMS-028, WORD-029): "`check_legal_docs.py` reports the Flesch-Kincaid grade of each section and fails a homeowner document section above grade 9 or a business document section above grade 10, sentences fixed by a requirement excepted." [L24, grades inferred]
16. **Recruiting emails** (business email notice terms section 6): "A recruiting email says in its first line that it is an advertisement for Ask Hava, carries the postal address and a one-step opt-out, and honors it at once (never later than 10 business days)." [L16]
17. **Acceptance record retention** (TERMS-016, PRIV-010): "Terms-acceptance records and every published terms version are kept 7 years after the last pick or charge made under them, with IDs only and no contact fields." [L27, period inferred]
18. **ADOR request contents** (D-444, FEE-046): "The ADOR private ruling request is filed in the LLC's name, describes the fee in the TERMS-027 words, attaches the business terms and a Confirm-page screenshot, discloses no audit or other filing, and FEE-046's set-aside stays until the answer is filed." [L22][L23]

## 17. Review checklist for legal-document tasks

- [ ] The document opens with the draft note and version line (TERMS-028); it is not published without Casey's go (WEB-013, BUILD_BRIEF.md:61).
- [ ] Ask Hava is described only in the WORD-023 words; WORD-002 appears in the required form; no WORD-020, WORD-022, WORD-026 or WORD-030 phrase appears [L2][L5][L8].
- [ ] Every number in the document matches the code or the catalog setting (TERMS-005, section 8).
- [ ] Nothing in the terms contradicts what a page shows; a disclaimer is never the only place a key limit appears [L2][L3].
- [ ] Assent: the acceptance line, link and record match PICK-016 and TERMS-016; text picks are covered [L15].
- [ ] No pre-ticked box; opt-outs as easy as opt-ins [L3].
- [ ] Liability cap carve-outs present; personal injury never waived [L25].
- [ ] Privacy promises match the code: who sees what (TERMS-025), retention, deletion, processors (PRIV-027).
- [ ] Breach plan: 45 days, notice methods, over-1,000 recipients, AG form [L10][L11].
- [ ] Commercial emails meet CAN-SPAM; transactional emails stay transactional [L16][F1].
- [ ] Nothing contradicts Stripe's or Twilio's terms [L20][L21].
- [ ] Examples use fake data only (928 555 01xx, example.com) (PRIV-019).
- [ ] Sentences short and plain; spec-fixed sentences unchanged (WORD-029).

## 18. Sources and why they are authoritative

Statutes and regulations are the law itself (Cornell LII and azleg.gov reproduce the official text); agency pages state how the agency applies its law; the Ninth Circuit opinion binds federal courts in Arizona; vendor terms bind Ask Hava as a customer.

- [L1] 15 U.S.C. 45 (FTC Act section 5): https://www.law.cornell.edu/uscode/text/15/45
- [L2] FTC Policy Statement on Deception (1983): https://www.ftc.gov/system/files/documents/public_statements/410531/831014deceptionstmt.pdf
- [L3] FTC staff report, Bringing Dark Patterns to Light (September 2022): https://www.ftc.gov/system/files/ftc_gov/pdf/P214800%20Dark%20Patterns%20Report%209.14.2022%20-%20FINAL.pdf
- [L4] 16 CFR part 465, Consumer Reviews and Testimonials: https://www.law.cornell.edu/cfr/text/16/part-465 ; section 465.7: https://www.law.cornell.edu/cfr/text/16/465.7 (eCFR, https://www.ecfr.gov/current/title-16/chapter-I/subchapter-D/part-465, refused the fetch)
- [L5] A.R.S. 44-1521 (definitions): https://www.azleg.gov/ars/44/01521.htm ; A.R.S. 44-1522: https://www.azleg.gov/ars/44/01522.htm
- [L6] A.R.S. 32-1121 (exemptions, including A(14)): https://www.azleg.gov/ars/32/01121.htm
- [L7] A.R.S. 32-1151: https://www.azleg.gov/ars/32/01151.htm
- [L8] A.R.S. 32-1165: https://www.azleg.gov/ars/32/01165.htm
- [L9] A.R.S. 18-551 (definitions): https://www.azleg.gov/ars/18/00551.htm
- [L10] A.R.S. 18-552 (notification): https://www.azleg.gov/ars/18/00552.htm
- [L11] Arizona Attorney General, data breach notification form: https://www.azag.gov/consumer/data-breach/notification-form
- [L12] 16 CFR 314.4 (Safeguards Rule, including (j)): https://www.law.cornell.edu/cfr/text/16/314.4
- [L13] 15 U.S.C. 7001 (ESIGN): https://www.law.cornell.edu/uscode/text/15/7001
- [L14] A.R.S. 44-7007: https://www.azleg.gov/ars/44/07007.htm ; A.R.S. 44-7012: https://www.azleg.gov/ars/44/07012.htm
- [L15] Berman v. Freedom Financial Network, No. 20-16900 (9th Cir. Apr. 5, 2022): https://www.govinfo.gov/content/pkg/USCOURTS-ca9-20-16900/pdf/USCOURTS-ca9-20-16900-0.pdf
- [L16] 15 U.S.C. 7704 (CAN-SPAM): https://www.law.cornell.edu/uscode/text/15/7704 ; FTC compliance guide: https://www.ftc.gov/business-guidance/resources/can-spam-act-compliance-guide-business
- [L17] FTC, Complying with COPPA: Frequently Asked Questions: https://ftc.gov/tips-advice/business-center/guidance/complying-coppa-frequently-asked-questions
- [L18] DOJ, Guidance on Web Accessibility and the ADA: https://www.ada.gov/resources/web-guidance/
- [L19] DOJ, Fact sheet on the 2024 Title II web and mobile rule: https://www.ada.gov/resources/2024-03-08-web-rule/
- [L20] Stripe Services Agreement: https://stripe.com/legal/ssa (first 100,000 characters read)
- [L21] Twilio Messaging Policy: https://www.twilio.com/en-us/legal/messaging-policy ; Twilio Acceptable Use Policy: https://www.twilio.com/en-us/legal/aup
- [L22] A.R.S. 42-2101 (private taxpayer rulings): https://azleg.gov/ars/42/02101.htm
- [L23] ADOR Letter Ruling LR21-003: https://www.AZDOR.gov/sites/default/files/2023-03/RULINGS_TPT_2021_lr21-003.pdf
- [L24] Digital.gov, Plain Writing Act requirements: https://digital.gov/resources/plain-writing-act
- [L25] Arizona Constitution article 18 section 6: https://www.azleg.gov/const/18/6.htm
- [L26] A.R.S. 22-503 (small claims limit): https://www.azleg.gov/ars/22/00503.htm
- [L27] A.R.S. 12-548 (six-year limit, written contract): https://www.azleg.gov/ars/12/00548.htm

Other brains cited: [C1][C2] `contractor-lead-experience.md` section 9; [F1] `email-deliverability.md` sources; [S10] `sms-compliance.md` sources.

Not legal or tax advice; no lawyer or CPA reviewed this file (D-367).
