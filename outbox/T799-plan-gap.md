---
id: T799-plan-gap
title: T799: plan gap reported, blocked for a plan fix
kind: blocked
---
T799's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: contradiction: D-5401 requires es.json text "dentro de 72 horas de recibir los datos del cliente" (5 keys), but tests/docs/test_introduction_fee_wording.py:60 forbids the pattern `\bdatos de(l)? clientes?\b` (WORD-026); test_td_word_10[app/messaging/templates/es.json] fails with 4 hits (the negated use in unreachable_no_attempts is allowed). That test is protected and not owned by T799. Reviewer (a4e70d1f2bbf42da0) confirmed on head a34a900bb2a5. Fix: change the Spanish phrase (e.g. "datos del propietario", recount segments) or add an exemption via a harness task. All other changes matched the goal; messaging, integration, intake tests, check_copy and check_ai_smell passed; smoke.sh was not run.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T799 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T799 --reason "..."` brings it back with the attempts it has left (D-2859).
