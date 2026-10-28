# About this folder

The Grok trade knowledge base, received October 28, 2026 (D-431). AI research, not legal advice; lines marked `[judgment]`, `[unverified]` or `[open question]` are not verified facts.

How Ask Hava uses it:

- **Used:** job types, triggers, the questions a pro asks, photo asks and photo cues, license-class notes and the 370 test messages (`test_set.csv`, run with the golden set, OPS-040). Each question set in `../question_bank.yaml` names its trade here (`kb_trade`) and its photo asks (`photos`, ids from `photos_to_request`).
- **Not used:** the `safety_step`, `emergency_signals` safety wording and any health, heat, water or appliance advice. Ask Hava sends only the one 911 text (INT-004, D-436). A test message marked `safety_step_first` means: send the INT-004 text first when the message is on the 911 list; otherwise treat it as an urgent job.
- **Changed in this copy:** phone numbers and one street address were removed so the repository's PII scan stays clean; the source URLs still point to them.
- The license map (`../license_map.yaml`) still decides who gets a card; this folder never does.
