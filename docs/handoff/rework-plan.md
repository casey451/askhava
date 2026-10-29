# Ask Hava rework plan: "as well packaged as possible" (fix round 14)

**For:** Casey · **Date:** October 28, 2026 · **Status:** applied in fix round 14 (D-429 to D-449), Casey's four answers taken as recommended. The build stays on hold at Gate P.

**Goal, in Casey's words:** the job going to the business, and the quotes coming back, should be "as well packaged as possible", so homeowners feel the service knows exactly what they need, and businesses get a pre-packaged job they'd otherwise pay a well-versed receptionist to gather.

**New inputs used:** the Grok trade knowledge base (37 trades, 309 job types, 370 test messages, photo playbook) and the ChatGPT sales-tax report (Oct 28).

---

## 1. Homeowner

| # | Change | Replaces |
|---|---|---|
| H1 | **One ordered intake script** per path (regular, urgent, fire-truck emergency, night, web chat), written out word for word in `intake.md` | G6 |
| H2 | **Expert questions per job type** from the Grok base: the sizing and setup questions a pro asks (bathroom size, system type, pool size), highest price impact first, 3 to 8, "Not sure" always allowed. AI may reword a question to fit what she already said and skip what she already answered | INT-039, G20, G13 |
| H3 | **Photos are part of intake**: asked for by job type (for example "the label on the outdoor unit"), 2 to 3 at a time, by picture text or an upload link. Checked for quality; one re-ask at most. No photos is fine and the request still goes | G2, PRIV-013 to 015, CARD-013 |
| H4 | **Inspiration images** (Pinterest screenshots) read into the project file: layout, style, finishes | new |
| H5 | **Concept image for remodels only**, labeled "AI-generated, not exact, may not be feasible", with "Is this the kind of thing you want?" and up to 3 rounds of changes. Built from the written project file, never from her photos | new |
| H6 | **She approves the actual job card**, not a 3-answer summary. Short text plus a link to the card on her portal page. Same 2-hour auto-send | INT-041, G3 |
| H7 | **Coverage told at once**: "We're sending this to the 5 pool cleaners on Ask Hava" or "We don't cover that kind of job yet. We've noted it." The no-coverage case emails the owner inbox for recruiting | TXT-032, INT-017 |
| H8 | **Emergency only for fire-truck situations** (see question 4). Flood goes straight to plumbers who take urgent jobs; AC out goes straight to HVAC businesses that take urgent jobs. **No heat-safety text, no 211 line, no health advice.** The site says plainly Ask Hava is not an emergency service and doesn't judge what needs one | INT-031, INT-034 water lines, INT-036 water and 211 entries |
| H9 | **Own-or-rent question and renter text removed.** Research found no law requiring a quote service to ask; landlord authority and lien exposure are the hiring business's concern (see section 6) | INT-028, TXT-028, CARD-011 |
| H10 | **Short texts plus a personal portal.** Texts carry one line and the link. The portal holds every quote, business info, license numbers, Google and Yelp review links, and her card | TXT-016, TXT-031, PICK-041 |
| H11 | **"Free for you" said plainly**: "Free for you. The business you pick pays Ask Hava a set introduction fee." at the top of her portal and in the first-quote text | G1, CON-003, PICK-016 |
| H12 | **No scheduling.** Removed: "Who will let the business in?", the call-window question and "has until 7:30 p.m. to confirm" lines in her texts. After the business confirms, it reaches her and sets the time | PICK-025, PICK-018 window, PICK-031, PICK-045, TXT-030 TIMES |
| H13 | **Human-sounding copy** throughout (sharp, experienced receptionist), with the not-a-contractor message still in every first reply, portal page and card | voice.md, every template |
| H14 | Clean-ups: email address confirmed by one tap before codes go there (G7); a "2 YES" with no Terms line in front gets the line first (G8); one Spanish "we continue in English for now" line (G15); one verb, **Pick** (G19) | G7, G8, G15, G19 |

## 2. Business

| # | Change | Replaces |
|---|---|---|
| B1 | **Uniform quotes per job type.** Each job type names its one quote format so quotes line up: price range (remodels, replacements), monthly price (pool cleaning, pest service), tiers (detailing, cleaning packages), firm price (small fixed jobs), diagnostic visit fee (repairs that need a look). Fee basis follows the format; a "site visit" can't stand in for a range on a big job | PICK-002, FEE-004, G4, G17 |
| B2 | **Notes are screened**: job-related only. Code blocks phones, links, emails, "call me", "text me", off-platform pay offers; AI flags notes that steer the homeowner away from Ask Hava. Flagged notes are sent back to the business to fix | PICK-007 |
| B3 | **Sign-up collects Google Business URL, Yelp URL and license numbers**; these show on every quote | BIZ-049, PICK-041 |
| B4 | **The job card is the packaged job**: the AI-written, homeowner-approved card, her answers, screened photos, project file and concept image if any. One job page on the portal, defined | CARD-007, CARD-009, G3 |
| B5 | **"Can't take it" button** on a pick: no fee, the backup takes over at once (G12). Edit or withdraw a quote before any pick (G14). Leaving Ask Hava defined (G16) | G12, G14, G16 |
| B6 | **Pest attestation wired through**: city license list match plus the attestation; cards say "not independently verified"; the daily state check doesn't apply | G5 |
| B7 | **Fee wording** (sales-tax report): "introduction fee", "accept this introduction", "you were picked by the homeowner". Never "lead", "unlock" or "customer data". Receipts say "Ask Hava introduction service, job <n>" | WORD rules, FEE-031 |

## 3. Owner (Casey)

| # | Change | Replaces |
|---|---|---|
| O1 | **No-coverage email** to the owner inbox (for your AI assistant) with trade, town and count; weekly roll-up. Recruiting stays email or in person | INT-017, OPS-030 |
| O2 | **Sales-tax set-aside 8.6%**, not 7.6% (Mohave County is 1%, per the October 2026 ADOR table) | FEE-046 |
| O3 | Owner time re-estimated by counting message types (G11); OPS-033 windows fixed (G21); emergency-call button or email fallback (G10); "default pending Casey" tags removed (G18) | G10, G11, G18, G21 |
| O4 | New scorecard lines: share of cards approved without edits, photo rate, notes sent back | OPS-039 |

## 4. AI

| # | Change | Replaces |
|---|---|---|
| A1 | AI now **writes**: follow-up questions, the job card, the project file, the concept image prompt. Always inside fixed limits: it never diagnoses, prices, judges safety, names a cause or recommends a business. Every AI-written card is approved by the homeowner before it goes | AI-006, TXT-004, CARD-007, AI-010 |
| A2 | Code safety check still runs first on every message; AI never downgrades it | INT-002 (unchanged) |
| A3 | Quote summaries stay **off** (Ask Hava's own words about prices are the risky kind) | PICK-003 (unchanged) |
| A4 | Privacy policy and every AI description rewritten (the "No AI reads your messages today" line goes); Twilio question updated to match | AI-018, TERMS-008, Twilio question |
| A5 | Image provider for concept images chosen at the account checkpoint; receives only the written project file | new |

## 5. Gaps G1 to G23

| Gap | Outcome |
|---|---|
| G1 | H11 |
| G2 | H3 |
| G3 | H6, B4 |
| G4 | B1 (format per job type sets the fee basis) plus a site-visit-share scorecard line |
| G5 | B6 |
| G6 | H1 |
| G7, G8 | H14 |
| G9 | Test journeys J1 and J2 rewritten to the new script |
| G10, G11 | O3 |
| G12 | B5 |
| G13 | Gone with H8's narrower list; "smell" options ask "gas or rotten egg?" and a yes sends the fire-truck text |
| G14, G16 | B5 |
| G15 | H14 |
| G17 | B1 |
| G18, G21 | O3 |
| G19 | H14 |
| G20 | H2 |
| G22 | Stripe question still to send (unchanged); recruiting walks each business through the pick-alert test |
| G23 | Closed by your Oct 28 call: insurance later (D-426) |

## 6. Research done before asking

- **Own or rent:** no Arizona or federal rule requires a quote service to ask. Landlord authority and lien risk fall on the business that does the work (Arizona courts have let a tenant's contractor lien the owner's property: *Wang Electric v. Smoke Tree Resort*, 2012). Lead-paint renovation rules put the duty on the renovator, not on whoever passed the request. So it goes.
- **Sales tax (ChatGPT):** probably no Arizona TPT on the fee (about 80% confidence), if the fee is an introduction fee, not a sale of a lead or data. The backup reserve should be 8.6%. Its two big suggestions are questions 2 and 3 below.

## 7. Decisions for Casey (one at a time)

1. Photos before a zero-retention agreement
2. When the business gets her first name and address (sales-tax wording)
3. ADOR private ruling on the fee
4. Which situations get the "call 911" text

Everything else above is decided by your direction and gets logged with your words.
