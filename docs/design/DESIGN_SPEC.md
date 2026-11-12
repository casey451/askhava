# Ask Hava design spec: Daylight and evergreen (V6)

**Status:** chosen by Casey, November 12, 2026 ("This is a guide. You can make it better."), decision D-3331, which supersedes D-451 (Plaster and brass). Every page, portal screen and email follows this file (WEB-027). Wording, rules and numbers still come only from `docs/spec/` and `docs/copy/site-copy.md`; this file decides how things look. Until the follow-up plan change named in D-3331 rewords WEB-001, WEB-027, TD-WEB-11 and the site tasks, any line there that names plaster, brass, paper textures or Cormorant Garamond is read as this file's matching rule (section 10).

**Mock-ups to build from** (`docs/design/mockups/`, rendered with Playwright from the reference at device scale 1):
- `v6-home-390-first-screen.png`: home page, 390 by 844, the first screen
- `v6-home-390-full.png`: home page, 390 px, whole page
- `v6-home-1440-first-screen.png`: home page, 1440 by 900, the first screen
- `v6-quote-page-390-full.png`: her quote page, 390 px, whole page

The HTML and CSS behind them are in `docs/design/reference/` (`home.html`, `quote_page.html`, `askhava-v6.css`; static, no JavaScript; a visual reference only, the build writes its own templates). The photo is in `docs/design/photos/`, the candidate mark in `docs/design/brand/`, the fonts in `docs/design/fonts/`.

## 1. The idea: a calm room in daylight

- **The page** is warm ivory, like a well-lit room. Headlines, navigation and plain explanation sit on it.
- **The card** is white with a thin grey-green edge. Everything a person reads closely or decides on sits on a card: quote cards, the job card, the pick review step, forms, codes, receipts. Cards are where contrast is highest.
- **Evergreen** is the one action color: the primary button, links, the step numbers, the kicker. One deep-evergreen band per page may hold the "how we keep it fair" promises in white text.
- **One real photograph** gives the page its warmth: on the home page it is on the first phone screen, above the headline. It never carries text and never claims to show Ask Hava work.
- Premium comes from editing: one photo, one action, plenty of space, nothing that does not do a job. No textures, gradients, badges, icon grids or decorative italics.

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
| `evergreen-deep` | `#183F35` | button hover and pressed | 11.7:1 with `on-evergreen` |
| `on-evergreen` | `#FFFFFF` | button labels | 8.2:1 on `evergreen` |
| `line` | `#C8CEC5` | card edges, rules, row dividers | decorative only (1.5:1); never the only edge of a control |
| `field-edge` | `#6E8077` | input, textarea, checkbox and secondary-button borders | 3.8:1 on `canvas`, 4.2:1 on `surface`, 3.6:1 on `band` (A11Y-028) |
| `night` | `#1F4A3E` | the one deep band per page | |
| `on-night` | `#FFFFFF` | text and links on `night` | 10.0:1 |
| `on-night-soft` | `#DCE8E0` | secondary text on `night` | 7.9:1 |
| `focus` | `#A8581E` | 3 px focus ring, 3 px offset, on light surfaces | 4.7:1 on `canvas`, 5.2:1 on `surface`, 4.4:1 on `band` |
| `focus-on-night` | `#F2B27A` | the focus ring on `night` | 5.4:1 |
| `alert` | `#9B2C1F` | the 911 note's border and heading, form errors | 7.0:1 on `canvas`, 7.6:1 on `surface`, 6.5:1 on `band` |

Rules: a new pairing is computed and added here before use (4.5:1 for text, 3:1 for large text, component edges and the focus ring); the WEB-027 test checks every pair in this table on every surface it can sit on, input borders and the ring included. Text never sits on the photo. V5's copper ring `#C57A3B` was not kept: it gave 2.9:1 on `band`, under the 3:1 floor.

## 3. Type

- **Headlines and prices:** Gelasio 500 (400 where a lighter price reads better), SIL Open Font License, files and license in `docs/design/fonts/` (`gelasio-latin-400-normal.woff2`, `gelasio-latin-500-normal.woff2`, `OFL-Gelasio.txt`). Gelasio is drawn to Georgia's metrics, so the Georgia fallback keeps the same line breaks while the font loads.
- **Everything you read and every control:** DM Sans 400, 500, 700, SIL Open Font License, already in `docs/design/fonts/`.
- Self-host both (no font CDN, no third-party asset of any kind); `font-display: swap`; fallbacks Georgia and the system sans. No italics: emphasis is weight, never a slanted phrase. Cormorant Garamond is no longer used (its files stay until the follow-up plan change moves T166 off them).
- The brand name never breaks across lines (`white-space: nowrap` on the wordmark and on the name inside a sentence).

| Role | Phone (320 to 767) | Desktop (1024 and up) |
|---|---|---|
| Hero headline (`h1`, serif 500) | 28 to 36 / 1.12 (`clamp(28px, 8.75vw, 36px)`); 48 at 768 | 60/66 |
| Page title (serif 500) | 28 to 34 / 1.15 | 44/50 |
| Section title (serif 500) | 28/34 | 36/42 |
| Card title, business name (serif 500) | 26/32 | 26/32 |
| Price (serif 500) | 44/48 | 44/48 |
| Body | 18/28 | 18/28; hero lede 20/32 |
| Small text, labels, hints, legal lines | 16/24, never smaller | 16/24 |
| Kicker (sans 600, sentence case, `evergreen`) | 16/24 | 16/24 |

**No text under 16 px, with no exceptions** (V5 used 10 to 13 px for its kicker, hints, banner and footer; Plaster and brass allowed a 14 px kicker). Paragraphs on phones are 18 px. Body text is never thinner than 400. Lines stay under about 70 characters on desktop.

## 4. Space, grid and shape

- Spacing steps: 4, 8, 12, 16, 20, 24, 28, 32, 40, 56, 72, 88 px.
- Phone gutter 20 px; the hero photo runs edge to edge on phones. Tablet (768 to 1023) gutter 40 px, photo inset with 18 px corners. Desktop: content width up to 1180 px, hero in two columns (1.05 : 0.95, 72 px apart), text left, photo right.
- Corners: buttons, inputs and fee rows 10 px; cards and the 911 note 12 px; photos 18 px when inset, square when edge to edge.
- Shadows: only the open menu panel (`0 12px 28px rgba(30,41,38,.12)`). Cards are separated by their `line` edge and space, not by shadow.
- Rules: 1 px `line` between sections, list rows and fact rows.
- Targets: every link and control is at least 44 px tall where it stands alone (header links, menu, footer links) and never under 24 by 24 px (A11Y-006).

## 5. Imagery

| File | What it is | Where |
|---|---|---|
| `photos/hero-kitchen.webp` | warm kitchen in daylight, 557 by 941, 82 KB | home hero |
| `brand/mark-candidate.png` | the connected-form mark from Casey's guide, evergreen, 446 by 399 | beside the wordmark, while the brand setting names it |

- **Concept placeholders, replace with licensed photography before launch.** The photo came with Casey's V5 guide as a generated concept image. It is not a picture of a business on Ask Hava or of work done through Ask Hava, and no caption may say it is. Launch item: licensed photos (stock with a written license, or local photos with written permission) replace it at the same name and size; the replacement is checked for the first-screen measurements in section 9.
- One photo per page at most, at the top of marketing pages (home, how it works, business page). Portals, the quote page, forms and emails have none.
- Photos are decorative (`alt=""`) unless they show something the text relies on; they never carry text, logos, people's faces or house numbers. Each is a local WebP under 250 KB, with width and height set so nothing shifts, and no metadata (`scripts/pii_scan.py`).
- V5's second concept image (a pool) was dropped: it was not used on the page and was too soft to keep.
- **Brand:** the wordmark is the brand file's display name (`app/core/data/brand.json`, read through `core::api::brand()` or a template token, never a literal), set in DM Sans 700. The mark is one image named by the same brand setting; while that setting has no mark, the wordmark stands alone. The connected-form mark is the **candidate mark pending a rename decision**: Casey's guide drew it for another name, the rename is not approved, and the brand stays "Ask Hava". Changing the name or mark is a change to that one setting and its image, never to templates.

## 6. Components

- **Header:** `canvas`, 64 px on phones, 76 px from 768 up, a 1 px `line` under it. Phones: wordmark (and mark) left, then "Español" and a "Menu" button (a `<details>` disclosure, so it works without JavaScript) holding How it works, What we cover, For businesses and Help. Desktop: the same four links as plain text and "Español". Help is in the same place on every page (A11Y-017). No button in the header.
- **Service ZIP (location):** ZIP is confirmed, never inferred (INT-048, INT-049). On the home hero the kicker line reads the market's name and a "Set your ZIP" link ("Lake Havasu City · Set your ZIP"); once she has entered one it reads "ZIP 86403 · Change". The link opens a server-rendered page with one labeled 5-digit field ("ZIP code of the home the job is at"). Nothing is looked up from the IP address or the browser's location, and no wording suggests a place was detected ("Near Havasu?" is not used). A ZIP set this way is shown back to her for confirmation at the chat's ZIP step; whether that confirmation can stand in for asking (A11Y-012) is for the follow-up plan change, so until then the chat asks as INT-049 says.
- **The request box (home hero):** one visible label ("What needs doing?"), one hint line above the field, one textarea, one primary button ("Get quotes"), then the fixed cost line (CON-003). No placeholder text (A11Y-029), no example buttons that need JavaScript, no voice or chat-bubble decoration. It posts to the web chat's first step, which keeps what she typed through the phone-code step (AI-021, A11Y-035). Until Gate C the hero names no texting number (WEB-017); once the number shows, the texting consent line (CON-005) sits under the cost line.
- **Primary button:** `evergreen` fill, `on-evergreen` label, DM Sans 700 18 px, 56 px tall, 10 px corners, full width on phones inside forms and cards. One per decision area. The label names the action ("Get quotes", "Pick Sample Air Repair", "Confirm job 1042"). No arrows or icons in the label. Never disabled (A11Y-031).
- **Secondary button:** `surface` fill, 1 px `field-edge` border, `ink` label, 500 weight.
- **Text link:** `evergreen`, underlined in running text; on `night`, `on-night`, underlined. Header and footer links may drop the underline (their position marks them) and underline on hover.
- **Quote card (`surface`):** No. and received time; the business name (serif); "Independent business, not affiliated with Ask Hava." (WORD-014); a rule; the price large in serif with its format beside it; the quote facts as label and value rows; "In their words" with the business's words in serif, not italic ("Not given" in plain `ink-soft`, never as a warning); the license line; ROC, Google and Yelp as underlined plain links; "Ask Hava doesn't rank businesses; check it yourself." (PICK-041); the Pick button; "or reply n YES" (PICK-015). Every card is identical in size, color and order of parts, and cards appear in arrival order only: no highlight, star, "new" ribbon or color on any one card.
- **Example quotes (home):** two small quote cards inside one card labeled "Example" and "Not real quotes", with made-up business names that say "Sample" and fake prices, in arrival order. They show format, not market prices.
- **Forms (on `surface`):** labels 16 px bold above the field, hints `ink-soft` 16 px before the field, inputs 1.5 px `field-edge` border, 18 px text, 48 px tall at least; checkbox 24 px, radio 24 px, each option a 48 px row with a `line` divider; errors in `alert` with the problem in words, linked from an error summary at the top of `main` (A11Y-011, A11Y-030). Code fields follow A11Y-032.
- **911 note:** a `surface` panel with a 2 px `alert` border all round, 12 px corners, a heading "In an emergency, call 911" in `alert` (DM Sans 700 18 px) and the fixed WEB-022 wording in `ink`. Never red-filled, never inside a quote card or form, never a left stripe, no icon. Placed on the home and help pages beside "Who runs Ask Hava", and in the web chat above the first question.
- **The fair band (`night`):** the three "how we keep it fair" promises (site-copy fair 1 to 3), each a serif heading and a short paragraph. No buttons on `night`.
- **Footer:** `band` with a `line` top edge; wordmark, the tagline "Local quotes. Your choice." (WEB-004), the full not-a-contractor statement (WORD-002, WEB-015), the "Homeowners" column with How it works, Services, Get quotes and Help (WEB-003), then Español, Terms, Privacy and Accessibility.

## 7. States and motion

- **Focus:** always visible; 3 px ring in `focus` (`focus-on-night` on the night band), 3 px offset, on every link, button and control (A11Y-005).
- **Hover (desktop):** buttons go to `evergreen-deep`; links thicken their underline. No movement, lift or shadow.
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
| Web chat | page and the conversation | each Ask Hava message on `surface` with a `line` edge; hers on `band`; the current question's form | the 911 note above the first question; one question per page without JavaScript (A11Y-035) |
| Her quote page | title, status and the WORD-002 statement at the top | every quote card | new-quote strip on `band` |
| Card approval, photo step, pick review, code entry | title and what happens next | the whole form or card; the pick review shows the PICK-017 lines and one Pick button | the photo step is a native file input with a visible label (A11Y-027) |
| Business portal and owner portal | header and page title | the working area: tables, forms and job cards, for dense reading; tables scroll inside a labeled region (A11Y-008) | no photo, no `night` band |
| Emails | not used | white (`#FFFFFF`) body in a 600 px column on `canvas` (`#F7F5F0`); `ink` text 16 to 18 px; Georgia for headings, Arial for text (no web fonts in mail); one solid `#245849` button with a white label, its text also given as a plain link; **no background images**, no photo, no mark image required (the wordmark is text); `lang` set (A11Y-034) and a plain-text part (A11Y-019) | |
| Texts | not applicable | | |

## 9. Checks before a page ships

- every pairing above passes; a new one is computed and added here
- no text under 16 px anywhere, at any width
- home first screen, normal text size: the photo and the whole "Get quotes" button are visible without scrolling at 390 by 844 and at 320 by 740 (the reference measures photo 65 to 254 px and button bottom 753 px at 390 by 844; photo 65 to 205 px and button bottom 735 px at 320 by 740; at 1440 by 900 photo 166 to 746 px and button bottom 705 px). At larger text sizes and zoom, reflow and no overlap win over the first-screen target.
- no sideways scroll at 320, 390, 768 and 1440 px, and at 400% zoom (A11Y-008)
- axe-core shows no violation at 320, 768 and 1280 px (A11Y-001, A11Y-002)
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

The textures in `docs/design/textures/` are no longer part of the look; they stay in the repository only until the follow-up plan change rewrites T216, which still names them.
