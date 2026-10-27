# Clearwater Modern — Ask Hava adaptation

**Direction:** understated lake-house hospitality meets calm local concierge. Large editorial serif headlines, generous whitespace, deep lake ink, teal CTAs, warm stone surfaces. Quietly premium but legible for people who use larger text and phones.

**Brand arrangement:** Ask Hava is the primary consumer name. “Get quotes” is an Ask Hava feature; “Ask Hava for Business” is the business entry. Existing concierge directory/events are linked externally as known site functions. Quote comparisons are explicitly neutral and never carry directory advertising.

**Fonts:** DM Serif Display for headlines, DM Sans for paragraphs/forms with system fallbacks. 18px body minimum.

**Palette:** `--ink #153342`, `--lake #086e76`, `--lake-dark #07555d`, `--cream #fbfaf6`, `--lake-light #e3f0ef`, `--sand #efe8db`, `--focus #9a4900`. All component dimensions, radii, shadows and spacing are defined in `assets/css/tokens.css`. Inspect `styleguide.html` for per-color calculated contrast ratios against white and ink. Only use pairings that meet WCAG AA text and nontext contrast requirements.

**Component inventory:** header, mobile menu, language switch, editorial hero, service tiles, steps, sample quote cards, editable forms, native `dialog` confirmation, status chips, accessible tab selections, info/safety callouts, data tables and footer.

**Do:** plainly say “Ask Hava gathers quotes; you choose.” Preserve the business's own quote wording. Show arrival order. Keep onboarding permission clear. Mark fictional content as fictional. Respect `prefers-reduced-motion`.

**Don't:** add sponsored quote sorting, star ratings, invented testimonials, claims of vetting, misleading license checks, fake urgency, people photos, countdown timers or decorative animations. Don’t blur the existing directory's ads into the quote product.

**Next build phases:** design integration with the current Ask Hava app, server APIs/auth, telecom consent audit, ROC integration, actual provider routing, real masked telephony, billing/webhooks, production security and accessibility validation. The front-end prototype does not implement these services.
