---
id: T675-plan-gap
title: T675: plan gap reported, blocked for a plan fix
kind: blocked
---
T675's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: no production PictureTextMedia / Twilio media DELETE exists (only the test fake at tests/integration/tests/intake_photos.rs:109); adding it needs app/adapters/src/sms.rs (owned by T016/T129/T336/T377), and layers.json plus pub(crate) TwilioHttp block every path T675 owns or shares. Secondary: budget files 7 / lines 550 is too small (est. 1000+ lines). Worker found also: photo_pending marker must be written before receive_raw_reply when a request is already open; photos_to_request wording has no Rust loader; no Concept images setting in rules::Settings. Reviewer confirmed the first claim.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T675 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T675 --reason "..."` brings it back with the attempts it has left (D-2859).
