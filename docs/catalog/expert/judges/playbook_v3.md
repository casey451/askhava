# Run prompt: expert file as guidance, not as a script (variant `playbook_v3`)

Same task, files and homeowner behavior as `intake_agent.md`. The standing rules, the 911 text, coverage, facts-only cards, no advice, no cause, no price and privacy all stay as written. What changes is how the questions are chosen.

**The intake asks its own questions, guided by the expert file.** There is no fixed question-bank set and no "one follow-up" cap. Instead, for the job type the intake lands on, it reads from the expert file:

- `must_know` (ranked): the facts to get, in that order, stopping when they are covered;
- `trade_brief.misdiagnosis_checks` and `hidden_conditions`: what to check when the words fit a look-alike or a trap;
- `allowed_follow_ups` and the question set: wording to reuse when it fits (reuse their plain words rather than inventing jargon);
- `photos_priority`: which photos to ask for, in that order, up to 3;
- `urgency`: which facts flag the request as urgent (never a judgment about safety);
- `card_template`, `scope_line_en`, `photo_reading`, `never_say`: how to write the card.

Rules that still bind the intake:

1. One question per message, in plain words, under 15 words where possible, with 2 to 6 numbered options where options help and "Not sure" always allowed.
2. Never ask what the homeowner's words already answered, and never ask for a photo after she says she cannot send one.
3. At most 8 questions in all, counting the timing question and photo asks. Stop sooner when the ranked must-know facts are covered; a shorter intake with the facts beats a longer one.
4. Ask the timing question only if her words did not already give it.
5. Keep her exact numbers and words on the card (a stated "1,800 sq ft" or "built 1974" goes on the card as stated, never only as a band).
6. A request that belongs to another category gets routed there and the intake continues in that category with its expert file, producing a card there; a two-trade request produces a card for the first category and names the second request on the card.
7. The card carries only what she said or a photo showed. Blank fields say "not asked" or "not said", never a default like "not checked".
8. The question set stays the fallback the app uses when the AI switch is off; it is not used here.

Write `docs/catalog/expert/eval_runs/<run>/<category>/playbook_v3.yaml` in the same shape as the playbook output, with `variant: playbook_v3`, `follow_ups_used: []` and `skipped_bank_questions: []` (not applicable), and `questions_asked` counted as above. Use only file tools; never call a scheduling or messaging tool. Reply with the file path and a two-line summary.
