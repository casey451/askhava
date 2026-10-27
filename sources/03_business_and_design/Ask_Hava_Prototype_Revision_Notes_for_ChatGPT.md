# Ask Hava "Clearwater Modern" prototype: revision notes (round 3)

**From:** Casey Solomon, with Claude · **Date:** October 27, 2026

I'm keeping the Clearwater Modern direction. The build quality is excellent. Below are the changes for the final round: mostly decisions made after the brief, plus a few fixes found in testing. Please keep everything else as it is and update README, DESIGN, CONTENT, ACCESSIBILITY, QA and CHANGELOG to match.

---

## What we tested on our side (so you don't have to repeat it)

- **axe-core 4.10** (WCAG 2.0/2.1/2.2 A and AA rules) on all 34 pages, at 1280px and at 320px width: **no violations at 1280px.** At 320px there is one issue on three pages (item 7 below). No horizontal scrolling at 320px on any page.
- **Browser click-through** (Chromium, 390px phone size):
  - the request flow, the quote page (Choose → code 123456 → confirmed, with focus moving to the result heading), business sign-up, the business portal, the homeowner portal and the owner page all work;
  - no JavaScript errors on any page.
- **Still not done:** screen-reader checks (VoiceOver or NVDA), 200% zoom screenshots and Lighthouse. Keep those listed as "not yet run" in QA.md.

---

## Changes

### 1. The old Ask Hava directory is gone (decided after the brief)

Ask Hava is now only the quote service. The old directory, events and chat pages are being deleted, so:

- Remove **"Explore Havasu ↗"** and **"Events ↗"** from the top navigation on every page, English and Spanish.
- Remove the banner text **"Quotes are a new Ask Hava feature."** Keep "Built for Lake Havasu" and the language switch.
- Remove the home-page section **"More than quotes. All of Havasu."** (Explore the directory / See local events / Ask the concierge).
- Remove the footer **"Explore"** column links to askhava.com/categories, /events and /chat (70 pages link to them). Replace the column with **"Homeowners":** How it works, Services, Get quotes, Help.
- Footer tagline: change "Local answers. Local quotes. Your choice." to **"Local quotes. Your choice."**
- The eyebrow "Your Lake Havasu concierge" can stay. "Concierge" now means Hava gathers quotes for you, nothing more.

### 2. Pricing rules (decided after the brief)

On pricing.html, businesses.html, business-signup.html, the business portal and CONTENT.md:

- **Ongoing services count as four months of the price** (was two). Update the example: "a $150-a-month pool account counts as $600, so the fee is $75." Move that example from the $25 row to the $75 row. Keep a small one-off repair as the $25 example.
- **Free first win: only for businesses that join in the first 90 days after launch.** Replace "Your first win is free" with **"Launch offer: your first win is free when you join in the first 90 days."**
- Everything else stays: $25 under $500, $75 from $500 to $1,500, $199 over $1,500, midpoints for ranges, the set tier for trip-fee quotes, the automatic refund rule, repeat work free.

### 3. Quote cards

- **Handyman quotes** must show: **"Not a licensed contractor. Only for jobs under $1,000 total with no permit."** (No page shows this today.)
- **Businesses in categories that need no state license** (cleaning, detailing, junk removal and so on) show **"Insurance on file: Yes"** or **"Insurance on file: No"**. This is shown, not required.
- Licensed trades keep the "ROC #… shows active on the state website as of [date]" line and the "Check it yourself" link.
- Keep arrival order and "Independent business, not affiliated with Ask Hava."

### 4. Hava's voice

The hero demo bubble says **"I can help gather local quotes. You'll compare them and choose."** That reads as if Hava is a person. Change it to the service voice, for example: **"Ask Hava: got it. Local businesses can send quotes, and you compare them and choose."** Check CONTENT.md for any other first-person "I" lines from Hava.

### 5. Spanish consent text

The Spanish request page and es/sms-terms.html show the consent prompt in English only. Add a **Spanish draft** above the English one, marked "Borrador — pendiente de revisión por un hablante nativo." Keep the English text too. Spanish draft to start from:

> "Hola, somos Ask Hava. Reunimos cotizaciones para que no tengas que hacer las llamadas. Es gratis para ti, y el negocio que elijas nos paga una pequeña tarifa. La frecuencia de mensajes varía según la solicitud. Pueden aplicarse tarifas de mensajes y datos. Responde STOP para cancelar, HELP para ayuda. Términos y privacidad: askhava.com/terms. Responde OK para empezar."

### 6. Links inside text messages stay on askhava.com

If any sample text message, or the complaint reply copy, links to an outside site (for example the Arizona ROC complaint page), point it to an **askhava.com page that links onward** (for example askhava.com/help/roc-complaint). Website pages may still link out directly.

### 7. Accessibility fix at 320px

axe-core flags **scrollable-region-focusable** (serious) at 320px on **app/owner.html, businesses.html and styleguide.html**. These are wide tables in horizontal scroll containers. Give each scroll container `tabindex="0"`, `role="region"` and an `aria-label` (for example "Lead price comparison table, scrolls sideways") so keyboard users can scroll it. Re-run your static checks after the fix.

### 8. Two small additions

- **A status-banner component** in the style guide and on the home page (hidden by default): "Texting is temporarily unavailable. You can still use the chat on this site and your quote page." It's used if the texting provider ever has an outage.
- **The help page:** add a short answer to "Is Hava a person?": "Ask Hava is a local service. Our texts are automated, and a real person reviews anything flagged."

---

## Deliver

One updated .zip with the same structure and filenames, plus an updated CHANGELOG entry listing these eight items. Please don't change the visual design.
