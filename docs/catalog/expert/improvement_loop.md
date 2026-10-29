# Improvement loop: how live signals change the question sets and expert files

The playbooks are a first draft by AI. Real businesses and real homeowners will show where they are wrong. This is the loop that turns those signals into logged changes, with no new requirement: it uses signals the plan already records.

## 1. Signals (all already in the plan)

| Signal | Where it comes from | What it usually means |
|---|---|---|
| A business asks a question through the job page before quoting (JOB-010 details request, or free text that OPS-036 routes) | `quotes` module, business job page (CARD-023) | A must-know fact is missing from the set or the card template |
| Quote spread on one job is wide (top quote more than 3 times the bottom, after removing visit-fee quotes) | `quotes` module, PICK-053 formats make this computable | The scope was unclear, or a hidden condition split the pros |
| The homeowner edits the card at the approval step (INT-041 EDIT) | `intake` module | The card template or the AI's reading got her words wrong |
| A card goes out "Job type not certain" (INT-040) | `intake` module | Triggers and the trade or job-type questions did not separate two job types |
| A business quotes a different job type than the card said, or declines with "wrong trade" | `quotes` module (decline reasons, BIZ-028 BUSY aside) | Routing or misdiagnosis check missed |
| A request fits no category or job type (INT-016 reply, OPS-041 email) | `intake` module | A job type or category is missing (DECISIONS-NEEDED item 1) |
| Monthly intake accuracy report (OPS-040) | `ops` module, T158 | The golden set drifts from real messages |
| The hand-run test's "what did you wish you'd known?" answers | `docs/pilot/hand-run-test.md`, sheet tab Quotes | The single best source before launch |

## 2. The weekly review (an AI session, not Casey)

Once a week, a Claude session with the repo runs this:

1. Pull the week's signals as counts and scrubbed text only (never homeowner details; the intake log keeps trade, job type and flags, AI-013).
2. For each category with 2 or more signals of the same kind, draft a change: a new or reordered bank question, a new allowed follow-up, a card-template line, a trigger phrase, or a misdiagnosis check. Each draft cites the signal counts.
3. Write the change to the expert file or question set on a branch, bump the set's `version`, and add the evidence to the file's `confirmed_by` or a `changes` list.
4. Re-run `scripts/check_expert.py` and `scripts/check_catalog.py`, then the category's scenarios through the harness (`judges/README.md`) with the changed file. A change that drops a score is not merged.
5. Log it in `docs/decisions.md` as a catalog change (the question bank header requires it) and in the weekly digest to Casey as one line per change.

Nothing in this loop asks Casey to decide anything unless a change adds a job type or category, which is a LIC change and a decision (`DECISIONS-NEEDED.md` item 1 pattern).

## 3. During the hand-run test

Add to the Quotes tab of `docs/pilot/hand-run-tracker.xlsx` one column per quote: **"What did you wish the card had told you?"** Ask it once per business per week, by the same email thread. Feed the answers into the review above. Casey's time: about 10 minutes a week, inside the playbook's existing business follow-up.

## 4. What never changes through this loop

- The AI-019 limits, the 911-only rule (D-436), the no-ranking and wording rules. A signal that "homeowners want to know what's wrong" is answered by a better question, never by advice.
- Routing: a learned trigger may change which job type a message maps to, but who receives a card is still only the license map.

## 5. Size discipline

An expert file should stay under 40 KB and a job type entry under one page. When a change would push past that, condense: fold a follow-up into a bank question, merge two misdiagnosis checks, drop a never_say line that the compliance checks already catch.
