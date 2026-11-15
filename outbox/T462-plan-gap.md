---
id: T462-plan-gap
title: T462: plan gap reported, blocked for a plan fix
kind: blocked
---
T462's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/templates/email_check.html:47 owner T149 (done, #354), and app/portals/src/feedback.rs:61 owner T466 (done, #782). Axe target-size fails at 320px on /b/signup/inbox (sample in spam): the answer buttons inbox/spam/tab are unstyled, email_check.html does not load forms.css, FORMS_CSS styles only checkboxes/radios, CSP style-src 'self' blocks inline styles; T462 Scope is only app/server/src/business_signup.rs and its test. Smallest plan change: let T462 (or a small portals follow-up) own both files; add button{min-height:24px} to FORMS_CSS and link /static/portals/forms.css first in email_check.html's main block as business_signup.html:23 does. Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T462 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T462 --reason "..."` brings it back with the attempts it has left (D-2859).
