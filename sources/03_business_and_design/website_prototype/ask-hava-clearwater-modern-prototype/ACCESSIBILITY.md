# Accessibility — WCAG 2.2 Level AA target

This is a **target, not a certification**. Only the tests listed below should be considered complete; do not infer WCAG conformity from the prototype appearance.

| Topic / success criterion | Implementation | Validation status |
|---|---|---|
| 1.1.1 Non-text content | Decorative SVG and shape are hidden or given names; no stock portraits | Review manually |
| 1.3.1 Info & relationships | Semantic headings, forms with labels, tables with headers, header/nav/main/footer | Source check |
| 1.3.2 Meaningful sequence | Document-order reading and logical grid order | Keyboard review pending |
| 1.3.5 Input purpose | Common autocomplete fields where relevant | Source check |
| 1.4.3 Contrast minimum | Dark ink or teal on light background; light text on dark panels | Color checks listed in style guide; automated audit pending |
| 1.4.10 Reflow | Responsive grid stacks; explicit 320px viewport test planned | Browser test report below |
| 1.4.12 Text spacing | Flexible cards, em/rem-based text | Manual audit pending |
| 2.1.1 Keyboard | Links, select/input controls and buttons are native; `dialog` supports keyboard/Escape | Automated navigation check plus manual pending |
| 2.1.2 No keyboard trap | Native `dialog` only traps focus when open; closes with Escape | Browser test report below |
| 2.4.1 Bypass blocks | Skip-to-content on every page | Source check |
| 2.4.3 Focus order | DOM reading order; hidden inactive tab panels | Manual audit pending |
| 2.4.7 Focus visible / 2.4.11 Focus not obscured | Visible 3px orange outline and normal-flow header | Manual check pending |
| 2.5.8 Target size | Buttons ~44px or larger | Size audit pending |
| 3.2.6 Consistent help | Footer help, email and sample phone every page | Source check |
| 3.3.1 Error identification | Explicit error messages; invalid code described and focused | Partial source check |
| 3.3.7 Redundant entry | Wizard keeps answers when users go Back; demo flow | Manual audit pending |
| 3.3.8 Accessible authentication | Demo code can be pasted; `autocomplete=one-time-code`; no puzzle | Browser check planned |
| 4.1.3 Status messages | Key demo updates announce in `aria-live` regions | Source check |
| Motion | `prefers-reduced-motion` disables transitions/motion | Source check |
| 200% zoom | Responsive typography and cards | Browser test report below |
| Spanish locale | `lang=es` on Spanish pages and visible language switch | Source check; translation review pending |

**Critical limitations:** no comprehensive axe-core/Lighthouse runs or VoiceOver/NVDA audit has been claimed at file-generation time; see the actual verification section appended after tests. Browser automation is not an assistive-technology audit. Native-speaker review of Spanish, mobile tap testing, legal content review and full WCAG 2.2 AA sign-off are still required. No accessibility overlay is used.

## Verification results

See `QA.md` for static validation results, and the explicit list of checks not run due to browser restrictions.
