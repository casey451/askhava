# Ask Hava: legal decisions record (for an optional attorney review)

**Owner:** Casey Solomon, organizing Ask Hava LLC (Arizona) · **Prepared:** October 27, 2026

**Status:** all 18 questions that were open after Grok's legal research have a **working answer that is built in now**, so nothing waits on outside approval. Each answer is the safest workable choice found in the research, and each has a fallback the code can switch to. These are research-based judgments, not legal advice. If Casey hires an attorney later, this is the list to review. Full detail is in `01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`, section 7.1; Grok's citations are in `04_reviews/Grok_Legal_Research_Findings.md`.

## The service in brief

- **The request:** a homeowner or tenant in Lake Havasu City texts one number or uses web chat on askhava.com to describe a job.
- **Job cards:** Ask Hava sends an anonymous job card to **every** opted-in local business that is licensed or otherwise eligible for that job type.
- **Quotes and the pick:** businesses quote in their own words. The homeowner sees every eligible quote on a private page, in arrival order and never ranked, and picks one with a code texted to their phone.
- **The fee:** the picked business confirms and pays a fixed introduction fee by job size: **$25** for jobs under $500, **$75** for $500–$1,500 and **$199** over $1,500. Ongoing services count as 4 months of their price. Homeowners never pay.
- **Contact details:** only after payment does the business get the homeowner's first name and address. It calls through a masked number.
- **What Ask Hava doesn't do:** it never performs, bids on, prices, supervises or guarantees work.

## Decisions to review

| # | Question | Decision in effect | Fallback ready |
|---|---|---|---|
| 1 | Is Ask Hava a contractor under A.R.S. 32-1101(A)(3)(a)? | No. Quotes are the businesses' own offers, and Ask Hava never prices, bids, schedules, supervises or guarantees work. Refusals are worded as eligibility rules. | — |
| 2 | Does the size-tiered fee look like a commission? | Kept as a fixed, published introduction fee by job size. It is never a percentage, is charged at confirmation, and isn't adjusted to the final bill. | Flat fee per job type (a settings switch) |
| 3 | Does advertising pest-control quotes trigger A.R.S. 3-3615(A)? | Pest control is available but never advertised. Only Dept of Agriculture licensees get the cards. | Remove the category |
| 4 | Licenses for garage doors, dryer vents, holiday lights, freestanding appliances and weekly pool cleaning | Garage doors are off at launch. Each of the others is split into no-license, handyman-under-$1,000 and licensed parts (section 7.1, item 4). | One row each in the category list |
| 5 | Platform liability for handyman jobs (split jobs, the $1,000 cap) | Split-job check, the cap on construction-type work, and the disclosure line. Handymen also certify each quote. | — |
| 6 | Is this "third-party lead generation" under Twilio and carrier rules? | No, on Claude's reading of Twilio's rules. Register once the full site is live, using the campaign wording in section 7. | Web-and-email mode |
| 7 | Do job-card texts to businesses need written consent? | Collected anyway, through a sign-up checkbox (E-SIGN, stored version). | — |
| 8 | Is "Reply OK" enforceable acceptance of the homeowner terms? | Two steps: the text OK plus "By choosing, you agree" on the quote page. No arbitration clause or class waiver. | — |
| 9 | Section 230 and AI-formatted quotes | The original wording is always shown, and the AI adds nothing. | — |
| 10 | Liability for the safety text | Generic wording plus "Ask Hava is not an emergency service." | — |
| 11 | Negligent-referral liability | States exactly what's checked; disclaimers; liability limited to fees paid; general liability and tech E&O insurance. | — |
| 12 | Arizona TPT and city tax on fees | Get a TPT license; fees are tax-included; a tax preparer confirms at the first filing. | — |
| 13 | Does the city trash franchise bar junk haulers? | Junk removal is off until city code ch. 8.04 is read. Estate cleanouts and donation pickup stay on. | Switch on after the check |
| 14 | Commercial-purpose rules (A.R.S. 39-121.03) for the ROC and city lists | Used for verification only, never recruiting. File a statement if the ROC terms require one. | — |
| 15 | Tenant flag | "Landlord approval may be needed"; the "skip tenant requests" option is dropped. | — |
| 16 | Card authorization for merchant-initiated charges | The drafted wording, a Stripe SetupIntent, the stored agreement, and a receipt for every charge. | — |
| 17 | Statutory agent | A commercial registered agent with a Maricopa County address, so no newspaper notice is needed. | — |
| 18 | Trademark | USPTO search before any brand spending; file in Class 35 after the LLC exists. | Rebrand before launch |

## Documents an attorney would review (drafted during the build)

1. Homeowner terms and privacy policy.
2. SMS terms and the Twilio campaign description.
3. Business terms: the card authorization, refund rules, non-discrimination clause, independent-business statement and the handyman certification.
4. The job-type-to-license table for the launch categories.
5. Site and ad copy for licensed trades.
