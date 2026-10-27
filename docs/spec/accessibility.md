# Spec: Accessibility (WCAG 2.2 AA)

Prefix: `A11Y` · Modules: `site`, `portals`, `messaging`, `calls` · Format and checks: [index.md](index.md)

- **A11Y-001** The site and all three portals meet WCAG 2.2 Level AA. Source: `BUILD_BRIEF.md:384`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:151`.
- **A11Y-002** axe-core runs on every page and state at 320, 768 and 1280px, and any violation fails the `Ally` check. Source: `BUILD_BRIEF.md:424`.
- **A11Y-003** Every page has one `h1`, landmarks and a skip link, using semantic HTML first and ARIA only where HTML can't do it. Source: `BUILD_BRIEF.md:387`.
- **A11Y-004** Every page has `lang="en"` or `lang="es"`, and any inline passage in the other language has its own `lang`. Source: `BUILD_BRIEF.md:388`.
- **A11Y-005** Every page works by keyboard alone, with visible focus never hidden behind sticky headers, a logical order and no keyboard traps. Source: `BUILD_BRIEF.md:389`.
- **A11Y-006** Targets are at least 24×24 CSS px, and nothing needs dragging or complex gestures. Source: `BUILD_BRIEF.md:390`.
- **A11Y-007** Text contrast is at least 4.5:1, and large text and UI parts at least 3:1. Source: `BUILD_BRIEF.md:391`.
- **A11Y-008** Pages work at 320px wide and at 400% zoom without sideways scrolling, except data tables, which scroll inside a focusable region with `tabindex="0"`, `role="region"` and an `aria-label`. Source: `BUILD_BRIEF.md:392`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:148`.
- **A11Y-009** Text spacing can be overridden without breaking the layout, and motion is reduced for users who ask for it. Source: `BUILD_BRIEF.md:393`.
- **A11Y-010** Form fields have visible labels with instructions before the inputs. Source: `BUILD_BRIEF.md:395`, `BUILD_BRIEF.md:396`.
- **A11Y-011** Form errors are named in text, linked from an error summary and announced. Source: `BUILD_BRIEF.md:397`.
- **A11Y-012** No form asks again for something the user already entered. Source: `BUILD_BRIEF.md:398`.
- **A11Y-013** Sign-in needs no memory or puzzle test, and pasting and autofill work. Source: `BUILD_BRIEF.md:399`.
- **A11Y-014** New quotes arriving are announced politely with `aria-live`, and focus moves to the result heading after a pick. Source: `BUILD_BRIEF.md:400`.
- **A11Y-015** Time limits exist only for pick codes, sign-in links and owner sessions; each warns before expiring, offers a one-step way to get a new code or extend, and keeps what was entered. Source: `BUILD_BRIEF.md:402`, `BUILD_BRIEF.md:403`, `BUILD_BRIEF.md:404`.
- **A11Y-016** Quote windows are shown as plain text, never as countdown timers. Source: `BUILD_BRIEF.md:405`.
- **A11Y-017** Help is in the same place on every page. Source: `BUILD_BRIEF.md:411`.
- **A11Y-018** The status banner and alerts use `role="status"` or `role="alert"` as appropriate. Source: `BUILD_BRIEF.md:412`.
- **A11Y-019** Emails are real text rather than images, include a plain-text version, and use meaningful link text. Source: `BUILD_BRIEF.md:419`.
- **A11Y-020** An accessibility statement page gives the conformance target, known limits, and a phone and email contact answered within 2 business days. Source: `BUILD_BRIEF.md:421`.
- **A11Y-021** Playwright keyboard-only journeys cover all four end-to-end flows, and automated reflow and zoom checks run. Source: `BUILD_BRIEF.md:425`, `BUILD_BRIEF.md:426`.
- **A11Y-022** `docs/a11y.md` holds the Gate B VoiceOver script covering the homeowner flow and the business card-entry step, including Stripe's embedded card form. Source: `BUILD_BRIEF.md:427`.
