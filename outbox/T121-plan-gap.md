---
id: T121-plan-gap
title: T121: plan gap reported, blocked for a plan fix
kind: blocked
---
T121's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/server/src/owner_portal.rs, app/server/src/owner_tools.rs, app/portals/src/owner_help.rs, app/server/src/homeowner_portal.rs
SEC-010 scan (D-4258) fails on ten form structs without deny_unknown_fields (owner_portal ChangeBody, BridgeBody, OptOutBody, StripeBody, SafetyBody, SpanishBody, BusinessBody, ReleaseBody; owner_tools EvidenceBody; owner_help HelpForm; homeowner_portal PhotoPost) and unclassified raw parses (owner_tools audit_log RawQuery, homeowner_portal upload_fields Multipart and csrf_checked), none owned by T121. Also business_signup.rs Asked::from_query (owned) is a request input fitting none of SEC-010's exceptions; needs a spec line. SEC-045 gitleaks commit-range scan needs a harness change (.github/workflows/ci.yml:128 runs only gitleaks dir). Reviewer confirmed all against main 21d991673. Worker's partial work left uncommitted.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T121 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T121 --reason "..."` brings it back with the attempts it has left (D-2859).
