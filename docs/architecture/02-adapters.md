# Architecture section 2 notes: `adapters`

Moved here verbatim from `docs/architecture.md` (D-6022), whose index lists every part. Until T885 makes `docs/architecture/` a shared path, a task adds new notes to the index, at the end of the section this part came from, and the next plan change moves them here.

The section 2 notes ("Processes and code layout") whose first named crate is `adapters`, in their original order.

- AI calls: `app/ai` sends only `ScrubbedText` through its `Transport` (over `adapters`' `StandInAi` in the build) to `/v1/messages`, with the versioned instruction, the text inside one escaped `<homeowner_message>` tag, `schemas/intake_reading.json` without its `$schema` and `$id` as `output_config` (the file holds only keywords the API supports, so the 0-to-1 confidence range is checked in code, D-2760) and no tools; it retries per AI-023 within the `ASKHAVA_AI_TIMEOUT_SECONDS` budget, stops at `ASKHAVA_AI_DAILY_CAP`, writes one `ai.call_stat` row per call (model, instruction version, request ID, outcome, alert ID) and screens AI-written text with `catalog`'s CARD-010 list, a link ban and length caps, on a normalized copy (T024, D-810 to D-814).
