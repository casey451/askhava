---
id: T862-plan-gap
title: T862: plan gap reported, blocked for a plan fix
kind: blocked
---
T862's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/server/src/homeowner_portal.rs, app/portals/src/homeowner_portal.rs
Evidence: her data download is assembled in app/server/src/homeowner_portal.rs:621-632 (held_file), which calls portals::more_held_items(&mut db, contact) with no vault or HomeownerSessionProof. OnFile::read (app/portals/src/homeowner_portal.rs:75-85) keeps only contact id and phone. Opening the sealed address under (PortalsHomeowner, OwnRecord) needs the vault and proof (privacy api.rs:317-322 read_own_contact). Neither file is owned or shared by T862 (kit/prompts/T862.md:35-44, :56). Smallest fix: add app/server/src/homeowner_portal.rs to T862's owned files. Reviewer confirmed at head febc73c2ddbc732b28ceed160a5dce512ae8d67e.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T862 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T862 --reason "..."` brings it back with the attempts it has left (D-2859).
