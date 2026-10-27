# Spec: Quotes, quote page and pick

Prefix: `PICK` · Modules: `quotes`, `picks`, `portals` · Format and checks: [index.md](index.md)

- **PICK-001** Businesses may quote for 24 hours after the job card is sent, counted every day of the week, and no quote is accepted once a pick is confirmed (D-011). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:83`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:66`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:85`.
- **PICK-002** A quote stores its type (firm, range or trip fee), amount, high amount, original wording, AI-read flag, business-confirmed flag, earliest arrival and expiry. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:92`.
- **PICK-003** The business's original wording is always stored and shown next to any AI-formatted summary. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:200`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:92`.
- **PICK-004** An AI summary that adds a price, scope, warranty or other claim not in the original wording is rejected and not shown. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:200`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:245`.
- **PICK-005** A business can view its quote card exactly as homeowners see it. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:245`.
- **PICK-006** A quote the AI reads with less than 80% confidence is sent back to the business to confirm before it is shown. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:192`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:292`.
- **PICK-007** A quote containing a phone number, a link, spelled-out numbers or "text me / call me" wording bounces back to the business. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:292`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:343`.
- **PICK-008** The quote page shows every eligible quote, numbered in arrival order, and a quote's number never changes. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:123`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:189`.
- **PICK-009** The quote page keeps adding quotes as they arrive, each with a Choose button. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:98`.
- **PICK-010** The quote page link carries an unguessable 128-bit token, the page is hidden from search engines, and it shows no names, phone numbers or addresses. Source: `BUILD_BRIEF.md:310`, `BUILD_BRIEF.md:311`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:427`.
- **PICK-011** Quote-page tokens stop working 30 days after the job closes. Source: `BUILD_BRIEF.md:314`.
- **PICK-012** The homeowner can pick any quote from the first quote onward. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:82`.
- **PICK-013** Picking on the page requires a 6-digit Twilio Verify code sent to the homeowner's phone, valid 10 minutes, with 5 tries and then a 15-minute cooldown (D-015). Source: `BUILD_BRIEF.md:312`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:190`.
- **PICK-014** The code field allows paste and one-time-code autofill. Source: `BUILD_BRIEF.md:313`.
- **PICK-015** A text of "<number> YES" (for example "2 YES" or "2 yes please") from the homeowner's own phone picks that quote. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:13`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:220`.
- **PICK-016** The quote page shows "By choosing, you agree to the Terms" with a link before any pick. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:213`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:244`.
- **PICK-017** The pick has a review step showing exactly what happens next, with "Go back" before the final confirm. Source: `BUILD_BRIEF.md:406`.
- **PICK-018** At the pick the homeowner chooses how the business reaches them: a call through the masked number or an email address they choose to share. Source: `BUILD_BRIEF.md:408`.
- **PICK-019** After a pick, the business has 2 business hours to tap Confirm, and without a Confirm the job returns to Quotes ready. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:174`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:221`.
- **PICK-020** The business's license and account status are re-checked at the pick and again at Confirm, and an ineligible business can't confirm. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:222`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:65`.
- **PICK-021** The business sees the fee before tapping Confirm, on a review step with "Go back". Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:343`, `BUILD_BRIEF.md:406`.
- **PICK-022** The homeowner's first name, address and full photos are released, and the masked call line opened, only after Stripe confirms payment or a free win is recorded. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`, `BUILD_BRIEF.md:151`.
- **PICK-023** The homeowner's phone number is never given to the business. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`, `BUILD_BRIEF.md:410`.
- **PICK-024** The details text prints the homeowner's first name once. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:124`.
- **PICK-025** After a pick on a rental property, one text asks the homeowner who will meet the business. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:614`.
- **PICK-026** Every business that quoted gets "This job is no longer active." whenever the job ends, including any business that quoted after the pick. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:85`.
- **PICK-027** A business that asks about a live pick gets a short reply with the Confirm link and what happens next. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:90`.
- **PICK-028** Quote, pick and quote-page link IDs are derived from what they identify plus a server secret. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:109`.
