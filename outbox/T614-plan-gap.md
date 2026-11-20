---
id: T614-plan-gap
title: T614: plan gap reported, blocked for a plan fix
kind: blocked
---
T614's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quote page's HTML, but T614 must put <details class="menu"><summary>Menu</summary> in the header of every page (docs/design/DESIGN_SPEC.md:102); quote_page.html extends base.html. Smallest plan change: a harness task narrows the check at quote_page.rs:170-174 to the page's <main>. Also D-4970's "two rules only" is not enough: a third rule is needed in site.css: .menu > summary { margin: calc((var(--header-h) - 44px) / 2) 0 calc((var(--header-h) - 44px) / 2) -12px; } (else the row is 289px in 280px at 320px and home_first_screen.spec.ts:75 fails); plus .menu, .menu::details-content { display: contents; } and .menu-list flex: 0 0 100%; and each .site-nav link and Espanol wrapped in a <p> for unstyled axe runs (capability_setup.spec.ts:62, emergency_switch_after_hours.spec.ts:33). Work kept in /tmp/T614-attempt.patch (stash on the worker's machine). Reviewer ab0c98e06113eaf23 confirmed: PLAN-GAP: CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T614 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T614 --reason "..."` brings it back with the attempts it has left (D-2859).
