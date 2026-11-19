---
id: T203-plan-gap
title: T203: plan gap reported, blocked for a plan fix
kind: blocked
---
T203's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/templates/es_business_signup.html:82 ("Licencia de control de plagas") and :88 ("Mi licencia incluye la categoría de organismos que destruyen la madera (termitas)") are flagged by the WEB-010 pest scan (tests/docs/test_wording_statements.py:385, ['plagas', 'termitas']), which checks against the closed List A of D-4258 rule (1). Rule (5) of kit/prompts/T203.md:9 says the worker "stops with PLAN GAP: <file:line> and the rule and adds no exemption of its own". The lines are the Spanish twins (same {% if pest %} pest-attestation fieldset, T488) of app/portals/templates/business_signup.html:90 ("Pest control license") and :96 ("My license includes the wood-destroying organism (termite) category"), already on List A; they are a license attestation, not pest advertising. Plan fix: add the two Spanish lines word for word to T203's D-4258 rule (1) List A. Evidence: pytest tests/docs/test_wording_statements.py -> 1 failed, 220 passed (the es_business_signup.html case). The worker's own exemption (a0c92e258) was reverted in 49e0ad77e per rule (5); re-applying it passes once List A is amended. Reviewer acc0244933ca848f7 confirmed: PLAN GAP CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T203 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T203 --reason "..."` brings it back with the attempts it has left (D-2859).
