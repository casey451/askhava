# Laftly design spec: Daylight and evergreen (V6)

**Status:** chosen by Casey, November 12, 2026 ("This is a guide. You can make it better."), decision D-3331, which supersedes D-451 (Plaster and brass). The product was called "Ask Hava"; Casey renamed it Laftly on November 12, 2026, and a design pass the same day took V6 further (section 11). Every page, portal screen and email follows this file (WEB-027). Wording, rules and numbers still come only from `docs/spec/` and `docs/copy/site-copy.md`; this file decides how things look. WEB-001, WEB-027, TD-WEB-11 and the site tasks T101, T166 and T216 point here (D-3331); section 10 maps each old Plaster-and-brass term to its V6 rule.

**Mock-ups to build from** (`docs/design/mockups/`, rendered with Playwright from the reference at device scale 2):
- `v6-home-390-first-screen.png`: home page, 390 by 844, the first screen
- `v6-home-390-full.png`: home page, 390 px, whole page
- `v6-home-1440-first-screen.png`: home page, 1440 by 900, the first screen
- `v6-home-1440-full.png`: home page, 1440 px, whole page
- `v6-quote-page-390-full.png`: her quote page, 390 px, whole page

The HTML and CSS behind them are in `docs/design/reference/` (`home.html`, `quote_page.html`, `laftly-v6.css`; static, no JavaScript; a visual reference only, the build writes its own templates). The photo is in `docs/design/photos/`, the mark in `docs/design/brand/`, the fonts in `docs/design/fonts/`.

## 1. The idea: a calm room in daylight

- **The page** is warm ivory, like a well-lit room. Headlines, navigation and plain explanation sit on it.
- **The card** is paper: white with a thin grey-green edge and one soft shadow under it. Everything a person reads closely or decides on sits on a card: quote cards, the job card, the pick review step, forms, codes, receipts. Cards are where contrast is highest.
- **Evergreen** is the one action color: the primary button, links, the step numbers, the kicker. One deep-evergreen band per page may hold the "how we keep it fair" promises in white text.
- **One real photograph** gives the page its warmth: on the home page it is on the first phone screen, above the headline, and on desktop it sits in an arched frame beside the form. It never carries text and never claims to show Laftly work.
- Premium comes from editing: one photo, one action, plenty of space, nothing that does not do a job. No textures, gradients, badges, icon grids or decorative italics. Fewer elements, better made: serif numerals, hairlines and paper cards carry the detail.

## 2. Color tokens

Contrast ratios are WCAG 2.x relative-luminance ratios, computed for this file (D-3331).

| Token | Hex | Use | Checked contrast |
|---|---|---|---|
| `canvas` | `#F7F5F0` | page background | |
| `surface` | `#FFFFFF` | cards, forms, the 911 note, inputs | |
| `band` | `#ECEFE8` | quiet full-width bands (coverage, example, footer), the new-quote strip | |
| `ink` | `#1E2926` | text | 13.8:1 on `canvas`, 15.0:1 on `surface`, 12.9:1 on `band` |
| `ink-soft` | `#4A5952` | secondary text, hints, "Not given" | 6.8:1 on `canvas`, 7.4:1 on `surface`, 6.4:1 on `band` |
| `evergreen` | `#245849` | primary button fill, links (always underlined in text), kicker, step numbers | 7.5:1 on `canvas`, 8.2:1 on `surface`, 7.0:1 on `band` |
| `evergreen-deep` | `#183F35` | button hover and pressed, the primary button's 1 px edge | 11.7:1 with `on-evergreen`; 10.7:1 on `canvas`, 10.0:1 on `band` |
| `on-evergreen` | `#FFFFFF` | button labels | 8.2:1 on `evergreen` |
| `line` | `#C8CEC5` | card edges, rules, row dividers | decorative only (1.5:1); never the only edge of a control |
| `field-edge` | `#6E8077` | input, textarea, checkbox and secondary-button borders | 3.8:1 on `canvas`, 4.2:1 on `surface`, 3.6:1 on `band` (A11Y-028) |
| `night` | `#1F4A3E` | the one deep band per page | |
| `on-night` | `#FFFFFF` | text and links on `night` | 10.0:1 |
| `on-night-soft` | `#DCE8E0` | secondary text on `night` | 7.9:1 |
| `night-line` | `#557A6E` | hairlines between the promises on `night` | decorative only (2.1:1) |
| `focus` | `#A8581E` | 3 px focus ring, 3 px offset, on light surfaces | 4.7:1 on `canvas`, 5.2:1 on `surface`, 4.4:1 on `band` |
| `focus-on-night` | `#F2B27A` | the focus ring on `night` | 5.4:1 |
| `paper-shadow` | `0 1px 2px rgba(30,41,38,.05), 0 14px 32px -18px rgba(30,41,38,.18)` | under every card | decorative; the `line` edge still marks the card |
| `alert` | `#9B2C1F` | the 911 note's border and heading, form errors | 7.0:1 on `canvas`, 7.6:1 on `surface`, 6.5:1 on `band` |

Rules: a new pairing is computed and added here before use (4.5:1 for text, 3:1 for large text, component edges and the focus ring); the WEB-027 test checks every pair in this table on every surface it can sit on, input borders and the ring included. Text never sits on the photo. V5's copper ring `#C57A3B` was not kept: it gave 2.9:1 on `band`, under the 3:1 floor.

## 3. Type

- **Headlines and prices:** Gelasio 500 (400 where a lighter price reads better), SIL Open Font License, files and license in `docs/design/fonts/` (`gelasio-latin-400-normal.woff2`, `gelasio-latin-500-normal.woff2`, `OFL-Gelasio.txt`). Gelasio is drawn to Georgia's metrics, so the Georgia fallback keeps the same line breaks while the font loads.
- **Everything you read and every control:** DM Sans 400, 500, 700, SIL Open Font License, already in `docs/design/fonts/`.
- Self-host both (no font CDN, no third-party asset of any kind); `font-display: swap`; fallbacks Georgia and the system sans. No italics: emphasis is weight, never a slanted phrase. Cormorant Garamond is no longer used and its files are removed; T166 serves Gelasio and DM Sans.
- The brand name never breaks across lines (`white-space: nowrap` on the wordmark and on the name inside a sentence).

| Role | Phone (320 to 767) | Desktop (1024 and up) |
|---|---|---|
| Hero headline (`h1`, serif 500) | 28 to 36 / 1.1 (`clamp(28px, 8.75vw, 36px)`), tracking -0.02em; 52/56 at 768 | 64/68, tracking -0.025em |
| Page title (serif 500) | 30 to 36 / 1.12 | 44/50 |
| Section title (serif 500) | 28/34 | 36/42 at 768, 40/46 at 1024 |
| Card title, business name (serif 500) | 26/32 | 26/32 |
| Price (serif 500, lining figures) | 48/52 | 48/52 |
| Step numeral (serif 400, `evergreen`) | 44/44 | 64/64 |
| Step and promise heading (serif 500) | 22/28 | 24/30 |
| Fee amount (serif 500) | 28/32 | 28/32 |
| Body | 18/28 | 18/28; hero lede 20/32 |
| Small text, labels, hints, legal lines | 16/24, never smaller | 16/24 |
| Kicker (sans 600, sentence case, `evergreen`) | 16/24 | 16/24 |

**No text under 16 px, with no exceptions** (V5 used 10 to 13 px for its kicker, hints, banner and footer; Plaster and brass allowed a 14 px kicker). Paragraphs on phones are 18 px. Body text is never thinner than 400. Lines stay under about 70 characters on desktop. Headlines and section titles use `text-wrap: balance` so no line ends on one stray word.

## 4. Space, grid and shape

- Spacing steps: 4, 8, 12, 16, 20, 24, 28, 32, 40, 56, 72, 88 px.
- Phone gutter 20 px; the hero photo runs edge to edge on phones. Tablet (768 to 1023) gutter 40 px, photo inset with 20 px corners. Desktop: content width up to 1160 px, hero in two columns (1 : 0.86, 88 px apart), text left, photo right. Two-column sections share the 88 px gap; three-column rows (steps, promises) sit 56 px apart. Sections are 56 px top and bottom on phones, 96 px on desktop.
- Corners: buttons and inputs 12 px; cards and the 911 note 14 px; the Menu control is a full pill (22 px); photos 20 px when inset, square when edge to edge. On desktop the hero photo is an arch: 248 px top corners, 20 px bottom corners.
- Shadows: `paper-shadow` under cards, the open menu panel (`0 12px 28px rgba(30,41,38,.12)`), and the primary button's own edge (a 1 px inner highlight and a 1 px drop). Nothing else. Hover never adds or grows a shadow.
- Rules: 1 px `line` between sections, list rows and fact rows; a 1 px `ink` rule tops each step column on tablet and desktop; `night-line` on the night band.
- Targets: every link and control is at least 44 px tall where it stands alone (header links, menu, footer links) and never under 24 by 24 px (A11Y-006).

## 5. Imagery

| File | What it is | Where |
|---|---|---|
| `photos/hero-kitchen.webp` | warm kitchen in daylight, 557 by 941, 82 KB | home hero |

**How the photo is cropped** (`object-fit: cover`): phones, a wide strip across the vase, the bowl and the window (`object-position: 50% 57%`), as tall as the first screen allows: `clamp(132px, 100svh - 606px, 252px)`, and `clamp(124px, 100svh - 616px, 252px)` under 360 px wide, so the photo takes the room the form leaves and the button stays on the first screen. Tablet: 360 px tall, inset, at `50% 62%`. Desktop: 640 px tall arch at `50% 78%`, so the lamp sits in the crown of the arch and the stools ground it. No overlay, tint, border or caption.

| File | What it is | Where |
|---|---|---|
| `brand/mark.svg` | the connected-form mark from Casey's guide, traced to a clean vector in `evergreen`, 446 by 399 box | beside the wordmark in the header and footer, 30 by 27 px |
| `brand/mark-candidate.png` | the mark as it came with the guide | kept as the source of the trace |

- **Concept placeholders, replace with licensed photography before launch** (T216 serves the photo at a fixed name so the swap is a file change). The photo came with Casey's V5 guide as a generated concept image. It is not a picture of a business on Laftly or of work done through Laftly, and no caption may say it is. Launch item: licensed photos (stock with a written license, or local photos with written permission) replace it at the same name and size; the replacement is checked for the first-screen measurements in section 9.
- One photo per page at most, at the top of marketing pages (home, how it works, business page). Portals, the quote page, forms and emails have none.
- Photos are decorative (`alt=""`) unless they show something the text relies on; they never carry text, logos, people's faces or house numbers. Each is a local WebP under 250 KB, with width and height set so nothing shifts, and no metadata (`scripts/pii_scan.py`).
- V5's second concept image (a pool) was dropped: it was not used on the page and was too soft to keep.
- **Brand:** the wordmark is "Laftly", set in DM Sans 700 at 24 px with -0.035em tracking, with the mark to its left (9 px gap). The build reads the name from the brand file (`app/core/data/brand.json`, through `core::api::brand()` or a template token, never a literal) and the mark from the same setting, so the name lives in one place. That file still says "Ask Hava" until the rename plan change moves it; the rename is Casey's decision of November 12, 2026, and this spec, the reference and the mock-ups already show Laftly. Wording rules that quote the old name (the WORD-002 statement, the "Ask Hava:" text prefix) are for that plan change, not this file.

## 6. Components

- **Header:** `canvas`, 60 px on phones, 76 px from 768 up, a 1 px `line` under it. Phones: wordmark (and mark) left, then "Español" and a "Menu" pill on `surface` with a `field-edge` border (a `<details>` disclosure, so it works without JavaScript) holding How it works, What we cover, For businesses and Help. Desktop: the same four links as plain text and "Español". Help is in the same place on every page (A11Y-017). No button in the header.
- **Service ZIP (location):** the home page has no ZIP field and no location control. ZIP is asked only inside the chat, at intake step 4 after the consent and 18+ screen (INT-049, CON-025), and an unknown or out-of-area ZIP is handled there (INT-022). Nothing is looked up from the IP address or the browser's location, and no wording suggests a place was detected ("Near Havasu?" is not used). The kicker above the headline names the market in plain text ("Lake Havasu City, Arizona") and is not a control.
- **The request box (home hero):** sits straight on `canvas`, no card. One label ("What needs doing?"), one hint line above the field, one textarea (84 px on phones, 104 px from 768 up), the full-width primary button ("Get quotes"), then the fixed cost line (CON-003). No ZIP field, no placeholder text (A11Y-029), no example buttons that need JavaScript, no voice or chat-bubble decoration. **Submitting it lands on the CON-025 web-chat start screen (the consent text above the chat box and the 18+ line) before anything is stored:** what she typed is carried into that screen's chat box for her to send, and nothing is saved, sent to AI or matched until she starts the chat there (AI-021, A11Y-035). Until Gate C the hero names no texting number (WEB-017); once the number shows, the texting consent line (CON-005) sits under the cost line.
- **Primary button:** `evergreen` fill, 1 px `evergreen-deep` edge, `on-evergreen` label, DM Sans 700 18 px, 56 px tall, 12 px corners, full width inside cards and in the request box. One per decision area. The label names the action ("Get quotes", "Pick Sample Air Repair", "Confirm job 1042"). No arrows or icons in the label. Never disabled (A11Y-031).
- **Secondary button:** `surface` fill, 1 px `field-edge` border, `ink` label, 500 weight.
- **Text link:** `evergreen`, underlined in running text; on `night`, `on-night`, underlined. Header and footer links may drop the underline (their position marks them) and underline on hover.
- **How it works:** three steps. Phones: a list with `line` rules, each step a 44 px serif numeral in `evergreen` beside a serif heading and a short paragraph. From 768 up: three columns, each under a 1 px `ink` rule, with a 64 px numeral above the heading. The numerals are decoration for the ordered list (`aria-hidden`); no icons.
- **Quote card (paper):** the arrival number written "#1", "#2" (the form WORD-005's "#1 means first received, not first choice." uses) and the received time above a rule; the business name (serif); "Independent business, not affiliated with Laftly." (WORD-014); the price large in serif with its format beside it; the quote facts as label and value rows; "In their words" with the business's words as a serif block quote, not italic ("Not given" in plain `ink-soft`, never as a warning); the license line; ROC, Google and Yelp as underlined plain links; "Laftly doesn't rank businesses; check it yourself." (PICK-041); the Pick button; "or reply n YES" (PICK-015). Every card is identical in size, color and order of parts, and cards appear in arrival order only: no highlight, star, "new" ribbon or color on any one card.
- **Example quotes (home):** on a `band` section, two paper quote cards side by side (stacked on phones) under one label row, "Example: weekly pool cleaning" and "Not real quotes", with the kicker "Example" on the section. Made-up business names that say "Sample", fake prices, arrival order, identical cards. They show format, not market prices.
- **Fee card (home, business page):** one paper card titled "Introduction fee by job size": three rows, the quote size on the left and the fee in serif on the right ($25, $75, $199); then the visit-fee line (a visit-fee quote uses its category's set tier, $25 by default, FEE-004); then the repeating-service line (sized at 4 months of its price, never more than $75, FEE-039), and any example price on the same page must agree with it (the home example's $140 a month is $560 over 4 months, so $75); then the notice line. Headings say "Pay only when you confirm a pick" (BIZ-030), never "if a homeowner picks you". Never the word "commission".
- **Business notice line (WEB-024):** the fee card's last line follows the business-texting switch (TXT-035): while it is off, "Job requests and picks come by email. We don't text or call businesses."; while it is on, "Choose how you get job requests and picks: email, text or both. Laftly never calls you with a recording." The template reads the switch; neither line is typed into a page.
- **Who runs Laftly (home, help, business page):** the WEB-021 facts in WEB-022's order: the LLC and Havasu lines, the LLC mailing address (registered agent or PO box, never a home address) and the help email, each read from settings and shown as a labeled row, the ACC record link and city license number once each exists, and the day-job and reply-time line. The reference shows the address and email as bracketed placeholders.
- **Forms (on `surface`):** labels 16 px bold above the field, hints `ink-soft` 16 px before the field, inputs 1.5 px `field-edge` border on `surface`, 12 px corners, 18 px text, 48 px tall at least (56 px in the request box); the border goes to `ink` on hover and `evergreen` with the ring on focus; checkbox 24 px, radio 24 px, each option a 48 px row with a `line` divider; errors in `alert` with the problem in words, linked from an error summary at the top of `main` (A11Y-011, A11Y-030). Code fields follow A11Y-032.
- **911 note:** a `surface` panel with a 2 px `alert` border all round, 14 px corners, no shadow, a heading "In an emergency, call 911" in `alert` (DM Sans 700 18 px) and the fixed WEB-022 wording in `ink`. Never red-filled, never inside a quote card or form, never a left stripe, no icon. Placed on the home and help pages beside "Who runs Laftly", and in the web chat above the first question.
- **The fair band (`night`):** the three "how we keep it fair" promises (site-copy fair 1 to 3), each a serif heading and a short paragraph under a `night-line` rule; three columns from 768 up. No buttons on `night`.
- **Footer:** `band` with a `line` top edge; the mark and wordmark, the tagline "Local quotes. Your choice." in serif 20 px (WEB-004), the full not-a-contractor statement beginning "Laftly is not a contractor and does not perform, bid on or supervise work." (WORD-002, WEB-015), the "Homeowners" column with How it works, Services, Get quotes and Help (WEB-003), a second column with For businesses, Español, Terms, Privacy and Accessibility, then a `line` rule and "Lake Havasu City, Arizona". Every footer link is a 44 px row.
- **Quote page head:** the kicker (request number and trade), the title "Your quotes", a small paper summary (quoted so far; open until), the cost line, then the ranking, order and not-a-contractor lines as a ruled list. From 1024 up the head sits in a left column and the cards in a right column.

## 7. States and motion

- **Focus:** always visible; 3 px ring in `focus` (`focus-on-night` on the night band), 3 px offset, on every link, button and control (A11Y-005).
- **Hover (desktop):** buttons go to `evergreen-deep` (a 150 ms color fade); links thicken their underline; field and Menu borders go to `ink`. No movement, lift or shadow.
- **Pressed:** `evergreen-deep`.
- **Disabled:** used only where the spec requires; 55% opacity with the reason in words beside it. Submit buttons are never disabled (A11Y-031).
- **Loading:** a short line of text ("Sending…"), never a spinner alone.
- **New quote arriving:** a `band` strip at the top of the list, "1 new quote received · Show", without moving the page; the `role="status"` line is empty in the first render and gets one summary line per update (A11Y-014, A11Y-033).
- **Motion:** fades of 150 to 200 ms at most; none with `prefers-reduced-motion` (A11Y-009). No carousels, autoplay or parallax.

## 8. Where each surface is used

| Surface | Page (`canvas`) | Card (`surface`) | Other |
|---|---|---|---|
| Home | header, hero text and form, how it works, business section, who runs it | example quotes, the 911 note, fee rows | the one photo; `band` for coverage, example and footer; `night` for the fair band |
| How it works, help, business page, what we cover, terms and policies | page, with at most one photo at the top (not on help, terms or policies) | the 911 note (help), sample job cards (business page, WEB-020), long legal text in a reading column on `canvas` | |
| Web chat | page and the conversation | each Laftly message on `surface` with a `line` edge; hers on `band`; the current question's form | the 911 note above the first question; one question per page without JavaScript (A11Y-035) |
| Her quote page | title, status and the WORD-002 statement at the top | every quote card | new-quote strip on `band` |
| Card approval, photo step, pick review, code entry | title and what happens next | the whole form or card; the pick review shows the PICK-017 lines and one Pick button | the photo step is a native file input with a visible label (A11Y-027) |
| Business portal and owner portal | header and page title | the working area: tables, forms and job cards, for dense reading; tables scroll inside a labeled region (A11Y-008) | no photo, no `night` band |
| Emails | not used | white (`#FFFFFF`) body in a 600 px column on `canvas` (`#F7F5F0`); `ink` text 16 to 18 px; Georgia for headings, Arial for text (no web fonts in mail); one solid `#245849` button with a white label, its text also given as a plain link; **no background images**, no photo, no mark image required (the wordmark is text); `lang` set (A11Y-034) and a plain-text part (A11Y-019) | |
| Texts | not applicable | | |

## 9. Checks before a page ships

- every pairing above passes; a new one is computed and added here
- no text under 16 px anywhere, at any width
- home first screen, normal text size: the photo and the whole "Get quotes" button are visible without scrolling at 390 by 844 and at 320 by 740 (the reference measures photo 61 to 299 px and button bottom 792 px at 390 by 844; photo 61 to 185 px and button bottom 699 px at 320 by 740; at 768 by 1024 photo 105 to 465 px and button bottom 971 px; at 1440 by 900 photo 149 to 789 px and button bottom 717 px). At larger text sizes and zoom, reflow and no overlap win over the first-screen target.
- no sideways scroll at 320, 390, 768 and 1440 px, and at 400% zoom (A11Y-008)
- axe-core shows no violation at 320, 390, 768 and 1440 px (A11Y-001, A11Y-002); the reference pages show 0 violations and 0 incomplete at all four on axe-core 4.13.0 (WCAG 2.0 to 2.2 A and AA, and best-practice rules)
- every control reachable by keyboard with the visible ring
- no third-party request: fonts, photo and mark are served by the site (`tests/security/tests/no_third_party.rs`)
- the copy passes `scripts/check_ai_smell.py`, `scripts/check_copy.py` and the cold read: no "commission", "vetted", "best" or ranking, no em dashes, exclamation marks or arrows-as-decoration
- no element without a job: no badges, no gradients, no textures, no shadows beyond section 4

## 10. From Plaster and brass to V6

| Plaster and brass (D-451) | V6 (D-3331) |
|---|---|
| wall (`#232322` plaster) | page (`canvas`) |
| paper (`#F3ECE0` card stock) | card (`surface`) |
| brass button, rules, focus | `evergreen` button; `line` rules; `focus` ring |
| `brass-text` kicker, links on the wall | `evergreen` kicker and links |
| alert on a dark panel | 911 note on `surface` with an `alert` border |
| Cormorant Garamond | Gelasio |
| the niche holding an example card | the hero photo; the example card moves below the fold |
| test values: lightest plaster patch, darkest brass pixel | the flat tokens in section 2 (no texture to sample) |

The plaster, brass and paper textures and the Cormorant Garamond files were removed with this change; T216 now covers licensed photography instead (D-3331).

## 11. The Laftly design pass (November 12, 2026, D-3331)

What changed from the first V6 draft, and why:

- **Name and mark:** Laftly throughout; the connected-form mark is redrawn as a vector (`brand/mark.svg`) so it stays sharp at 30 px and on dense screens.
- **Hero:** on desktop the photo is a 640 px arch, which reads as a place, not a template; the headline is larger (64/68) and balanced. On phones the photo height follows the screen, so it is as tall as the first screen allows.
- **ZIP:** the draft's ZIP field beside the button was removed after the independent review: it asked before consent (CON-025) and before intake step 4 (INT-049) and skipped the out-of-area path (INT-022). The chat asks for the ZIP; the home page names the market in plain text.
- **How it works:** large serif numerals and rules in place of small numbers in a list.
- **Cards:** paper (edge plus one soft shadow), 14 px corners, a ruled top row, a 48 px serif price. The example shows two real-shaped cards, not cards nested in a card.
- **Fees:** a single fee card with serif amounts in place of three boxed sentences.
- **Quote page:** a summary card and a ruled list replace five loose paragraphs; two columns on desktop.
- **Footer:** mark, serif tagline, two link columns, 44 px link rows.
- **Controls:** 12 px corners, a defined button edge, hover states on fields; all pairs recomputed (section 2).
