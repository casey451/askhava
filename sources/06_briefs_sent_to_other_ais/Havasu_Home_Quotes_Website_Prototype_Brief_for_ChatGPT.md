# Havasu Home Quotes: website prototype brief

**For:** ChatGPT (designer and front-end developer)
**From:** Casey Solomon, owner
**Date:** October 26, 2026

---

## 0. What I want from you, in short

Design and build a **working, clickable website prototype** for my business, Havasu Home Quotes. It must:

- look **extremely premium**;
- be **easy for anyone to use**, including older homeowners on a phone;
- meet **WCAG 2.2 Level AA** accessibility (Section 7).

**I want to keep it if I like it.** Deliver real, clean files I can download, host for free, hand to a developer later, and use as the reference for the real build. Don't give me a mock-up picture or a page that only lives inside this chat.

Work in **three rounds**, and wait for my answer after each one:

1. **Three design directions.** For each, give a name, a one-paragraph mood description, the color palette with contrast ratios, the type pairing, and a working home page (mobile and desktop) I can open. I pick one, or mix them.
2. **The full prototype** in the chosen direction: every page and flow in Sections 4 and 5, delivered as one **downloadable .zip** (Section 8).
3. **Polish:** I review, you revise, then you send a final .zip with all the docs in Section 8 updated.

---

## 1. The business

Residents of Lake Havasu City, Arizona (later Bullhead City, Fort Mohave and Kingman) **text one local number or use web chat** to say what service they need. That can be a repair (plumbing, AC, electrical), a recurring service (pool cleaning, yard care, house cleaning, pest control) or a one-time job (car or boat detailing, junk removal, moving, painting).

1. We send an **anonymous job card** to every qualified local business that opted in to that kind of job. Businesses send quotes.
2. The homeowner sees **every quote** on one private page. Each quote shows the price, the business's own wording, review links, and a state license link where a license is required.
3. The homeowner picks a quote by entering a **code texted to their own phone.**
4. The business taps **Confirm** and pays us a small fee. Only then does it get the homeowner's first name, address and photos.
5. The business calls the homeowner through **our masked number**, so the homeowner's phone number is never shared.

**Homeowners never pay.** No spam and no marketing texts. STOP works instantly.

**Positioning:** "We gather quotes; you choose. We're just compiling the information." We are not a contractor, we don't do the work, and we don't recommend one business over another. Quotes are shown in the order they arrived, never sorted by price, and no business can pay to rank higher.

**Audiences:**

1. **Homeowners and property owners.** Many are retirees, part-time residents and vacation-rental owners, and about half of Havasu residents are over 55. Most use a phone. Some prefer Spanish.
2. **Local service businesses.** Busy owner-operators who quote from a truck. They need to see quickly that this is a better deal than paying for leads.
3. **Me (the owner).** I need a simple owner dashboard.

---

## 2. Brand and premium feel

- **Name:** Havasu Home Quotes (working name). **Domain placeholder:** hhq.example. **Phone placeholder:** (928) 555-0100. Use these placeholders; don't invent real ones.
- **Feel:** calm, trustworthy, quietly luxurious, local. Think high-end hospitality or a private bank, not a lead-generation site. Restraint over decoration.
- **Place:** Lake Havasu is desert and water: deep lake blues, warm sandstone and canyon tones, bright sun, clean whites. Use this as inspiration, but stay tasteful and avoid clichés.
- **Typography:** a refined pairing from Google Fonts. Body text at least 18px on mobile, with generous line height and a strong hierarchy.
- **Imagery:** use original SVG illustrations, abstract shapes or clearly marked image placeholders. **No stock photos of real people, no scraped images, and no other companies' logos or trademarks.**
- **Motion:** subtle and purposeful. It must respect "reduce motion" settings, and nothing may flash.
- **Performance:** fast on a mid-range phone over cellular. Aim for a Lighthouse performance score of 90 or higher. No heavy frameworks unless they earn their place.
- **Voice:** plain, warm, short sentences, no jargon or hype. Sentence case. Never "best," "#1," "guaranteed," "vetted" or "trusted pros."

---

## 3. Honesty rules (non-negotiable)

1. **No fake reviews, testimonials, star ratings, user counts, "as seen in" logos or badges.** Where social proof would go, use a clearly labeled placeholder, such as "[Real reviews will appear here after launch]".
2. **Sample businesses** in the prototype must be obviously fictional (for example "Sample Plumbing Co."). Label them "Sample." Use obviously fake license numbers such as "ROC 000000 (sample)."
3. **License wording:** "ROC #000000 shows active on the state website as of 10/26/2026," with a "Check it yourself" link to the Arizona ROC search: https://azroc.my.site.com/AZRoc/s/contractor-search
4. Every quote card must say "**Independent business, not affiliated with Havasu Home Quotes.**"
5. **Handyman quotes** say: "Not a licensed contractor. Only for jobs under $1,000 total with no permit."
6. **Don't claim** we check insurance, quality or workmanship. Say only what we actually check: license status where a license is required, and identity.
7. **No dark patterns.** No fake urgency, no pre-checked boxes, no hidden opt-outs and no confirm-shaming.
8. **Footer of every page:** "Prototype — not a live service" in small text.

---

## 4. Pages

All pages are mobile-first and responsive. A Spanish version of every homeowner page must be reachable with an English/Español switch (Section 6).

**Public pages**

1. **Home (homeowners)**
   - Hero: "Text one number. Get every quote. Choose." Show the text number and a "Start online" button.
   - How it works, in 4 steps.
   - The promises: free, private number, no spam, every quote shown.
   - Popular services, and a short FAQ.
2. **How it works**
   - The full homeowner journey.
   - What we share and when: nothing until you pick and the business confirms, and your phone number is never shared.
   - What we don't do.
3. **Services**
   - Every category in Section 6.1, grouped, with a short line each.
   - Mark "Coming soon" for categories not open at launch.
4. **For businesses**
   - The pitch: pay only when a homeowner picks you and you confirm.
   - How job cards work, and the pricing (Section 6.2).
   - The comparison with other lead sources (Section 6.3), with sources.
   - "Join free" button.
5. **Pricing** (for businesses): the three tiers, the rules, and worked examples.
6. **About:** local, owner-run, what we are and aren't. Placeholder for my story.
7. **Help and FAQ**, for homeowners and for businesses.
8. **Contact:** help email, phone and a form. The form doesn't send anywhere in the prototype.
9. **Text message terms:** frequency, rates, STOP and HELP, and the exact consent wording (Section 6.4).
10. **Privacy policy (draft)** and **Terms (draft)**. Lay these out, but mark the text "**Draft — for attorney review**." Include the privacy clause in Section 6.4.
11. **Accessibility statement:** our WCAG 2.2 AA target, the date of the last review, known issues, and how to report a problem by email and phone with a response time.
12. **404 page.**

---

## 5. Clickable flows (front end only, fake data, nothing sent anywhere)

### 5.1 Homeowner: start a request online (web chat style)

1. **Consent prompt** with the exact wording in Section 6.4, an "OK, start" button, and links to the terms and privacy pages.
2. **Questions:** what's wrong, ZIP code, how soon, an optional photo upload, and whether they own or rent.
   - If they rent or won't be there: who will meet the business.
   - Language preference.
3. **Emergency branch.**
   - If the text mentions a gas smell, rotten eggs, sparks, smoke, burning or flooding, show the safety message with the reason. For example: "A rotten-egg smell can mean a gas leak. Leave the area, don't flip switches, and call 911 or the gas company."
   - Next, ask "Are you safe now?"
   - Then ask whether the repair is urgent. Explain that after-hours work usually costs more, and let them choose emergency or regular.
4. **Done screen:** "Local businesses have your job. We'll text you when the first quote arrives."

### 5.2 Homeowner: the private quote page (the most important screen)

1. Show the job summary, then **every quote** as a card in arrival order, with a note: "More quotes may still arrive."
2. Each quote card shows:
   - the business name and "Independent business, not affiliated with Havasu Home Quotes";
   - the **price type in large letters**: FIRM PRICE, PRICE RANGE, MONTHLY PRICE (for recurring), or TRIP FEE + ESTIMATE ON SITE. A trip-fee card adds "This is not a repair price."
   - the business's original wording, word for word;
   - what's included, the earliest arrival, and when the quote expires;
   - the license line and "Check it yourself" link;
   - "Read their reviews" link placeholders;
   - optional tags: "We speak Spanish" and "After-hours available";
   - a **Choose** button.
3. **Choose** opens the confirm step: "We'll text a code to your phone. Nothing is shared until you enter it." It has an accessible 6-digit code input. Enter 123456 to succeed.
4. **Success screen:** "Sample Plumbing Co. has 2 business hours to confirm. We'll text you who will call and from what number."
5. **Other actions:**
   - "Close this request" (with a confirmation step);
   - "Ask a question" (passed along anonymously);
   - "Report a problem."

### 5.3 Homeowner portal

- Sign in with phone number and code.
- **My jobs:** status chips (Collecting quotes, Quotes ready, Picked, Business confirmed, Contacted, Done, Closed, Cancelled).
- **Job detail** with a timeline.
- **Past jobs**, with "Request this business again."
- **My info:** properties, on-site contacts, language, texts on or off, and "Delete my data."

### 5.4 Business sign-up

1. Business name, then a **license number field**. Use 000000 to show a successful "found" state.
2. **License read-back:** "We found R-39 (Air conditioning and refrigeration). It covers AC repair, heating and system replacement. Which jobs do you want?" Show a checklist.
   - Adding a job type the license doesn't cover shows "That needs a different license."
   - An unlicensed category shows a "No state license required" path with a city business license check.
3. **Options:**
   - towns served;
   - after-hours emergencies;
   - "We speak Spanish";
   - "Skip tenant requests";
   - "Also send me jobs nobody else covers."
4. **Identity check screen:** "We'll send a code to the phone listed on the state record." Fallbacks: "Call me with the code instead," then "Email me," then "Mail me a letter."
5. **Terms and texting consent.** The consent box must be **unchecked by default**, with the disclosures in Section 6.4 and the fee schedule shown.
6. **Welcome screen:** "Your first win is free."

### 5.5 Business portal

- **Today:**
  - job cards waiting for a quote;
  - picks waiting for Confirm, shown as "Confirm by 4:15 p.m." (a clock time, not a ticking countdown);
  - homeowners to call.
- **Quote form:** choose a price type (firm, range, monthly, or trip fee + estimate); enter the amount, earliest day and how long the quote is good. Show the fee tier live ("If you're picked, the fee is $25").
- **Jobs** and **Billing:** charges, free first win, refunds, statements, and a payment method placeholder.
- **Settings:** pause, towns, checklist, staff phones.

### 5.6 Owner dashboard (light)

- Today, this week and this month: requests, picks, paid introductions, money in, refunds and net.
- A "Needs you" list.
- Pause switches: all, town, category, charging, texting.
- Coverage by category and town (green, yellow or red). Status must never rely on color alone.

---

## 6. Content to use

### 6.1 Service categories

| Group | Categories |
|---|---|
| Home repair | Plumbing; AC and heating; electrical; handyman; appliance repair; garage doors; roofing |
| Pool and water | Pool cleaning (weekly); pool repair; water softeners |
| Yard and outdoor | Landscape maintenance; desert landscaping (coming soon); irrigation (coming soon); pressure washing |
| Cleaning and property care | House cleaning; vacation-rental turnover cleaning; window cleaning; trash-can cleaning; dog waste pickup; pest control; junk removal |
| Interior and exterior | Painting; solar screens |
| Vehicles and boats | Car detailing; boat detailing; boat repair |
| Moving | Local movers |

### 6.2 Pricing for businesses

| If you're picked and confirm, and the chosen quote is | Fee |
|---|---|
| Under $500 | **$25** |
| $500 to $1,500 | **$75** |
| Over $1,500 | **$199** |

Rules to show:

- **Pay only when a homeowner picks you and you confirm.** Never for a lead you lose.
- **Ongoing services** (pool, yard, cleaning, pest) count as **2 months** of the price. A $150-a-month pool client counts as $300, so the fee is $25.
- **Price ranges** use the middle of the range. A **trip-fee quote** uses the set tier for that type of job.
- **Your first win is free.**
- **Automatic refund** if the homeowner can't be reached (2 call attempts at least 3 hours apart within 72 hours) or cancels before you talk.
- **Repeat work** from the same customer is never charged.
- No sign-up fee, no monthly fee, no contract.

### 6.3 Comparison with other lead sources (for the businesses page)

Show these as **published third-party estimates, as of October 2026,** with a visible link to each source. Don't round them up or add any we can't source.

| Trade | What one lead costs elsewhere (won or lost) | What it costs to win one customer elsewhere | Our fee, only when you win |
|---|---|---|---|
| HVAC | $51–$149 | about $288 | $25 repair · $199 replacement |
| Plumbing | $57–$183 | about $263 | $25 repair · $75–$199 bigger jobs |
| Electrical | $25–$80 | about $218 | $25 · $75 bigger jobs |
| Roofing | $162–$228 | about $369 | $75 repair · $199 replacement |
| Handyman | $8–$34 | about $77 | $25 |
| Landscaping | $10–$40 | about $89 | $25 per client |
| Pest control | $20–$150 | about $46–$68 | $25 per client |

Sources to link:

- https://themediacaptain.com/google-local-service-ad-statistics/
- https://pipelineon.com/blog/home-service-marketing-benchmarks-2026/
- https://www.webtonic.io/blog/pest-control-google-ads-statistics

Add a note: "Estimates from marketing-industry reports; your costs may differ."

### 6.4 Exact wording that must appear

- **Consent prompt (homeowner):** "Hi, this is Havasu Home Quotes. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: hhq.example/terms. Reply OK to start."
- **Business texting consent (unchecked box):** "I agree to receive job notices and account texts from Havasu Home Quotes at this number. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help." Link the terms and privacy pages.
- **Privacy clause:** "We do not share or sell mobile numbers or SMS opt-in data with third parties or affiliates for marketing. Your name and address go only to the one business you choose, after you confirm, to arrange the job you asked for. Your phone number is never shared."
- **Closing a request by text:** "Reply CLOSE 1042." Never "Reply CANCEL."

### 6.5 Spanish

- Every homeowner-facing page and flow gets a Spanish version, with an English/Español switch on every page.
- Set `lang="es"` correctly on Spanish pages and `lang="en"` on English ones.
- Mark the Spanish text "Borrador — pendiente de revisión por un hablante nativo" (draft, pending native-speaker review) in the content file.
- Business pages can stay in English for now.

---

## 7. Accessibility (must meet WCAG 2.2 Level AA)

**Why:** for private businesses, the ADA has no specific web rule yet. For state and local governments, the DOJ's Title II rule adopts WCAG 2.1 AA, and courts and plaintiffs treat WCAG 2.1 AA as the benchmark. WCAG 2.2 AA includes everything in 2.1 AA, so building to 2.2 AA covers both. **Do not use an accessibility overlay or widget.** The FTC fined one overlay vendor $1 million over false compliance claims, and overlays don't make a site compliant.

**Build it in, and document how each requirement is met:**

1. **Structure:**
   - semantic HTML, with one `h1` per page and headings in logical order;
   - landmarks: header, nav, main and footer;
   - a "Skip to main content" link;
   - a correct `lang` attribute on each page.
2. **Keyboard:**
   - everything works with the keyboard alone;
   - a visible, high-contrast focus indicator that is never hidden behind sticky headers (WCAG 2.4.11);
   - no keyboard traps;
   - a logical tab order.
3. **Color and contrast:**
   - body text at least 4.5:1;
   - large text and interface parts (borders, icons, focus rings) at least 3:1;
   - never color alone to show status, errors or tiers.
4. **Text and zoom:**
   - readable at 200% zoom;
   - the layout reflows at 320px width without sideways scrolling;
   - text spacing can be increased without breaking the layout.
5. **Touch targets:** at least 24×24 CSS pixels (WCAG 2.5.8). Aim for 44×44 on primary buttons.
6. **Forms:**
   - visible labels, not placeholder-only;
   - instructions before the field;
   - errors described in text, linked to the field and announced;
   - correct `autocomplete` values;
   - no re-entering information already given (WCAG 3.3.7).
7. **Sign-in and code entry:**
   - the 6-digit code field supports paste and `autocomplete="one-time-code"`;
   - no puzzles or memory tests (WCAG 3.3.8).
8. **Timing:**
   - no time limits on homeowners;
   - business deadlines are shown as clock times, with no auto-logout while someone is typing;
   - no auto-advancing carousels.
9. **Motion:** honor `prefers-reduced-motion`; nothing flashes more than 3 times a second.
10. **Images:**
    - meaningful alt text on informative images;
    - empty alt text on decorative ones;
    - SVG icons with accessible names or `aria-hidden`.
11. **Dynamic content:**
    - status messages (new quote arrived, code sent, saved) announced with live regions;
    - dialogs trap and return focus correctly and close with Escape.
12. **Consistent help:** the same help link and phone number in the same place on every page (WCAG 3.2.6).
13. **Plain language:** short sentences and common words. Aim for about a grade 7 reading level on homeowner pages.
14. **Testing:**
    - Run automated checks (axe-core and Lighthouse accessibility) and fix everything they find.
    - Then test by hand with the keyboard alone, at 200% zoom and at 320px width.
    - Check with a screen reader, VoiceOver on iPhone or NVDA on Windows, if you can.
    - Report what you actually ran, and don't claim a test you didn't run.

---

## 8. Deliverables (so I can keep and reuse it)

Deliver a single **.zip** containing:

```
havasu-home-quotes-prototype/
  index.html                 (home)
  how-it-works.html, services.html, businesses.html, pricing.html,
  about.html, help.html, contact.html, sms-terms.html, privacy.html,
  terms.html, accessibility.html, 404.html
  es/                        (Spanish versions of homeowner pages)
  app/                       (clickable flows: request, quote page, homeowner
                              portal, business sign-up, business portal, owner)
  assets/css/tokens.css      (all colors, type, spacing, radius, shadow as variables)
  assets/css/styles.css
  assets/js/                 (small, plain JavaScript; no build step required)
  assets/img/                (original SVGs only)
  styleguide.html            (every color with contrast ratio, type scale,
                              buttons, form fields, quote card, status chips,
                              alerts, in all states incl. focus and error)
  README.md                  (what's here, how to open it, how to host it free)
  DESIGN.md                  (design direction, tokens, components, do's/don'ts)
  CONTENT.md                 (every line of copy, English and Spanish, in one place)
  ACCESSIBILITY.md           (each WCAG 2.2 AA criterion: how it's met, how tested,
                              results, known issues)
  CHANGELOG.md
```

**Technical requirements:**

- **Plain HTML, CSS and a little JavaScript, with no build step.** It must open by double-clicking `index.html` and work offline, except for Google Fonts.
- No trackers, analytics, cookies or third-party scripts.
- Fake data lives in one file, `assets/js/sample-data.js`.
- Clean, commented code with consistent class names, so a developer can later turn it into the real app.
- **README hosting steps** must be non-technical and numbered:
  1. **Netlify Drop:** drag the folder onto app.netlify.com/drop.
  2. **GitHub Pages:** for a permanent copy with version history.

If you can't produce a .zip in this chat, give me every file in full, one code block per file with its path as the heading, and say so.

---

## 9. What "done" looks like

- [ ] Every page in Section 4 and every flow in Section 5 works on a phone and on a desktop.
- [ ] Spanish versions of the homeowner pages work, with the language switch.
- [ ] The style guide shows every component in every state.
- [ ] axe-core and Lighthouse report no accessibility violations. The keyboard, zoom and 320px checks pass. Any screen-reader check you ran is reported honestly.
- [ ] No fake reviews, counts, logos or real business names anywhere.
- [ ] The exact wording in Section 6.4 appears where required.
- [ ] Lighthouse performance is 90 or higher on mobile.
- [ ] README, DESIGN, CONTENT, ACCESSIBILITY and CHANGELOG are complete.

---

## 10. Sources for the accessibility requirement

- Promise Legal, "ADA Website Accessibility in 2026" (Aug 18, 2026): no Title III web rule yet; Title II adopts WCAG 2.1 AA; deadlines extended to 2027–2028; FTC order against an overlay vendor. https://blog.promise.legal/startup-central/ada-website-accessibility-compliance-2026/
- W3C, WCAG 2.2: https://www.w3.org/TR/WCAG22/
