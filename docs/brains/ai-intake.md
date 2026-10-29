# Brain: AI intake with the Claude API

- **Purpose:** how Ask Hava should call the Claude API to read a homeowner's request and word its questions and job card, how to treat what comes back, and where the current plan meets or misses Anthropic's documentation and the published guidance on LLM security and classification.
- **Who reads it:** any session building or reviewing the `ai` crate (the adapter, the schemas, the deterministic stand-in), the `intake` conversation (reading, trade matching, the question bank, photo asks), the `privacy` scrubber and inbound reader, `quotes` (AI quote summary, note screening), and any task whose prompt touches AI-*, INT-020, INT-021, INT-026, INT-039 to INT-041, INT-044 to INT-046, PRIV-014, PRIV-026, PRIV-029, PICK-003, PICK-052 or CARD-013.
- **How to use it:** read the section for your task, run the review checklist (section 15) before you open the PR, and cite the source tag (`[N#]`, list in section 16) when you rely on a rule.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec, and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = not backed by a primary source; reasoning only.

## 1. The contract the plan fixes

The plan treats the model as a narrow, replaceable reader that sits behind plain code on both sides. In one line: scrubbed text in, schema-checked fields and checked words out, no tools, no decisions.

- **What goes in.** Only `ScrubbedText`, a type only `privacy` can build (AI-004, architecture section 8). The scrubber (AI-002, AI-003, AI-020) removes stated names, the vault name, 7+ digit runs (also spelled out in English or Spanish), street numbers, emails, links and handles, after NFKC normalization and removal of Unicode format characters. Quote text from businesses is scrubbed too (AI-002). The name-and-address answer never reaches the AI (INT-027). No web-chat text is sent before its phone code is verified (AI-021). Photos go only after zero retention and the AI-018 photo sentence (AI-016, CARD-013), and always after metadata is stripped (PRIV-014).
- **What comes out.** Fixed fields (AI-010: trade, job type, emergency, urgent, language, major, missing info) and, inside AI-019, four kinds of words: a reworded next bank question, one follow-up the expert brief allows, the job card and project file, and the INT-046 image prompt. Everything fails closed: a schema failure is discarded (AI-005), a word check failure falls back to the template or her own scrubbed words (AI-019, AI-021).
- **What it never does.** No tools; nothing it returns is executed; it never moves money, releases details, chooses recipients or judges safety (AI-006). Its emergency and urgent fields are recorded only; the INT-002 keyword match alone sends the 911 text (INT-026). Gas words override its handyman reading (AI-012). Keyword rules give an independent second opinion on trade and job type (AI-011, INT-040). It never guesses a license class (AI-017).
- **When it runs at all.** Stand-in during the build (AI-009); real model only from the Railway test project on fake data in Phase 2 (AI-008); in live only after zero retention, or before it only with the owner switch on, the provider's terms recorded and the privacy policy matching (AI-008, AI-018). Summaries of quotes stay off at launch (PICK-003).

These choices match the authorities closely. OWASP's prompt-injection entry asks for least privilege with "extended functions in code, not through the model", deterministic code that validates output formats, and human approval for high-risk actions [N20]; its excessive-agency entry asks for "authorization in downstream systems rather than relying on an LLM to decide if an action is allowed" [N23]. Willison's "lethal trifecta" is private data plus untrusted content plus a way to communicate out; he says the only safe course is to avoid the combination [N29]. Ask Hava's AI holds no private data (scrubbed input), and its only outward channel is words that pass code checks into fixed slots, so two legs are cut [inferred from N29].

## 2. Calling the API: the request shape

- **Endpoint.** The Messages API only. Zero data retention covers the Messages and Token Counting APIs; the Batch API (29-day retention) and Files API (kept until deleted) are not eligible, nor is the Console playground [N8]. Proposal 3.
- **Model.** `claude-haiku-5-5` is Anthropic's current Haiku, described for "high-volume, latency-sensitive tasks such as classification, extraction, and routing", 1M-token context, 128K max output, priced from $0.10 per million input tokens and $0.50 per million output tokens [N6]. Section 7 covers pinning.
- **System prompt.** The versioned instruction for the job (AI-015), with the response language stated in it: Anthropic advises stating the input and output language explicitly and putting a runtime language choice in the system prompt rather than letting the model infer it [N17]. Allow the model to say it doesn't know: "This technique can drastically reduce false information" [N14]; in a schema that means an "unknown" enum value and a `missing_info` list, never a forced guess [inferred from N14].
- **User turn.** The scrubbed text wrapped in one fixed XML tag (for example `<homeowner_message>`), since tags "reduce misinterpretation" when a prompt mixes instructions and variable inputs [N13]; OWASP asks to "segregate and identify external content" [N20]. Proposal 8 escapes the tag name inside the text.
- **Output.** `output_config.format` with `type: "json_schema"`; no beta header is needed [N1]. Schema limits: `additionalProperties` must be `false`; `enum`, `const`, `required` and `anyOf` work; `minLength`, `maxLength`, `minimum`, `maximum` and array limits beyond `minItems` 0 or 1 are not supported and return a 400 [N1]. So length caps live in code (AI-021), not in the schema.
- **No `tools` field.** Sending none is the whole defence for OWASP LLM06 [N23]; the stand-in refuses a tool offer (`stand_ins/ai.py`).
- **Small `max_tokens`.** A reading needs a few hundred tokens [inferred]; a cut-off answer has `stop_reason: "max_tokens"` and "might be incomplete and not match your schema" [N1].
- **Temperature 0 for readings** [inferred], knowing that "even with temperature set to 0, the results will not be fully deterministic" [N15]. Tests against the real model therefore measure rates on a fixed set (section 10), never exact strings.

## 3. Treat every output as untrusted input

OWASP: "Treat the model as any other user, adopting a zero-trust approach", encode output for where it is used, use parameterized queries, a strict CSP, and log unusual outputs [N22]. In Rust terms:

1. **Check `stop_reason` first.** Only `end_turn` is usable. `refusal` returns HTTP 200, is billed, and "the output might not match your schema" [N1]; `max_tokens` likewise [N1]. The full list is `end_turn`, `max_tokens`, `stop_sequence`, `tool_use`, `pause_turn`, `refusal`, `model_context_window_exceeded` [N19]; anything else is a discard.
2. **Parse with serde `deny_unknown_fields`** into a typed struct even though structured outputs constrain decoding, since compliance holds "in most cases" and has listed exceptions [N1].
3. **Allow-list every enum against the catalog.** Trade must be an open category; job type must be a job type under that trade in `docs/catalog/`; language `en` or `es`. Structured outputs "don't guarantee the capitalization of string enum and const values" [N1], so trim and lowercase before matching, and never define two enum values that differ only in case. Proposal 9.
4. **Cap lengths in code** for every free-text field (AI-021), then run the AI-019 checks, CARD-010, the URL ban and, last on a card, CARD-008.
5. **Never render model text unescaped.** Askama autoescapes; the AI-smell scan bans `|safe` and `escape = "none"` (architecture section 2). Texts go out only through fixed templates by ID with the AI words in a slot (TXT-004).
6. **Never let model text choose anything.** No model string becomes a SQL fragment, URL, file path, template name, recipient or fee (FEE-005, SEC-029) [N22].
7. **Store only enumerated fields** in `ai.reading` and the off-database stream (AI-013, PRIV-029); AI-written texts go where PRIV-008 and TXT-020 retention applies.

## 4. Prompt injection: homeowner text, quote text and photos

- **No filter catches every injection.** OWASP says prevention measures only reduce impact [N20]. Willison is "generally skeptical about attempts to use AI to filter and catch prompt injection attacks" because "it's impossible to reliably predict the shape of every potential attack" [N28]. So the plan's defence is to limit what a successful injection can change, and AI-007 tests that, which is the right shape [N20 item 7, "adversarial testing"].
- **What an injection could still reach**, and the guard:
  - A wrong trade or job type: the keyword second opinion (AI-011), disagreement asking her (INT-040), and her approval of the card (INT-041).
  - A contact detail, link or off-platform push in a card or question: CARD-008, the URL ban (AI-021), PICK-052 for notes.
  - A diagnosis, price, business name or claim: the AI-019 checks and the WORD-003 and WORD-022 scans.
  - A fake emergency: emergency fields are recorded only (INT-026).
  - A fee or a disclosure: code only (FEE-005, TXT-004).
- **Hidden characters.** AI-020's removal of Unicode format characters (category Cf) also removes the Unicode tag block sometimes used to hide instructions [inferred]. NFKC is "compatibility decomposition, followed by canonical composition" and folds width variants and superscript digits [N33]; Unicode warns it "must not be blindly applied to arbitrary text" [N33], which is why the scrubber matches on a normalized copy and the stored text keeps her own characters [inferred].
- **Photos.** OWASP notes that instructions hidden in an image can change a multimodal model's behavior and that defences are still research [N20]. The plan's order helps: no photo reaches the AI before zero retention (AI-016); the on-server check blurs or drops readable text (INT-044) first; two screening calls with different instructions must both pass (AI-016); and a photo description passes the same AI-019 checks.
- **Photo reading, when it starts.** Keep images at or above 320 px on the short side (INT-044), well above the 200 px under which Claude "might hallucinate or make mistakes" [N18]; send JPEG or PNG only; never ask the model to identify people (it refuses) or to judge whether an image is AI-generated (it cannot) [N18]. Sensitive-data guidance in OWASP LLM02 (strict input validation, clear retention policies) [N21] is met by the scrubber and PRIV-008. Confabulation, "confidently stated but erroneous or false content", is a named GenAI risk [N27] and OWASP LLM09 [N25]; INT-041's review is its control.
- **Screens with a second model.** Anthropic suggests a lightweight pre-screen with Haiku and a boolean schema, treating a `refusal` as a harmful verdict [N12]. PICK-052's AI note check and AI-016's two photo calls follow that pattern; a refusal from either counts as a flag [N12].
- **Repeat abusers.** Anthropic advises throttling users who keep trying to bypass guardrails [N12]; the AI-021 daily cap is global only [inferred gap; no proposal, note for `ops`].

## 5. Failures: errors, retries, timeouts, fallback

From Anthropic's error reference [N2] and rate-limit page [N3]:

| Response | Meaning | Ask Hava action |
|---|---|---|
| 400 `invalid_request_error` | Bad request, unsupported schema feature, or a spend limit reached | No retry; alert; AI-off path |
| 401, 403 | Key problem | No retry; critical alert; AI-off path |
| 413 | Over 128 MB | Cannot happen with capped input [inferred]; no retry |
| 429 with `retry-after` | Rate limit (RPM, ITPM, OTPM, token bucket) | Wait the header's seconds, at most the retry budget |
| 429 without `retry-after` | Tier spend cap; "keeps failing until access resumes" | No retry; alert; AI-off path |
| 500 `api_error` | Internal; "retry with exponential backoff" | Retry within the budget |
| 504, 529 | Timeout; overloaded across all users | Retry within the budget |
| 200 with `refusal` or `max_tokens` | Billed, output may not fit the schema [N1] | Discard; AI-off path for this step |

- Keep the `request-id` header with each AI call stat (no text), since support asks for it [N2].
- Anthropic's SDKs retry twice by default with backoff [N2]; Ask Hava calls the API through reqwest (BUILD_BRIEF.md:112), so it must do this itself.
- OWASP asks for timeouts and graceful degradation under load [N24].
- **Where the plan stands.** INT-021 holds the request ("we have your message" once) and retries the reading every 30 minutes. AI-008 already defines a complete AI-off path: keyword rules, fixed questions, cards from fields and her words. Holding an urgent request for 30 minutes when that path exists is a worse outcome than using it [inferred]. Proposal 4.

## 6. Cost and volume

- **Price.** At Haiku 5.5's listed starting prices [N6], a reading of about 2,000 input and 300 output tokens costs well under a cent [inferred arithmetic]. The cost risk is abuse volume, not normal use; OWASP LLM10 asks for input size limits, per-source quotas, throttling and monitoring [N24]. AI-021's daily cap, the verified-code rule for web chat and INT-007's short-message rule cover most of it.
- **Spend limits.** An organization can set a monthly spend limit; reaching it returns 400, and a tier's spend cap returns 429 with no `retry-after` until the first of the next month (00:00 UTC) [N2][N3]. Proposal 12.
- **Prompt caching.** Haiku 5.5 caches prompts of 512 tokens or more; shorter prompts are silently not cached [N7]. The default lifetime is 5 minutes, refreshed on each hit; a cache write costs 1.25x and a read 0.1x the base input price [N7]. Changing the system prompt invalidates the cache [N7]. Put the versioned instruction first with `cache_control` and the scrubbed text after it. At Havasu volumes most hits will come from the several calls in one conversation within minutes [inferred]. Cached reads mostly don't count toward the ITPM rate limit [N3]. Prompt caching is ZDR-eligible; KV cache data is "held in memory only and are not stored at rest" [N7].

## 7. Model pinning and deprecation

- Each model ID "identifies a pinned version of the model"; "Anthropic does not update the weights or configuration of an existing model ID"; a dateless ID from the 4.6 generation on "is not an alias. It is the snapshot" [N5]. So `claude-haiku-5-5` is a fixed model. Serving changes can still cause "minor differences in observable behavior" [N5], which is a reason to re-run the golden set on a schedule, not only on change [inferred].
- Retirement: at least 60 days' notice by email and in the docs; requests to a retired model fail [N4]. Haiku 5.5 is Active, retirement "not sooner than October 7, 2027" [N4]. Proposal 5 makes the model ID a recorded setting and makes a model change pass the same gates as an instruction change.

## 8. Confidence, abstaining and human review

INT-039 lets the AI skip a question when its confidence is at or above 0.9, and INT-040 sends a request on when keyword rules and the AI agree at or above a high-confidence setting.

- **Self-reported confidence is not calibrated.** Xiong et al. found that "LLMs, when verbalizing their confidence, tend to be overconfident" [N30]. Guo et al. showed modern neural networks are "poorly calibrated" and that a simple post-hoc fit corrects much of it [N32]. A 0.9 from the model is not 90% right until measured [inferred from N30, N32].
- **Abstaining is the tool.** Selective classification trades coverage for a guaranteed error rate: the classifier rejects inputs as needed to meet a target risk [N31]. INT-040 already abstains (ask her, then the "Job type not certain" card, then no card and an owner email), and INT-041 is the human review step.
- **Anthropic's guidance agrees.** It says to allow "I don't know" [N14] and to measure with code-graded evals before relying on a model [N16]. NIST's generative AI profile suggests minimum performance thresholds as go/no-go criteria before deployment (GV-1.3-002) [N27].
- Proposal 6 sets the threshold from a measured run.

## 9. The deterministic stand-in

What exists (`stand_ins/ai.py`, D-121 to D-126): keyword rules, `ScrubbedText` only, a tool offer refused, output validated against a fixed field list. To keep tests stable:

- **Pure function.** The reading depends only on the instruction version and the text: no clock, randomness, network or set iteration order [inferred].
- **Same schema file** as the real request's `output_config` and the Rust types, so a drift fails `Ci` [inferred]. Today the stand-in returns a `rental` field that AI-010 does not list, and no confidence value although INT-039 and INT-040 need one. Proposal 7.
- **Fault modes chosen by the test**, never by homeowner text: 429 with and without `retry-after`, 500, 529, a timeout, `stop_reason` `refusal` and `max_tokens`, output that breaks the schema, an enum in the wrong case, and an output carrying a phone number, a link, a price or a cause word. Each maps to a row in section 5 or a check in section 3 [N1][N2].
- **Contract tests** from the published API shapes, re-run against the real API in Phase 2 (architecture section 7, T137).

## 10. Offline evaluation of intake quality

- Anthropic's eval advice: mirror the real task distribution and include edge cases; prefer automated, code-graded checks; "more questions with slightly lower signal automated grading is better than fewer questions with high-quality human hand-graded evals" [N16]. Trade and job type are exact-match graded [N16].
- The plan's sets: `docs/catalog/intake_golden.yaml` (about 20 fake requests per launch-open trade; not yet written, its task is in `kit/tasks.json`) and `docs/catalog/trade_kb/test_set.csv` (370 rows), run on every catalog change against a 90% target (OPS-040). The twin (`twin/`, not yet built) runs the Rust rules through `rules_bridge`.
- Add to each run [inferred]:
  - injection cases (AI-007, T072);
  - AI-019 output-check cases;
  - Spanish rows;
  - every keyword-versus-AI disagreement, so INT-040 abstention rates are visible.
- Against the real model, record the model ID, the instruction version and the date with each result (T137). NIST asks that pre-deployment results reach whoever approves release (MS-2.3-003) and that third-party GenAI is monitored in deployment (GV-6.2-004) [N27]. Proposal 11.

## 11. Spanish

- Anthropic reports Spanish at 96.4% of English for Haiku 4.5 on its zero-shot benchmark; no Haiku 5.5 figure is published on that page [N17]. Keep accents and `¿ ¡ ñ` (native script) [N17]. The scrubber's NFKC step keeps them [N33].
- State the output language in the system prompt from the stored language flag [N17]; the reworded question then matches the template language (CON-026).
- The AI-019 cause-word check lists English words only ("probably", "likely", "sounds like", "needs a new", "the problem is"). A Spanish card can say "probablemente" and pass. Proposal 1.
- Golden and test rows in Spanish are few [inferred from a scan of `test_set.csv`]; proposal 11 sets a floor.

## 12. Retention: what zero retention covers and what it does not

- **Without ZDR**, API inputs and outputs are deleted "within 30 days of receipt or generation"; Usage Policy enforcement and legal requirements are exceptions, and flagged content is kept "for up to 2 years" [N9].
- **With ZDR**, "Anthropic does not store customer prompts or responses at rest after the API response is returned" for the Messages and Token Counting APIs, but "may retain data where required by law or where it has been flagged by Anthropic's automated trust and safety systems ... for up to 2 years" [N8].
- **Not covered by ZDR:**
  - the Console and playground;
  - Batch (29 days) and the Files API;
  - Managed Agents and MCP connectors;
  - consumer plans [N8].
  ZDR is enabled per organization through Anthropic sales [N8].
- **Structured outputs** are ZDR-eligible; "only the JSON schema is cached, for up to 24 hours since last use" [N1]. The schema holds catalog IDs only, never personal data [inferred].
- **Training.** The commercial terms say "Anthropic may not train models on Customer Content from Services" (section B) [N10]. The customer warrants it has the rights to submit inputs (L.1) [N10], which is why scrubbing comes first.
- **Gap.** AI-018's sentences say the provider "deletes it within <days> days" and, after ZDR, "doesn't keep that text". Both leave out the flagged-content exception [N8][N9]. Proposal 2.

## 13. Usage policy

The Usage Policy fetched on 2026-10-29 (showing an effective date of November 12, 2026) requires that "all consumer-facing chatbots or AI agents that communicate directly with external users" disclose that they are AI "at a minimum at the beginning of each chat session or in the product interface" [N11]. Whether a fixed question flow with AI-reworded questions counts as a chatbot is a reading of the policy [inferred]. AI-018's wording sits in the privacy policy and data pages, not in the chat itself. CON-031's "Automated texts" is accurate, but it does not say "AI". Proposal 10. The policy's high-risk list (housing, finance, legal and others) [N11] does not cover routing a repair request [inferred].

## 14. Requirement map and proposed requirements

**Right already:**
- scrubbed text only, through a type the compiler enforces (AI-002 to AI-004, AI-020);
- no tools, no money, no release, no recipient choice (AI-006) [N23];
- schema fields or nothing (AI-005) [N1];
- keyword second opinion and gas override (AI-011, AI-012);
- emergency handled by keywords alone (INT-002, INT-026);
- output checks with template fallback (AI-019, AI-021) [N22];
- human approval of the card (INT-041) [N20];
- injection tests on outcomes (AI-007) [N20];
- versioned instructions (AI-015);
- no photos before ZDR, metadata stripped first (AI-016, PRIV-014, CARD-013);
- quote summaries off, original words always shown (PICK-003);
- daily cap (AI-021) [N24];
- stand-in during the build (AI-009);
- AI-off mode as a full product (AI-008).

**Proposed** (need a spec change logged in `docs/decisions.md`; a task may not add them on its own):

1. **Spanish cause words** (AI-019): "The AI-019 cause-word check also matches 'probablemente', 'parece que', 'seguramente', 'suena a', 'el problema es', 'necesita un nuevo' and 'necesita una nueva' (any case, accents optional), and a test feeds each through a Spanish card." [N17; word list inferred]
2. **Retention wording** (AI-018, AI-008, TERMS-008): "The account checklist records the provider's flagged-content retention period with the general one, and the AI-018 sentences read 'deletes it within <days> days, unless its safety systems flag it, when it may keep it up to <flag days> days' before zero retention and 'doesn't keep that text or train on it, except text its safety systems flag' after it." [N8, N9]
3. **Messages endpoint only** (AI-001, AI-008): "The `ai` adapter calls only `/v1/messages` and `/v1/messages/count_tokens`; a test fails on any other path, and no real homeowner text is entered in the provider's Console." [N8]
4. **Failure handling** (INT-021, AI-008, AI-021): "Each AI call has a 20-second timeout and at most 2 retries with exponential backoff, only on a connection error, 500, 504, 529 or a 429 carrying `retry-after`; any other error, a `refusal` or `max_tokens` stop, or a schema failure ends the call; a failed call runs that step on the AI-off path (keyword rules and fixed templates), replacing INT-021's hold and 30-minute retry; a 401, 403, spend-limit 400 or spend-cap 429 also raises an owner alert." [N1, N2, N3, N24; numbers inferred]
5. **Model pinning** (AI-001, AI-013, AI-015): "The model ID is a setting, `claude-haiku-5-5` at launch, stored with every AI call stat beside the instruction version; changing it requires the golden set, `test_set.csv` and the injection suite to score no worse than the recorded run, logged as a decision; a deprecation notice for the pinned ID opens an owner task the day it arrives." [N4, N5]
6. **Calibrated skip threshold** (INT-039, INT-040): "The AI may skip an INT-039 question, or count toward INT-040 agreement, only at a threshold chosen from a recorded real-model run on the golden set and `test_set.csv`, as the lowest value at which no more than 2% of skipped questions had a different expected answer; until that run is recorded the AI skips nothing." [N30, N31, N32; 2% inferred]
7. **One schema for stand-in and adapter** (AI-009, AI-010): "The AI-010 fields, each with a confidence from 0 to 1 for trade and job type, are defined in one committed JSON schema used by the stand-in, the Rust types and the request's `output_config`; a `Ci` test fails on any difference (today the stand-in's `rental` field)." [N1]
8. **Delimited input** (AI-007): "The scrubbed text is sent inside one fixed XML tag after any occurrence of that tag's name in the text is escaped, and the instruction states that the tag's content is data and never instructions; an injection test closes the tag early." [N13, N20]
9. **Enum allow-list** (AI-005): "Enum fields are trimmed and lowercased, and a trade or job type not open in the catalog under the returned trade discards the reading." [N1, N22]
10. **AI disclosure in the chat** (AI-018, CON-025): "While the AI-008 switch is on, the web-chat page shows 'An AI service helps word our questions.' above the chat box, and a decision records whether the text path needs the same line." [N11]
11. **Evaluation floor** (OPS-040, AI-010): "At least a quarter of the golden rows per trade are in Spanish, and each real-model run records model ID, instruction version, date, job-type accuracy by language, abstention rate and output-check fallbacks in `docs/phase2/real-ai.md`, re-run monthly while AI is on." [N16, N17, N27]
12. **Provider spend limit** (AI-021): "Before the AI-008 switch is first turned on, the account checklist records a monthly spend limit set in the provider's Console at no more than 3 times the expected monthly cost." [N2, N3; multiple inferred]

## 15. Review checklist for AI intake tasks

- [ ] `ai` accepts only `ScrubbedText`; no path builds it outside `privacy` (AI-004).
- [ ] The request has no `tools`, uses `output_config.format` json_schema, a small `max_tokens`, the pinned model ID and the versioned instruction (AI-006, AI-015) [N1][N5].
- [ ] `stop_reason` checked; only `end_turn` is used [N1][N19].
- [ ] serde `deny_unknown_fields`; enums trimmed, lowercased and checked against the catalog; lengths capped in code (AI-005, AI-021) [N1].
- [ ] Every AI-written text passes AI-019, CARD-010, the URL ban and, on cards, CARD-008, with template fallback.
- [ ] No model text reaches SQL, a URL, a path, a template name, a recipient or a fee; no `|safe` [N22].
- [ ] Emergency and urgent AI fields change nothing (INT-026); gas override applies (AI-012).
- [ ] Retries only on the section 5 rows that allow them; the AI-off path runs on failure.
- [ ] No real API key before Phase 2; fake data only; phones 928 555 01xx, emails at example.com (AI-009, PRIV-019).
- [ ] Only enumerated fields stored in `ai.reading` and the off-database stream (AI-013, PRIV-029).
- [ ] Injection and fault-mode cases added to the stand-in tests for any new AI job (AI-007).
- [ ] Wording of any AI description is AI-018's, never "AI-powered".

## 16. Sources and why they are authoritative

Anthropic is the provider; its documentation defines what the Claude API does and its terms bind Ask Hava (N1 to N19). `docs.claude.com` links now redirect to `platform.claude.com`. OWASP's GenAI project publishes the industry list of LLM application risks (N20 to N25). NIST is the US standards agency (N26, N27). Willison is the developer who named prompt injection and writes about it continuously (N28, N29). N30 to N32 are research papers on calibration and abstention (N32 peer-reviewed at ICML 2017; N30 and N31 as arXiv records). N33 is the Unicode standard.

- [N1] Structured outputs: https://platform.claude.com/docs/en/build-with-claude/structured-outputs
- [N2] Errors: https://platform.claude.com/docs/en/api/errors
- [N3] Rate limits and spend limits: https://platform.claude.com/docs/en/api/rate-limits
- [N4] Model deprecations: https://platform.claude.com/docs/en/about-claude/model-deprecations
- [N5] Model IDs and versions: https://platform.claude.com/docs/en/about-claude/models/model-ids-and-versions
- [N6] Models overview (Haiku 5.5 ID, limits, prices): https://platform.claude.com/docs/en/about-claude/models/overview
- [N7] Prompt caching: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- [N8] API and data retention (ZDR scope): https://platform.claude.com/docs/en/build-with-claude/api-and-data-retention
- [N9] Commercial data retention: https://privacy.claude.com/en/articles/7996866-how-long-do-you-store-my-organization-s-data
- [N10] Commercial Terms of Service: https://www.anthropic.com/legal/commercial-terms
- [N11] Usage Policy: https://www.anthropic.com/legal/aup
- [N12] Mitigate jailbreaks and prompt injections: https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/mitigate-jailbreaks
- [N13] Structure prompts with XML tags: https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/use-xml-tags
- [N14] Reduce hallucinations: https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/reduce-hallucinations
- [N15] Glossary (temperature and non-determinism): https://platform.claude.com/docs/en/about-claude/glossary
- [N16] Define success and build evaluations: https://platform.claude.com/docs/en/test-and-evaluate/develop-tests
- [N17] Multilingual support: https://platform.claude.com/docs/en/build-with-claude/multilingual-support
- [N18] Vision (image limits; under 200 px less reliable; no naming people; cannot detect AI-generated images): https://platform.claude.com/docs/en/build-with-claude/vision
- [N19] Messages API reference (`stop_reason` values): https://platform.claude.com/docs/en/api/messages
- [N20] OWASP LLM01:2025 Prompt Injection: https://genai.owasp.org/llmrisk/llm01-prompt-injection/
- [N21] OWASP LLM02:2025 Sensitive Information Disclosure: https://genai.owasp.org/llmrisk/llm022025-sensitive-information-disclosure/
- [N22] OWASP LLM05:2025 Improper Output Handling: https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/
- [N23] OWASP LLM06:2025 Excessive Agency: https://genai.owasp.org/llmrisk/llm062025-excessive-agency/
- [N24] OWASP LLM10:2025 Unbounded Consumption: https://genai.owasp.org/llmrisk/llm102025-unbounded-consumption/
- [N25] OWASP Top 10 for LLM Applications 2025 (index; LLM09 Misinformation): https://genai.owasp.org/llm-top-10/
- [N26] NIST AI Risk Management Framework: https://www.nist.gov/itl/ai-risk-management-framework
- [N27] NIST AI 600-1, Generative AI Profile (July 2024): https://nvlpubs.nist.gov/nistpubs/ai/NIST.AI.600-1.pdf
- [N28] Willison, the Dual LLM pattern: https://simonwillison.net/2023/Apr/25/dual-llm-pattern/
- [N29] Willison, the lethal trifecta: https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/
- [N30] Xiong et al., Can LLMs Express Their Uncertainty?: https://arxiv.org/abs/2306.13063
- [N31] Geifman and El-Yaniv, Selective Classification for Deep Neural Networks: https://arxiv.org/abs/1705.08500
- [N32] Guo et al., On Calibration of Modern Neural Networks (ICML 2017): https://arxiv.org/abs/1706.04599
- [N33] Unicode UAX #15, Normalization Forms: https://unicode.org/reports/tr15/

Not legal advice; no lawyer reviewed this file (D-367).
