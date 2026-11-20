---
id: T684-plan-gap
title: T684: plan gap reported, blocked for a plan fix
kind: blocked
---
T684's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/quotes/src/models.rs:265 `card_sent_at` is pub(crate); T684 owns no app/quotes/src file. quotes::api has no per-business card send-time reader (open_card_jobs returns job ids only; spot_check_cards is a random sample); picks may not query quotes.* (R8, docs/brains/rust-production.md:44). Smallest plan change: new app/quotes/src/open_cards.rs with open_cards(db, business, now) plus one mod line in lib.rs and one pub use in api.rs; raise budget to 4 files/~600 lines. Also: portals link trait implemented in server lib.rs; email recipient is member mailbox with subject ids starting with business id; no member-language API (Spanish); Notice::OpenCards needs own EN/ES words. Reviewer confirmed (GAP-VERDICT: CONFIRMED).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T684 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T684 --reason "..."` brings it back with the attempts it has left (D-2859).
