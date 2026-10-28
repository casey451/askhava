# Ask Hava design spec: Plaster and brass

**Status:** chosen by Casey, October 28, 2026 ("This is the design that I want to go with"), D-451. Every page, portal screen and email follows this file (WEB-027). Wording, rules and numbers still come only from `docs/spec/`; this file decides how things look.

**Mock-ups to build from** (`docs/design/mockups/`):
- `AskHava_desktop_home.jpg`: home page, 1440 px
- `AskHava_phone_home.jpg`: home page, 390 px
- `AskHava_phone_quotes.jpg`: her quote page, 390 px
- `AskHava_phone_pick.jpg`: pick review step, 390 px, with the keyboard focus ring on the Pick button

The HTML and CSS behind them are in `docs/design/reference/` (a visual reference only; the build writes its own templates). The textures are in `docs/design/textures/`, the fonts in `docs/design/fonts/`.

## 1. The idea: the wall and the paper

- **The wall** is dark Venetian plaster with brushed-brass details, lit by a soft warm wash from the upper left, like a gallery wall under a picture light. Headlines, navigation and calm explanation sit on the wall.
- **The paper** is cream card stock. Everything a person reads closely or decides on sits on paper: quote cards, the job card, the pick review step, forms, codes, receipts. Paper is where contrast is highest.
- **Brass** is the one accent: primary buttons, thin rules, the arched niche, the footer's top edge and the focus ring. Brass never carries small text on the wall.
- Premium comes from restraint: few elements, precise spacing, real materials, no decoration that does not do a job.

## 2. Color tokens

| Token | Hex | Use | Checked contrast |
|---|---|---|---|
| `wall` | `#232322` | page background under the plaster texture | |
| `wall-deep` | `#161514` | footer and overlays (at 82 to 85% opacity) | |
| `on-wall` | `#F1EBE0` | text on the wall | 8.3:1 on the lightest plaster patch `#45433F` |
| `on-wall-soft` | `#DCD4C7` | secondary text on the wall | 6.7:1 on `#45433F` |
| `brass-text` | `#E2C48F` | kickers, step numerals, "You pick." italic, links on the wall | 5.9:1 on `#45433F` |
| `paper` | `#F3ECE0` | paper surfaces under the paper texture | |
| `ink` | `#1F1D1B` | text on paper | 14.3:1 |
| `ink-soft` | `#4F4A43` | secondary text on paper | 7.5:1 (6.7:1 on the darkest paper fibre `#E9E0D0`) |
| `paper-line` | `#D9CCB6` | rules and row dividers on paper | decorative |
| `paper-link` | `#6B4A1E` | links on paper, always underlined | 6.8:1 |
| `brass` | `#B8935A` | fallback fill under the brass texture | |
| `on-brass` | `#1B1915` | button labels on brass | 6.4:1 at the darkest pixel of `brass-bar@2x.jpg` |
| `focus` | `#E2C48F` on the wall, `#8A6A2E` on paper | 3 px focus ring, 3 px offset | 9.4:1 and 4.3:1 |
| `alert` | `#F2A898` text, `#C9786A` border | the 911 note on the wall; on paper, errors use `#8C291D` | 7.6:1 text, 4.8:1 border |

Rules: a new pairing is checked before use (4.5:1 for text, 3:1 for large text and component edges); a test computes the brass button's worst pixel against `on-brass` and fails under 4.5:1.

## 3. Type

- **Headlines:** Cormorant Garamond 400 and 500, 400 italic for one emphasized phrase per screen ("You pick."). SIL Open Font License, files and license in `docs/design/fonts/`.
- **Everything you read:** DM Sans 400, 500, 700. SIL Open Font License.
- Self-host both (no font CDN); `font-display: swap`; fallbacks Georgia and the system sans.

| Role | Phone | Desktop |
|---|---|---|
| Hero headline | 50/50 | 92/90 |
| Page title | 46 to 50/48 to 50 | 64/66 |
| Section title | 34/38 | 42/46 |
| Card title (business name) | 30/32 | 31/32 |
| Price | 54/52 serif | 58/54 serif |
| Body | 18/28 to 29 | 19 to 21/30 to 34 |
| Small text and labels | 16/23 to 25, never smaller | 16 to 17 |
| Kicker (all caps, 2.6 to 3.2 px tracking) | 14/20 | 16/22 |

The kicker is the only text under 16 px; it repeats nothing essential. Body text is never thinner than 400 and never below 18 px for paragraphs on phones.

## 4. Space, grid and shape

- Spacing steps: 4, 8, 12, 16, 20, 24, 28, 32, 40, 56, 72, 96 px.
- Phone gutter 22 px; paper sheets inset 16 px from the edge. Desktop: 96 px margins, 1248 px content width, hero in two columns (1.25 : 0.95).
- Corners: buttons and paper 3 to 4 px (crisp, like cut stock and metal); the niche is an arch (half its width as the top radius).
- Shadows only for things that sit off the wall: paper (`0 22px 44px rgba(0,0,0,.5)`), the niche, brass buttons. Nothing else casts a shadow.
- Rules: a 1 px brass hairline that fades at both ends separates sections on the wall; `paper-line` separates rows on paper.

## 5. Materials

| File | What it is | Where |
|---|---|---|
| `textures/plaster@2x.jpg` | charcoal Venetian plaster with trowel strokes, 2880 by 3520 | page background, the niche recess |
| `textures/brass-bar@2x.jpg` | light brushed brass, horizontal grain, wide highlight | buttons, rules, the picture light, the footer edge |
| `textures/brass-plate@2x.jpg` | brushed brass plate with a diagonal highlight | the arched niche frame |
| `textures/paper@2x.jpg` | cream card stock with fine fibre | every paper surface |

- The page adds two soft radial "wall-wash" lights (warm, 9 to 14% opacity) and a slight darkening toward the bottom.
- **Real photos later:** these textures were generated for the mock-ups. When close-up photos of real plaster and brushed brass are available (for example from a local finisher, with written permission), replace the files at the same names and sizes; keep the contrast checks.
- **Performance:** each texture is a single compressed image under 400 KB, loaded once and cached; the page paints the fallback color first, so text is readable before the texture arrives. On "reduce data" (`prefers-reduced-data` where supported) skip the plaster image and keep the color.

## 6. Components

- **Header:** wordmark (Cormorant 500) left; on desktop, plain text links plus a small brass "Text us your job" button; on phones, Español and a "Menu" button outlined in brass. A brass hairline under it.
- **Primary button:** brass texture, `on-brass` label, 700 weight, 56 px tall on phones (48 in the header), 3 px corners, a fine top highlight and bottom edge. One per decision area. The label names the action ("Pick Desert Air HVAC", "Text us your job").
- **Secondary button:** transparent, 1 px brass outline on the wall, 1 px ink outline on paper, 500 weight.
- **Text link:** brass on the wall, `paper-link` and underlined on paper.
- **Quote card (paper):** No. and received time; business name (serif); the "Independent business" line; a rule; the price large in serif with its format beside it; the quote facts as label and value rows; "In their words" in serif italic ("Not given" in plain sans, never as a warning); the license line; ROC, Google and Yelp as underlined text links; the "check it yourself" line; the brass Pick button; "or reply n YES". Every card is identical in size, color and order.
- **The niche (home hero):** a brass arch frame, a recessed plaster arch with an inner shadow, a small brass picture light and its glow, holding an example quote card labeled "Example". On phones it sits under the hero text.
- **Forms (on paper):** checkbox 26 px, radio 24 px, both drawn in `ink`; each option a 52 px row with a `paper-line` divider; legends 18 px bold; helper text `ink-soft` 16 px; errors in `#8C291D` with the problem in words.
- **911 note:** a dark panel (`rgba(16,15,14,.62)`) with the `alert` border and heading; never on paper, never red-filled.
- **Footer:** `wall-deep` with a 2 px brass edge on top; wordmark, the "Who runs Ask Hava" line, the WORD-002 statement and the footer links.

## 7. States and motion

- **Focus:** always visible; 3 px ring in `focus`, 3 px offset, on every link, button and control (shown in `AskHava_phone_pick.jpg`).
- **Hover (desktop):** the brass highlight shifts slightly (background position), links underline; no movement, no bounce.
- **Pressed:** the brass darkens 6% and the top highlight disappears.
- **Disabled:** used only where the spec requires; paper at 55% with the reason in words beside it.
- **Loading:** a short line of text ("Sending…"), never a spinner alone.
- **New quote arriving:** a paper strip at the top of the list, "1 new quote received · Show", without moving the page.
- **Motion:** fades of 150 to 200 ms at most; none with `prefers-reduced-motion`.

## 8. Where each surface is used

| Surface | Wall | Paper |
|---|---|---|
| Home, how it works, help, business page, terms and policies | page | example card; long legal text sits on a paper sheet for reading |
| Web chat | page | each message from Ask Hava on paper; hers in brass outline |
| Her quote page | page and intro | every quote card |
| Card approval, photo step, pick review, code entry | title area | the whole form or card |
| Business portal and owner portal | header band only | the working area (tables, forms, cards), for dense reading |
| Emails | not used (many email apps drop background images) | cream background `#F3ECE0`, `ink` text, a solid `#B8935A` button with `on-brass` label, Georgia for headings |
| Texts | not applicable | |

## 9. Checks before a page ships

- every pairing above passes; a new one is computed and added here
- 200% zoom and 320 px width: nothing clipped, no sideways scroll
- every control reachable by keyboard with the visible ring
- the texture never sits under small text without the contrast check (the lightest plaster patch is the test value)
- no element without a job: no badges, no gradients beyond the two wall-washes, no shadows beyond section 4
