# Prompt assembly: what the intake AI is given, per request

For T024 (`app/ai`), T156 (`app/intake`) and T163 (`app/ai/src/writer.rs`). This is a proposal for how the versioned AI instructions (AI-015) are built from the catalog; it changes no requirement. The AI is Claude Haiku with no tools (AI-001, AI-006). It sees scrubbed text only (AI-002 to AI-004) and returns schema-validated fields (AI-005, AI-010). Everything it knows about a trade arrives in the prompt below.

## 1. The three AI jobs and what each loads

| Job | When | Loads | Returns (AI-010) |
|---|---|---|---|
| **Read** | After the first message (and again after each answer while the job type is unsure, INT-040) | Standing rules; the category list (id, name, keywords, job types) from `categories.yaml`; for the 2 or 3 candidate categories the keyword rules scored highest, their `trade_kb` `triggers_en` / `triggers_es` and job type labels | trade, job type, confidence, emergency flag (fire-truck list only), urgent flag, language, major and missing-info fields |
| **Ask** | Before each bank question while the AI switch is on (INT-039) | Standing rules; the request so far (scrubbed words, answers, photo notes); the question set for the category; the **expert file's job_types entry** for the chosen job type (must_know, allowed_follow_ups, photos_priority, urgency); the trade file's `questions` for that job type | the next bank question reworded or unchanged; `skip: true` when her words already answered it with high confidence; at most **one** follow-up, chosen from `allowed_follow_ups` by id, with its text copied or lightly reworded |
| **Write** | When the questions and photo asks are done (INT-041) | Standing rules; the request; the expert file's `card_template`, `scope_line_en`, `photo_reading` and `never_say` for the job type; the kb `sample_card` | the job card (CARD-007), the project file (INT-045), and for design categories the INT-046 image prompt |

Photo notes come from the on-server check before zero retention (INT-044) and from the AI photo read after it (AI-016); the Write job uses `photo_reading[].neutral_wording` as the pattern for photo lines.

## 2. Standing rules (same text in every call, versioned in code)

1. You are Ask Hava's intake. Ask Hava is not a contractor and does not perform, bid on or supervise work. It gathers a few facts, writes a job card the homeowner approves, and sends it to local businesses that quote in their own words.
2. You never diagnose, name a cause, estimate cost, judge safety or urgency, recommend or mention any business, promise an outcome, or add a fact the homeowner did not give. You never give repair or safety advice. (The 911 text is sent by code, not by you.)
3. You ask one question at a time, in plain words a 12-year-old could read, under 15 words where you can. You rewrite a bank question only to fit what she already said. You skip a bank question only when her own words answered it. You may ask one follow-up, and only one from the list you were given, by its id.
4. The card carries facts she gave, in her scrubbed words, plus what a photo shows in neutral observable words. It carries no name, phone, email, street number or gate code. It never says what is wrong, what it will cost or who should do it.
5. Spanish: when she writes in Spanish, use the Spanish text of each question and option; otherwise English.
6. Output only the fields in the schema. Anything else is discarded.

Code then runs the AI-019 checks on every output: contact details (CARD-008), dollar amounts and prices, cause words, business names, safety wording, banned words (WORD-003) and the AI-smell list; a hit drops the AI text and falls back to the fixed template.

## 3. Size budget per call

Measured on the reference file (`plumbing_repair.yaml`, the largest category):

| Piece | Size |
|---|---|
| Standing rules | about 350 words |
| Question set (one category) | 300 to 900 words |
| Expert file, one job type entry | 250 to 500 words |
| Trade file `questions` for the job type | 150 to 400 words |
| Card template, scope line, photo reading, never_say | 150 to 350 words |
| Request so far | under 400 words |

So no call needs more than about 3,000 words of context. A whole trade file (up to 40 KB) is never sent; only the slices for the chosen job type. The slicing is plain code over the YAML, so it is testable without the AI.

## 4. How the one follow-up is chosen

The Ask job is given the job type's `allowed_follow_ups` with their `ask_when` lines. The rule in the prompt: "Pick a follow-up only if its ask_when fits what she said and no bank question in this set covers the same fact. Pick the one whose must_know fact ranks highest. If none fits, ask no follow-up." The returned field is the follow-up's id; code looks up the text and options, so the AI cannot invent a question. The AI may return a reworded text for it, which passes the AI-019 checks like any other AI text.

## 5. Fallback with AI off

With the AI-008 switch off, intake asks the fixed bank questions and photo asks only, and skips a question only on a plain keyword rule (INT-039). The expert file still serves routing tests, the card template (filled by code from card-field answers, CARD-007), and the business-side answers.

## 6. Where the expert file is read by code, not by the AI

- `urgency.urgent_capable_triggers`: a keyword list code may use to set the "urgent" flag for the after-hours path (BIZ-007), never a safety judgment.
- `urgency.fire_dept_triggers`: test fixtures for the INT-004 keyword list; the list itself lives in code and the catalog.
- `business_side.signup_objections`: the sign-up help page and the AI-drafted HELP replies, if OPS-036 is ever switched on.
- `photos_priority`: the order of the question set's photo asks for that job type (INT-044 "in usefulness order").
