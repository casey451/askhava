# Judge prompt: blind pro panel

You judge how well three intakes scoped the same jobs. You have never seen Ask Hava's playbook and must not open `docs/catalog/expert/<category>.yaml`, `docs/catalog/question_bank.yaml` or `docs/catalog/trade_kb/`. You judge only what is in the run files.

## Load

- `docs/catalog/expert/scenarios/<category>.yaml`: for each scenario, the hidden facts, traps and expected outcome.
- `docs/catalog/expert/eval_runs/<run>/<category>/playbook.yaml`, `baseline_none.yaml`, `baseline_human.yaml`.

For each scenario, shuffle the three variants into labels A, B, C (write the mapping in the output, then score by label so the labels do not bias you).

## The panel

Score as three people, each in the trade the category belongs to, and record each one's answers:

- **Owner**: 25-year owner-operator. Would I send a truck on this card, and would I know roughly what to charge before I go?
- **Estimator**: prices from text and photos all day. What is missing that I would have asked? What was asked that I didn't need?
- **Field tech**: shows up with the van. What would surprise me on site that the card should have warned me about?

## Score each variant on each scenario

```yaml
- scenario_id: <id>
  labels: {A: playbook, B: baseline_none, C: baseline_human}   # the shuffle you used
  scores:
    A:
      quotable: 0            # 0 = no, I'd have to call; 1 = maybe, one call; 2 = yes, with a visit-fee or range quote; 3 = yes, firm from the card
      missing_facts: ["..."]           # facts in hidden_facts a pro needed that are not on the card
      wasted_questions: ["..."]        # questions in the transcript that did not change the quote
      trap_caught: true                # the scenario's traps surfaced on the card (null when the scenario has no traps)
      routing_correct: true            # category and bank job type match expected (null when expected has none)
      fire_dept_correct: true          # fire_dept_text matches expected
      facts_not_stated: ["..."]        # anything on the card the homeowner never said and no photo showed
      questions_asked: 4               # copy from the run file
      stopped_before_card: null        # copy from the run file
      panel_notes: {owner: "...", estimator: "...", field_tech: "..."}
    B: ...
    C: ...
```

Rules of judging:
- Judge the card and the question list, not the prose style.
- A card that says a cause or a price is not better for it; it is scored on facts. (The compliance judge handles whether it was allowed.)
- `quotable` is about the business quoting without a phone call. A visit-fee quote from a clear card is a 2. Only a firm price from the card is a 3.
- `missing_facts` are only facts a pro would really need for this job; do not list trivia.
- When a run stopped before a card for the expected reason (outside area, no businesses), score `quotable: null` and `routing_correct: true` if it stopped for the right reason.
- Be hard on all three. The point is to find where the playbook loses to a human.

## Output

Write `docs/catalog/expert/eval_runs/<run>/<category>/judgement.yaml`:

```yaml
category: <id>
run: <run id>
judge_model: "<model>"
panel: [owner, estimator, field_tech]
scenarios:
  - (one block per scenario as above)
summary_by_variant:
  playbook: {median_quotable: 2, quotable_share: 0.92, median_questions: 4, routing_correct_share: 1.0, fire_dept_correct_share: 1.0, trap_caught_share: 0.9, facts_not_stated_count: 0}
  baseline_none: {...}
  baseline_human: {...}
where_playbook_lost_to_human: ["..."]   # the specific facts or questions the human got and the playbook missed, as fixes to propose
```

Then follow `compliance_judge.md` on the playbook run and append its block to the same file. Reply with the file path and the three `quotable_share` numbers.
