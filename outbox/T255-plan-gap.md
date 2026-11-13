---
id: T255-plan-gap
title: T255: plan gap reported, blocked for a plan fix
kind: blocked
---
T255's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/picks/src/release.rs:337 (owned by T069, T077, T253, T254, T278, T281, T318, T355) - an emergency address cannot reach the release with T255's files. picks::release reads the address only from the proof's contact (release.rs:347, privacy/src/api.rs:257); the sink mints the proof for the phone contact (release_sink.rs:101-133); take_name_address stores the address as a separate phone-less vault contact (conversation.rs:408-421); privacy has no public setter for a stored contact (vault.rs, only private reseal). Minting for the address-only contact would give the masked line a homeowner with no phone (release.rs:226-243). Fix needs either a privacy setter (T255 co-owns app/privacy/src/vault.rs and api.rs) or release.rs reading a second address contact named in the proof. Reviewer confirmed all five points.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T255 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T255 --reason "..."` brings it back with the attempts it has left (D-2859).
