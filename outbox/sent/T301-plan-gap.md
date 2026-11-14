---
id: T301-plan-gap
title: T301: plan gap reported, blocked for a plan fix
kind: blocked
---
T301's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: kit/tasks.json:18887 (T301 goal). Serving every ActionKind's POST so it "acts once" needs code in crates T301 does not own (picks, businesses, quotes, T261's Confirm page) and cannot fit the 3-file, 600-line budget (prompt line 52). Reviewer confirmed: Away and Busy have no storage (picks::api::away_until/away_for only compute times, no migration column); Confirm needs a Verify code (UseBy::LinkAndCode, app/portals/src/action_links.rs:117) and its page is T261's (docs/architecture.md:539-540, not a T301 dependency); no non-test app code builds QuoteRules (app/quotes/src/intake.rs:245); T399 (kit/tasks.json:10489) says T301's Approve POST only uses up the link, contradicting "acts once" for every kind. Hold (picks.window_extension via extend_once) and Pause/Resume (businesses::api::pause_self/resume_self) and PeriodJob (payments::api::resolve_fee_facts) do have production paths. Smaller issue: quote_form_page and quote_form_page_warned render no price fields; only quote_form_page_priced does (app/portals/templates/business_actions.html:83). Suggested fix: narrow T301's acts-once to Quote, Hold, Pause, Resume, Link this phone, Ask about a charge; Confirm to T261, Approve to T399, Away/Busy to a new task owning storage; add a task or dependency building QuoteRules from the stored job; log in docs/decisions.md. No code was changed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T301 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T301 --reason "..."` brings it back with the attempts it has left (D-2859).
