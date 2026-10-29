# Brain: accessible forms, pages, emails and texts (WCAG 2.2 AA)

- **Purpose:** how to build Ask Hava's pages, forms, emails and texts so disabled people can use them, with the exact rules behind A11Y-* and WEB-*, the pitfalls of the plaster-and-brass palette, and what the automated `Ally` check cannot see.
- **Who reads it:** any session building or reviewing `site`, `portals` (homeowner quote page, business portal, owner portal), `messaging` (email and text templates) or `calls`; any task whose prompt names A11Y-*, WEB-*, BIZ-038, BIZ-042, PICK-040 or `docs/a11y.md`.
- **How to use it:** read sections 1 to 3 before writing a form or page, section 4 before choosing a color, section 7 before claiming "accessible", and run the checklist in section 9 before the PR. Cite the tag (`[A1]`, list in section 10) when a design choice rests on it.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec and report the disagreement in your task report (and in `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = reasoning, not backed by a source read for this file.

## 1. The standard and who sets it

- **WCAG 2.2** is a W3C Recommendation, published 5 October 2023 [A1][A2]. W3C's Web Accessibility Initiative (WAI) writes the standard and the "Understanding" pages that explain each success criterion (SC); they are the primary source for what each rule means. A11Y-001 sets Level AA for the site and all three portals.
- 4.1.1 Parsing is "obsolete and removed from WCAG 2.2" [A2]. Don't cite it.
- Nine criteria are new in 2.2; six apply at A or AA [A2]. The five that matter most for Ask Hava are in section 2. The sixth, **3.2.6 Consistent Help (A)**, needs help in "the same relative order" on every page [A2]: A11Y-017 already covers it (the "Get help" link of BIZ-047 and the help page link of WEB-008 must sit in the same place in the header or footer of every page).

## 2. The WCAG 2.2 additions that bite here

| SC (level) | What it requires | Where Ask Hava meets it | Watch for |
|---|---|---|---|
| **2.4.11 Focus Not Obscured (Minimum), AA** | "When a user interface component receives keyboard focus, the component is not entirely hidden due to author-created content." [A3] Sticky headers, sticky footers and banners are the usual failures (failure F110); a cookie-style banner fails if it fully hides a focused item [A3]. | A11Y-005 ("visible focus never hidden behind sticky headers"). | The WEB-007 status banner and any sticky header. Use CSS `scroll-padding-top` (and `scroll-padding-bottom` for a sticky footer) equal to the bar's height: technique C43 [A3]. Partial overlap passes AA but W3C recommends reducing it [A3]. |
| **2.5.7 Dragging Movements, AA** | Any function done by dragging "can be achieved by a single pointer without dragging, unless dragging is essential" [A2][A7]. | A11Y-006 ("nothing needs dragging"). | Photo upload must be a plain `<input type="file">` (a drop zone may be added on top, never instead) [inferred from A7, which does not discuss drop zones]. No drag-to-reorder, no slider without a tap-on-track or number box [A7]. |
| **2.5.8 Target Size (Minimum), AA** | Targets at least 24 by 24 CSS px, unless spaced so a 24 px circle centred on each doesn't touch another target, or an equivalent control exists, or the target is inline in a sentence, or the browser draws it unchanged, or the size is essential [A4]. | A11Y-006. | Inline links in a sentence are exempt [A4]; links in a row (footer, "Go back" beside "Confirm") are not. Checkboxes and radios restyled by CSS lose the "user agent control" exception [A4]: make the whole label clickable. In emails, make action links buttons of at least 44 px height [inferred: a margin over 24 px for thumbs on a phone]. |
| **3.3.7 Redundant Entry, A** | Information the user already gave in the same process is "auto-populated, or" "available for the user to select", except when re-entry is essential, needed for security, or the old value is no longer valid [A5]. It covers one process, not storage between visits, and browser autofill doesn't count: the site must supply the value [A5]. A failed payment keeps what was typed [A5]. | A11Y-012; PICK-040's "Same as last time" pre-fill; BIZ-049's pre-ticked checklist. | Sign-up must not ask for the email or phone again on a later step (BIZ-040 verification shows the address, not a blank box). After an error, every field keeps its value [G2][G3]. |
| **3.3.8 Accessible Authentication (Minimum), AA** | No step of sign-in may require a "cognitive function test" (remembering or transcribing) unless an alternative or a helping mechanism such as paste or a password manager is offered [A6]. Email-link sign-in is a listed sufficient technique (G218) [A6]. "A service that requires *manual* transcription of a verification code is not compliant"; blocking paste or blocking password managers fails [A6]. | A11Y-013 ("no memory or puzzle test, and pasting and autofill work"); SEC-004 sign-in by link or code. | Pick codes (PICK-029), sign-in codes and Confirm codes: one `<input>` with `autocomplete="one-time-code"`, `inputmode="numeric"`, paste allowed. Avoid six separate digit boxes; if a design insists, a paste into the first box must fill all six [inferred from A6]. No CAPTCHA anywhere [A6]. |

## 3. Form patterns (GOV.UK Design System, Roselli, Soueidan)

The GOV.UK Design System is built and user-researched by the UK Government Digital Service for services every resident must be able to use, including people with low literacy and assistive technology; it is the most-tested public pattern library for plain, server-rendered forms [G1]-[G5]. Adrian Roselli is an accessibility consultant whose posts are widely cited in WCAG discussions, and Sara Soueidan is an inclusive design engineer who teaches accessible front-end practice [credibility notes inferred].

### 3.1 One thing per page

- Split a form so each page holds "one piece of information", "one decision" or "one question"; it helps people "understand what you're asking them to do", focus, use a phone and "recover from form errors" [G4].
- **Ask Hava:** the web chat (INT-001, WEB-009, TXT-034) works without JavaScript (WEB-001), so build each chat turn as a server-rendered page: the transcript so far, then one question, one answer field and one submit button. That is one-thing-per-page with the history shown above it [inferred]. Business sign-up (BIZ-021, BIZ-049) follows the same shape: categories, license number, checklist, towns, largest job, each its own page.
- The homeowner's card preview before sending (INT-041) is Ask Hava's "check your answers" step; let her change any answer from it and return to it.

### 3.2 Labels, hints and accessible names

- Every field has a visible `<label for>` matched to the input's `id`. Roselli's order of preference: native HTML label first, then `aria-labelledby` pointing at visible text, then visually hidden text, then `aria-label` last [R1]. A native label gives "a larger hit area" and works for voice control; `aria-label` can't be selected by voice users and isn't reliably translated [R1]. Ask Hava ships Spanish pages (WEB-002), so this matters.
- A button's or link's visible words must be part of its accessible name (SC 2.5.3 Label in Name, cited in [R1]'s discussion). Never give "Confirm job 1042" an `aria-label` of "Submit".
- Hint text sits between the label and the input, is "a few words only; ideally, one sentence" and holds no links [G6]; connect it with `aria-describedby` [inferred: standard GOV.UK markup, the component page was not read for this file]. Never use `placeholder` as the label or the hint [inferred].
- Use `autocomplete` tokens on personal fields (`given-name`, `email`, `tel`, `street-address`, `postal-code`): SC 1.3.5 Identify Input Purpose [A9]. This also helps 3.3.7.

### 3.3 Errors: validate on submit, on the server

- **When:** "Do not validate when the user moves away from a field"; validate when they try to continue [G3]. "You'll always need to carry out server side validation" [G3]. Add `novalidate` to forms and don't add the `required` attribute, so browser bubbles never replace the designed messages [G3].
- **The page after a failed submit:** prefix the `<title>` with "Error: " ("Error: " in Spanish too) so screen readers hear it first [G1][G3]; show the error summary at the top of `<main>`, above the `h1`, headed "There is a problem", with focus moved to it and each message a link to its field (to the first radio or checkbox of a group) [G1]; show the same words inline after the label and hint, with a visually hidden "Error:" prefix [G2]; keep every value the person entered, right or wrong [G2][G3].
- **Focus without JavaScript:** GOV.UK moves focus with its script [G1]. Ask Hava's pages must work without it (WEB-001): render the summary with `tabindex="-1"` and the `role="alert"` container, and add an `autofocus` attribute on the summary container when it is present [inferred: verify with VoiceOver in the section 7 script].
- **Wording:** say what went wrong and how to fix it, using the question's words ("Enter your ZIP code", "ZIP code must be 5 numbers"); never "invalid", "forbidden", "please", "sorry" or "oops"; never "This field is required" [G2]. Eligibility problems (a trade not covered, a town outside the area) get their own page, not a field error [G2]: INT-017's "We don't cover ... yet" is such a page.
- **Don't disable submit buttons.** "The place where buttons should *not* be disabled is within the context of submitting a form"; let people submit and show the errors, and handle double submits on the server [R2]. Ask Hava's server already needs idempotent Confirm and quote posts (FEE-016, FEE-037); never rely on a disabled button for that.

### 3.4 Live regions, status and focus after an action

- A live region must be in the DOM before its content changes: "place the live region container in the DOM as early as possible" [S1]. Use `role="status"` (polite) for new quotes and `role="alert"` (assertive) only for errors and time-critical warnings; "Assertive notifications are very disruptive" [S1].
- Prefer persistent content over announcements: "if you can use another more persistent approach, you almost definitely should" [S1]. Support for `aria-atomic`, `aria-relevant` and `aria-busy` is inconsistent: test, don't trust [S1].
- **Ask Hava:** A11Y-014 (new quotes announced politely, focus to the result heading after a pick) and A11Y-018 (status banner and alerts). Put the empty `role="status"` element in the server-rendered quote page; on a refresh or script update, write one short line per batch ("2 new quotes. 5 quotes in total.") rather than one per quote [inferred]. The quote list itself is the persistent record.

### 3.5 Focus indicators

- Never remove the focus outline without replacing it (SC 2.4.7) [S2]. Use `outline` with `outline-offset`: it doesn't shift layout and survives forced-colors mode [S2]. Use `:focus-visible` so mouse users don't see the ring [S2].
- The ring needs 3:1 against the colors next to it (SC 1.4.11) [S2][A8]. On mixed surfaces use a two-tone ring (a dark outline plus a light shadow, or the reverse) [S2].
- **Ask Hava:** DESIGN_SPEC section 2 sets a 3 px ring with 3 px offset: `#E2C48F` on the wall (9.38:1 on `#232322`) and `#8A6A2E` on paper (4.28:1 on `#F3ECE0`, 3.84:1 on the darkest fibre `#E9E0D0`; computed for this file). Both pass 3:1. A brass button on paper sits next to paper, so the ring around it must use the paper color `#8A6A2E`, not the wall color, which is 1.43:1 on paper [computed].

## 4. Color contrast for the plaster-and-brass palette

Rules: text 4.5:1, large text (24 px, or 18.67 px bold) 3:1, and UI component edges and focus rings 3:1 against adjacent colors [W1][A8]. Ratios can't be rounded up: 4.47:1 fails [W1]. Over images, textures or gradients, test the least-contrasting area [W1][A8]: that is why WEB-027 tests the lightest plaster patch (`#45433F`) and the darkest brass pixel.

Ratios below were computed for this file with the WCAG relative-luminance formula [A1]:

| Pair | Ratio | Verdict |
|---|---|---|
| `on-wall` `#F1EBE0` on lightest plaster `#45433F` | 8.32 | Pass |
| `brass-text` `#E2C48F` on lightest plaster `#45433F` | 5.89 | Pass (the only brass allowed as text on the wall) |
| `brass` fill `#B8935A` used as **text** on `#232322` / on `#45433F` | 5.51 / **3.46** | **Fails on the light patches.** Never use the fill color for text on the wall. |
| `on-brass` `#1B1915` on `brass` `#B8935A` | 6.15 | Pass (WEB-027 also checks the darkest pixel of the texture) |
| White or `on-wall` on `brass` | 2.85 / 2.41 | **Fail.** Never put light text on brass. |
| `brass-text` `#E2C48F` or `brass` on paper `#F3ECE0` | 1.43 / 2.43 | **Fail.** Brass is never text on paper; links on paper are `paper-link` `#6B4A1E` (6.82), underlined. |
| `paper-line` `#D9CCB6` on paper | 1.35 | Decorative only. **Never as an input border.** |
| Error text `#8C291D` on paper | 7.30 | Pass |
| `alert` border `#C9786A` on wall | 4.78 | Pass for a component edge |

- **Input borders on paper:** a text box whose border is what shows it's a field needs 3:1 [A8] (W3C's Figure 45 fails a light-grey border). `paper-line` cannot be that border. Use `ink-soft` `#4F4A43` (7.5:1 per DESIGN_SPEC) or another pair checked first.
- **Buttons:** a text button doesn't need a 3:1 boundary when its text identifies it [A8], so the brass button on paper passes 1.4.11 through its label. Make sure the label itself passes (it does, at 6.15).
- **Color alone never carries meaning** (SC 1.4.1) [A1]: errors also get the word "Error:" and the inline message; quote status ("Picked", "Can't take it") is written, not just tinted.
- Disabled controls are exempt from contrast [W1]. That's one more reason not to disable form controls (3.3).

## 5. Plain language and reading level

- plainlanguage.gov, the US federal plain-language site, was retired and now redirects to digital.gov; its content is archived in a GitHub repository and the Internet Archive [P3]. The current federal guidance is the digital.gov plain-language guide [P1]; the 2011 Federal Plain Language Guidelines survive in the University of North Texas web archive [P4].
- Principles: "write for your audience"; use active voice; put the summary first; use lists and tables [P1]. "Express only one idea in each sentence"; break exceptions into their own sentences [P2]. The guide does not set a grade level: "Only write for 8th graders if your audience is, in fact, an 8th-grade class" [P1].
- **Ask Hava's bar is its own decision:** `scripts/check_copy.py` fails homeowner copy above Flesch-Kincaid grade 8 and business copy above grade 9; the voice guide asks for sentences of 15 words or fewer (WORD-024, `docs/voice.md`). Readability formulas only count word and sentence length; they don't prove clarity [inferred]. The cold read (WORD-028) and copy panel (WORD-029) are the clarity check.
- Error messages, hints, button labels and link text are copy too: they go through the same checks.

## 6. Texts, emails, relay users and progressive enhancement

**Texts.**
- Screen readers read each emoji by its Unicode name, "which ... may not match your intended meaning"; never use an emoji for a word, as a bullet, or mid-word [E1]. TXT-005 already bans emoji-only meaning; WORD-024's voice guide bans emoji outright.
- Show full links (TXT-005) so a screen reader user hears where the link goes; keep the link on its own at the end of the text [inferred].
- Deaf, hard-of-hearing and speech-disabled people reach phone lines through Telecommunications Relay Service; 711 connects voice and TTY relay from any US phone, but not video relay, IP Relay or IP captioned calls, which dial the number directly with an assistant joined automatically [F1]. CALL-013 makes the masked line work with 711; A11Y-020 tells relay users to reach Ask Hava by text or email. A text-first service is already relay-friendly [inferred].

**Emails** (A11Y-019, A11Y-023, A11Y-024).
- Real text, not images; a plain-text part; link text that names the action and the job ("Confirm job 1042"), never "click here".
- Set `lang="en"` or `lang="es"` on the HTML email's root, use real headings, and keep tables only for layout with `role="presentation"` [inferred: email clients vary; check in the section 7 script].
- Every action reached from an email opens a page that says what it will do before it does it (SEC-026, A11Y-023). That page is a normal form page and follows section 3.

**Progressive enhancement** (WEB-001, BIZ-038).
- Build the core task in HTML; CSS next; JavaScript only enhances, because "The JavaScript layer is not fault-tolerant": network errors, blockers, firewalls and browser bugs stop it [G5]. Don't build single-page apps: they break focus handling, back and forward, and error recovery [G5].
- Ask Hava's third-party exception is Stripe Elements on the Confirm page (FEE-036): an iframe Ask Hava doesn't control. The VoiceOver script (A11Y-022) must cover it; the page around it must still say the fee, the refund rule and "Go back" in plain HTML (PICK-021).

## 7. What axe can test, and what the VoiceOver script must cover

- **Deque** makes axe-core, the open-source rules engine Ask Hava's `Ally` check runs (A11Y-002). Deque reports axe finds "on average 57% of WCAG issues automatically", from data on over 2,000 audits and 13,000 pages; that's a share of issue volume, not of success criteria, and it's a vendor's own figure [D1][D2]. axe aims for "zero false positives" and marks what it can't decide as "incomplete" (needs review) [D1].
- **The gap this leaves:** A11Y-002 fails only on violations. "Incomplete" results don't fail the build. Text over the plaster texture is the likely case: axe usually can't compute contrast over a background image and reports it as needing review [inferred: documented axe behavior, not read for this file]. WEB-027's own pixel test closes that one gap; the others need a person.
- **Automated checks can't judge:** whether link text and error messages make sense; whether focus order is logical and focus moves to the right place after a submit or a pick; whether focus is hidden by the sticky header at each scroll position (2.4.11); whether a live region is actually spoken; whether a CAPTCHA-free, paste-friendly code field works with autofill (3.3.8); whether instructions come before the inputs; emails; texts; Stripe's iframe [inferred from D1, D2 and A3 to A6].
- **Who uses what:** 91.3% of screen-reader users in WebAIM's 2023-24 survey use one on a phone, 70.6% of those VoiceOver; 71.6% explore a page by headings first [W2]. So the human script runs on an iPhone with VoiceOver and checks headings first. WebAIM (a non-profit at Utah State University) runs the longest-standing survey of screen-reader users [W2].
- **The Gate B VoiceOver script (`docs/a11y.md`, A11Y-022) must cover, at minimum:**
  1. Rotor set to Headings on every page: one `h1`, headings in order [W3].
  2. The web chat from first message to card preview, swiping only; each question read once with its label and hint.
  3. A failed submit: "Error:" heard in the title, focus on the summary, each summary link moves to its field, values kept.
  4. A pick code pasted from Messages or Mail; autofill offered; no forced re-typing.
  5. The quote page receiving a new quote: the polite status line is spoken once, not repeated.
  6. A pick, then focus on the result heading (A11Y-014).
  7. Business card entry in Stripe Elements, and an error inside it.
  8. Opening a pick email in iPhone Mail, swiping its links, opening Confirm and Hold, and hearing what each will do before doing it (A11Y-023).
  9. Two-finger swipe to read the full page, at 200% text size (Dynamic Type) and in Dark Mode [inferred additions].
  10. Each step again in Spanish for one page (`lang="es"` changes the voice) (A11Y-004).

## 8. Requirement map and proposed requirements

| Topic | Covered by | Gap |
|---|---|---|
| WCAG 2.2 AA everywhere | A11Y-001, A11Y-002 | axe "incomplete" results don't fail (P-A11Y-1) |
| Focus not obscured | A11Y-005 | method not named (P-A11Y-2) |
| Target size, dragging | A11Y-006 | file upload fallback (P-A11Y-3) |
| Contrast | A11Y-007, WEB-027 | input borders on paper (P-A11Y-4) |
| Labels, hints | A11Y-010 | label method, no placeholder labels (P-A11Y-5) |
| Errors | A11Y-011 | title prefix, server validation, kept values, no disabled submit (P-A11Y-6, P-A11Y-7) |
| Redundant entry | A11Y-012, PICK-040, BIZ-049 | none |
| Authentication | A11Y-013, SEC-004 | code field markup (P-A11Y-8) |
| Live regions | A11Y-014, A11Y-018 | region present at load, batched (P-A11Y-9) |
| Consistent help | A11Y-017, BIZ-047, WEB-008 | none |
| Emails | A11Y-019, A11Y-023, A11Y-024 | `lang` on HTML email (P-A11Y-10) |
| Texts, relay | TXT-005, CALL-013, A11Y-020 | none |
| No-JS pages | WEB-001, BIZ-038 | web chat shape (P-A11Y-11) |
| Human test | A11Y-021, A11Y-022 | script contents (section 7) |

**Proposed requirement sentences** (for Casey's decision; not yet in the spec):
- **P-A11Y-1** Every axe-core "incomplete" (needs review) result on any page and state is listed in the `Ally` job's report, and a page with an unreviewed incomplete result older than one release fails `Ally`.
- **P-A11Y-2** Every page with a sticky header, footer or the WEB-007 banner sets `scroll-padding-top` (and `scroll-padding-bottom`) to the bar's height, and a Playwright test tabs through each page and fails if any focused element is fully covered.
- **P-A11Y-3** Every photo upload is a native file input with a visible label; any drag-and-drop area is an addition to it, never a replacement.
- **P-A11Y-4** Every text input, select and textarea on paper has a border of at least 3:1 against the paper behind it, and the WEB-027 pair test includes input borders and the focus ring against every surface it can sit on.
- **P-A11Y-5** Every form field is named by a visible `<label for>`; `aria-label` is used only where no visible text exists, and `placeholder` never carries a label or hint.
- **P-A11Y-6** Every form posts to the server with `novalidate` and no `required` attribute; after a failed submit the page title begins "Error: ", the error summary is the first thing in `<main>` and gets focus without JavaScript, each message links to its field, and every entered value is kept.
- **P-A11Y-7** No form's submit button is ever disabled; double submits are refused or ignored by the server.
- **P-A11Y-8** Every one-time code field (pick, sign-in, Confirm, identity) is a single input with `autocomplete="one-time-code"` and `inputmode="numeric"` that accepts pasting.
- **P-A11Y-9** The quote page's `role="status"` element is present, empty, in the first server render, and each update writes one short summary line, never one line per quote.
- **P-A11Y-10** Every HTML email sets `lang` on its root element to the recipient's language.
- **P-A11Y-11** The web chat works without JavaScript as one question per server-rendered page, with the conversation so far shown above the current question.

## 9. Review checklist (run before the PR)

- [ ] One `h1`, landmarks, skip link, `lang` set; Spanish passages carry their own `lang` (A11Y-003, A11Y-004).
- [ ] Every field: visible `<label for>`, hint between label and input, `autocomplete` where personal.
- [ ] Failed submit: "Error: " title, summary at top with focus and links, inline messages match the summary, values kept, no browser bubbles.
- [ ] No disabled submit buttons; server handles double submits.
- [ ] Code fields accept paste and autofill; no CAPTCHA.
- [ ] Nothing needs dragging; file input present; targets 24 px or spaced; email buttons tall.
- [ ] Tab through the page with the sticky header present: no focused item fully hidden.
- [ ] Every new color pair checked: no brass text on paper, no light text on brass, no `paper-line` input borders, focus ring 3:1 on every surface.
- [ ] Live region present at load; polite unless an error.
- [ ] Copy passes `scripts/check_copy.py` (grade 8 homeowners, 9 businesses) and error messages say what to do.
- [ ] Emails: plain-text part, `lang`, action-and-job link text, no images of text. Texts: no emoji, full links.
- [ ] Page works with JavaScript off.
- [ ] axe clean at 320, 768 and 1280 px; "incomplete" results read and noted in the PR.
- [ ] If the change touches a flow in section 7, the VoiceOver script was updated.

## 10. Sources

Standards (W3C WAI, the body that writes WCAG):
- [A1] WCAG 2.2 Recommendation, 5 October 2023: https://www.w3.org/TR/2023/REC-WCAG22-20231005/ (latest: https://w3.org/TR/WCAG/)
- [A2] What's New in WCAG 2.2: https://w3.org/WAI/standards-guidelines/wcag/new-in-22/
- [A3] Understanding SC 2.4.11 Focus Not Obscured: https://www.w3.org/WAI/WCAG22/Understanding/focus-not-obscured.html
- [A4] Understanding SC 2.5.8 Target Size (Minimum): https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html
- [A5] Understanding SC 3.3.7 Redundant Entry: https://www.w3.org/WAI/WCAG22/Understanding/redundant-entry.html
- [A6] Understanding SC 3.3.8 Accessible Authentication (Minimum): https://www.w3.org/WAI/WCAG22/Understanding/accessible-authentication.html
- [A7] Understanding SC 2.5.7 Dragging Movements: https://www.w3.org/WAI/WCAG22/Understanding/dragging-movements
- [A8] Understanding SC 1.4.11 Non-text Contrast: https://w3.org/WAI/WCAG22/Understanding/non-text-contrast
- [A9] Understanding SC 1.3.5 Identify Input Purpose (located, not re-read for this file): https://www.w3.org/WAI/WCAG21/Understanding/identify-input-purpose

GOV.UK (UK Government Digital Service, user-researched public patterns):
- [G1] Error summary: https://design-system.service.gov.uk/components/error-summary
- [G2] Error message: https://design-system.service.gov.uk/components/error-message
- [G3] Validation pattern: https://design-system.service.gov.uk/patterns/validation
- [G4] Service Manual, structuring forms: https://www.gov.uk/service-manual/design/form-structure
- [G5] Service Manual, building a robust frontend using progressive enhancement: https://www.gov.uk/service-manual/making-software/progressive-enhancement
- [G6] DWP Design System, hint text (built on the GOV.UK Design System): https://design-system.dwp.gov.uk/guidance/hint-text/

Practitioners:
- [R1] Adrian Roselli, "My Priority of Methods for Labeling a Control": https://adrianroselli.com/2020/01/my-priority-of-methods-for-labeling-a-control.html
- [R2] Adrian Roselli, "Don't Disable Form Controls" (2024, updated 2026): https://adrianroselli.com/2024/02/dont-disable-form-controls.html
- [S1] Sara Soueidan, "Accessible notifications with ARIA Live Regions (Part 1)": https://www.sarasoueidan.com/blog/accessible-notifications-with-aria-live-regions-part-1/
- [S2] Sara Soueidan, "A guide to designing accessible, WCAG-conformant focus indicators": https://sarasoueidan.com/blog/focus-indicators/

Testing (Deque, maker of axe-core; WebAIM, Utah State University non-profit):
- [D1] axe-core README: https://github.com/dequelabs/axe-core
- [D2] Deque, "Automated testing study identifies 57 percent of digital accessibility issues" (2021): https://www.deque.com/blog/automated-testing-study-identifies-57-percent-of-digital-accessibility-issues
- [W1] WebAIM, Contrast and Color Accessibility: https://webaim.org/articles/contrast/
- [W2] WebAIM, Screen Reader User Survey #10 Results (Dec 2023 to Jan 2024, 1,539 respondents): https://webaim.org/blog/screen-reader-user-survey-10-results/
- [W3] WebAIM, Using VoiceOver to Evaluate Web Accessibility (iOS): https://webaim.org/articles/voiceover/mobile

Plain language, relay, emoji:
- [P1] digital.gov Plain Language Guide, principles: https://digital.gov/guides/plain-language/principles
- [P2] digital.gov Plain Language Guide, clear and short: https://digital.gov/guides/plain-language/writing/clear-short
- [P3] VisibleThread, "What happened to PlainLanguage.gov" (Oct 2025, secondary): https://www.visiblethread.com/blog/what-happened-to-plainlanguage-gov-why-it-matters-for-federal-contracting/
- [P4] Federal Plain Language Guidelines (archived copy): https://webarchive.library.unt.edu/web/20121006190813mp_/http:/www.plainlanguage.gov/howto/guidelines/FederalPLGuidelines/TOC.cfm
- [F1] FCC, 711 for Telecommunications Relay Service: https://www.fcc.gov/node/23845
- [E1] Studio 24, accessibility guide to emojis (secondary): https://www.studio24.net/guides/accessibility/emojis

Not reachable for this session: none. Contrast ratios in section 4 were computed in this session from DESIGN_SPEC hex values with the WCAG formula.
