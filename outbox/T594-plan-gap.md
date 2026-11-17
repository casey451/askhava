---
id: T594-plan-gap
title: T594: plan gap reported, blocked for a plan fix
kind: blocked
---
T594's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/server/src/chat_codes.rs:187 and app/site/templates/request.html, both owned by T489. Smallest plan change: a T489 fix task that adds `.method(Method::POST)` to the request builder in `posted_csrf` and puts the "18 or older" line (CON-025, docs/spec/consent.md:29) on the request page, then re-run T594.
Evidence: `npx playwright test --config tests/e2e/playwright.config.ts tests/e2e/web_chat.spec.ts` exit=1, 5 failed, 2 passed. web_chat.spec.ts:137 (18 or older) fails: Expected pattern /18 or older/i not in main. web_chat.spec.ts:142, :153, :187 fail at line 94 (main form has 0 fields: chat POST refused 403, csrf_rejected events) and :219 fails at line 227 (main ol not found). posted_csrf builds Request::builder() with no method -> GET -> axum Form reads the empty query string -> csrf "" -> 403 on every same-site chat POST. Reviewer (a90cf301abc6dc1ca) confirmed: both files are T489's in kit/tasks.json; T594 (build) owns only tests/e2e/web_chat.spec.ts; PLAN GAP CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T594 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T594 --reason "..."` brings it back with the attempts it has left (D-2859).
