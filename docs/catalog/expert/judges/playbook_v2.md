# Run prompt: playbook with three proposed rule changes (variant `playbook_v2`)

Same task, files and homeowner behavior as `intake_agent.md`, with exactly three changes to the intake's rules. Everything else in `intake_agent.md` (standing rules, 911 text, coverage, card facts only, no advice, no cause, no price, privacy) stays as written.

The three changes (each is a proposed catalog or rule change in `../PROPOSED-RULE-CHANGES.md`, tested here so the proposal carries evidence):

1. **Bank questions apply per job type.** Once the kb job type is known, a bank question is asked only if the expert file's `must_know` for that job type names it in `bank_question`, or the question is marked `emergency: true` in the question set. Any other bank question is skipped for that job type. (Today's rule: ask the whole set in order and skip only what her words answered.)
2. **Up to two follow-ups.** The intake may ask up to two follow-ups from the job type's `allowed_follow_ups` (including ones shared by id), chosen by `ask_when` and by must_know rank, never a third. (Today's rule: one.)
3. **Photo asks follow the expert file.** The photo asks are the job type's `photos_priority` ids (up to 3, in that order), using the trade file's `ask` wording for each, instead of the question set's fixed `photos` list. (Today's rule: the set's three ids for every job type.)

The free-text "anything else" bank question, where the set has one, still counts as a bank question under rule 1: ask it only when the job type's must_know names it.

The total question cap stays 8. Count every question, follow-up, the timing question and each photo ask in `questions_asked`.

Write `docs/catalog/expert/eval_runs/<run>/<category>/playbook_v2.yaml` in the same shape as the playbook output, with `variant: playbook_v2`, `follow_ups_used: [ids]` (a list, replacing `follow_up_used`) and `skipped_bank_questions` listing the ids skipped under rule 1 or because her words answered them. Reply with the file path and a two-line summary.
