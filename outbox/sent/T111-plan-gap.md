---
id: T111-plan-gap
title: T111: plan gap reported, blocked for a plan fix
kind: blocked
---
T111's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: scripts/check_legal_docs.py:403-410 and :458 contradict TERMS-026 (docs/spec/terms.md:29) and D-3877 (kit/prompts/T111.md:9). The Spanish summary must carry the TERMS-026 paragraph in English word for word, which contains "subcontract"; check_legal_docs.py bans \bsubcontract\w* (BANNED :307, trace_spec.py:69) in every DOCUMENTS file and exempts it only through that file's own REQUIRED phrases (check_phrases :393-412, scan in run() :442-464). The new business-terms-summary-es.md has no REQUIRED entry, and the prompt allows no script line beyond the DOCUMENTS/BUSINESS_DOCUMENTS additions. Observed: "ERROR business-terms-summary-es.md: WORD-022/WORD-023 wording: 'subcontract'", Errors: 1. Proposed plan fix: let T111 add REQUIRED["business-terms-summary-es.md"] with the WORD-002 and TERMS-026 tuples business-terms.md already uses (~5 lines). Reviewer confirmed, and noted trace_spec.py:77 also bans \bsubcontrat\w*, so the Spanish draft must avoid "subcontrato" or get its own exemption. Also noted: TD-TERMS-05 says 4 days (D-223) but TERMS-022 says 2 days (D-371); drafts follow 2 days.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T111 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T111 --reason "..."` brings it back with the attempts it has left (D-2859).
