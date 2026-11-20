---
id: T672-plan-gap
title: T672: plan gap reported, blocked for a plan fix
kind: blocked
---
T672's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/intake_order_two_jobs.rs:278 (owned by T711 and T717, not T672) also pins the old order, but D-4822 named only two functions in intake_order_two_jobs_rules.rs. Evidence: test td_int_45_also_after_the_zip_opens_the_second_job_through_open_job sends the ZIP reply then ALSO and expects only job.handyman.none linked ("question open", :284-:288); under D-4822 steps 6 to 8 come after the coverage line so "How soon?" is open, and ALSO opens a second job (left: ["job.handyman.none","job.plumbing_repair.none"]). kit/tasks.json: file owned by T711 and T717. Smallest plan change: add that function to T672's owned files under D-4822, changed like function (a). Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T672 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T672 --reason "..."` brings it back with the attempts it has left (D-2859).
