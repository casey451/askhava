# COPY-ES-1 verdicts (report ca59e1dc), Opus read-only verification

Tree: `/tmp/claude-0/wt-pf` at `ed430909b` (code = main, plan files = plan branch). State: T799 QUEUED, attempts 1 (`origin/claude/kit-state`).
Counts: a Python copy of `count_segments` + `gsm_units` (`app/messaging/src/templates.rs:310-337`) on the brand-filled text, as `Registry::parse_with`/`check_filled` do (`templates.rs:147-195`): `{brand_short}`/`{brand}` filled from `app/core/data/brand.json` (Laftly, laftly.com) and from the test brand (`tests/integration/tests/brand_setting.rs:50-66`: Testbrand, `tb.example`), `{job}` `{link}` `{time}` left literal. Format: units Laftly/Testbrand, segments Laftly/Testbrand. The copy reproduces D-5837's five figures exactly (332/335, 431/434, 361/364, 386/389, 377/380).
Checks run: a throwaway `git archive` snapshot in the scratchpad, never the repo, with T799's full es.json goal plus every change endorsed below applied. Results: `python3 -m pytest tests/docs` 3012 passed, 9 skipped (includes `test_introduction_fee_wording.py`, `test_segments.py`, the wording and template scans); `scripts/check_ai_smell.py` 0 hits; `scripts/check_copy.py` 0 problems (it does not read es.json). I ran no cargo tests. The Rust pins are listed per item.

## 1. T799's planned text (D-5401 + D-5837): PARTLY right

- **Phrase:** CONFIRMED. The phrase is "dentro de 72 horas de recibir los datos del propietario" in all five keys (`kit/prompts/T799.md:9`; `docs/decisions.md:2972`).
- **TXT-026 sentence:** CONFIRMED. "Sus 2 intentos deben tener al menos 3 horas de diferencia y pueden ser en días distintos." is already in T799's goal, part (d).
- **"El reembolso necesita 2 intentos":** WRONG. T799's goal (a) writes **"El reembolso necesita 2 llamadas."**, matching the English "The refund needs 2 calls." "llamadas" and "intentos" are both 9 characters with no accent, so the report's counts match by coincidence. Its wording does not.
- **Caps:** WRONG as a new finding. T799's goal already raises es `business_no_call_reminder` 4→5 and es `business_no_call_reminder_email` 6→7, plus en `business_no_call_reminder_email` 2→3. Today's es.json still has 4 and 6 only because T799 has not landed. No extra cap work is needed.
- **name_address:** T799 also already changes "la recibe" to "recibe estos datos" (D-5401). The DONE pin `app/intake/src/tests/conversation.rs:289` is T799's.

T799's final counts (verified):

| Key | Units | Segments | Cap after T799 |
|---|---|---|---|
| business_no_call_reminder | 332/335 | 5/5 | 5 |
| business_no_call_reminder_email | 431/434 | 7/7 | 7 |
| unreachable_refunded | 361/364 | 6/6 | 6 |
| unreachable_no_attempts | 386/389 | 6/6 | 6 |
| unreachable_no_one_attempt | 377/380 | 6/6 | 6 |
| name_address | 197/200 | 3/3 | 3 |

All are UCS-2.

## 2. "cliente" → "propietario" in the two T799 texts: ENDORSED (plan change to T799's goal)

- **No rule bans "cliente" here.** WORD-026 bans only `datos de(l) cliente(s)` (`tests/docs/test_introduction_fee_wording.py:60`). D-5837 says the other "cliente" mentions "are not WORD-026 matches and stay". That is a ruling on WORD-026, not on consistency.
- **Why change:**
  - The English says "The homeowner chose email" and "we told the homeowner" (en.json).
  - After T799, each of these two Spanish texts would use "datos del propietario" and "el cliente" for the same person, in one text.
- **Fit and pins:** both fit their caps, and no test pins either string. The only files that hold the phrases are es.json, `help_answers.rs` and its test (grep over the tree).
- **Not changed:** the other 6 business texts that say "cliente" without "propietario" (`pick_withdrawn`, `unreachable_no_talked`, `_no_notice`, `_no_reply`, `_no_emailed`, `pick_live_reply`). Each is internally consistent. They are left for the native-speaker review (TXT-003/TXT-039), except `pick_live_reply`, which item 3 fixes anyway.

| Key | Final Spanish (whole text) | Units | Segments | Cap |
|---|---|---|---|---|
| business_no_call_reminder_email | `{brand_short}: Trabajo {job}: aún no hay llamada. Llame dentro de 72 horas de recibir los datos del propietario, o la tarifa no se puede reembolsar. Llame entre las 8 a.m. y las 8 p.m., hora de Arizona (a cualquier hora en un trabajo de emergencia). Sus 2 intentos deben tener al menos 3 horas de diferencia y pueden ser en días distintos. El propietario eligió correo electrónico; sus 2 intentos de llamada siguen contando para el reembolso.` | UCS-2 435/438 | 7/7 | 7 (T799's) |
| unreachable_refunded | `{brand_short}: Trabajo {job}: su tarifa de presentación se reembolsa. Llamó 2 veces, con al menos 3 horas de diferencia, dentro de 72 horas de recibir los datos del propietario y en el horario de llamadas. Nadie contestó y le avisamos al propietario que usted llamó. ¿Preguntas? Use el formulario Preguntar por un cargo en su página de facturación en https://{domain}/b.` | UCS-2 365/368 | 6/6 | 6 |

## 3. The six "other issues"

| # | Key | Meaning really differs? | Report's Spanish | Verdict / endorsed final Spanish | Units, segments (cap) | Pins / owner |
|---|---|---|---|---|---|---|
| 1 | pick_live_reply | **Yes.** English: "their first name and address". Spanish: "el nombre … del cliente" overstates what is released (first name only). It is not a leak, but it misstates the privacy promise. | OK | **ENDORSE** `{brand_short}: El propietario eligió su cotización para el trabajo {job}. Confirme aquí: {link} Luego verá el nombre de pila y la dirección del propietario en su portal.` ("nombre de pila" is es.json's own term: `quote_list_end`, `text_pick_terms`). | UCS-2 162/165, 3/3 (cap 4) | No Spanish pin. No queued owner of the key. |
| 2 | name_address | Partly. The "la recibe" half is **already fixed by T799** (D-5401). "dirección de su casa" against "street address" was kept on purpose by D-5401 ("stays"). | Rewrites the whole sentence | **NOT ENDORSED** beyond T799. Leave "primer nombre"/"dirección de su casa" for the native-speaker review. | T799: 197/200, 3/3 | T799 owns the pin `conversation.rs:289`. |
| 3 | close_howto | **Yes, mildly.** "lo atiende de inmediato" means "handles it immediately". English is "gets it right away", and CON-018's fixed Spanish is "recibe … de inmediato" (`docs/spec/consent.md:22`). The Spanish overpromises. | "recibe su mensaje de inmediato": correct, but 166/172 → 2 segments for Laftly too | **ENDORSE** the smaller `… y el dueño de {brand} lo recibe de inmediato.` Full text: `{brand_short}: Responda CERRAR {job} para cerrar la solicitud {job}. Si eso no lo resuelve, responda con lo que pasa y el dueño de {brand} lo recibe de inmediato.` | GSM-7 158/164, 1/2 (cap 2) | **Not es.json-only.** The same sentence is `fix_line` in `app/messaging/src/help_answers.rs:171` (T482, DONE). The DONE test `tests/integration/tests/help_answers.rs:147` asserts that close and cost bodies end with it (`:195-227`). All three must change together by a logged spec change, in a harness task with Opus review. |
| 4 | cost_answer | Same as 3. | as above | **ENDORSE** `{brand_short}: Es gratis para usted. El negocio que elija le paga a {brand} una tarifa fija de presentación ($25, $75 o $199, según el tamaño de la cotización). Usted le paga directamente al negocio por el trabajo. Si eso no lo resuelve, responda con lo que pasa y el dueño de {brand} lo recibe de inmediato.` | UCS-2 299/308, 5/5 (cap 5) | as 3 |
| 5 | hold_end_notice | **Slightly.** PICK-043/PICK-045 say only "sends the homeowner no text" (`docs/spec/quotes-pick.md:47`, `:49`), and her portal shows the acceptance end time. "No le avisamos" claims no notice of any kind. | "No le enviamos al propietario un mensaje de texto sobre esto.": correct, 148/151 | **ENDORSE, low priority, shorter:** `{brand_short}: El trabajo {job} queda en espera. Ahora tiene hasta {time} para confirmar. No le enviamos un texto al propietario sobre esto.` | GSM-7 137/140, 1/1 (cap 3) | No pin. No queued owner. |
| 6 | business_auto_reply | **Yes.** "No pudimos leer ese mensaje" (we could not read it) against "couldn't read that as a quote or a reply word". | **WRONG:** "cotización" holds ó, which makes the text UCS-2 at 194/197 units, **3 segments against cap 1**. `test_segments.py` fails it, as I confirmed on the snapshot. | **ENDORSE within cap:** `{brand_short}: No entendimos ese mensaje. Para cotizar, confirmar o cambiar ajustes, use su portal: https://{domain}/b. Escriba AYUDA para recibir ayuda.` | GSM-7 148/151, 1/1 (cap 1) | No pin (inbound tests count rows only, `inbound_first_contact.rs:281-296`). No queued owner. |

Usted forms, no exclamation marks or emoji, no superlatives, and no "comisión" or ranking words appear in any of them. All pass the wording, AI-smell and segment checks on the snapshot.

## Ownership and smallest plan path

- **Who can write es.json:**
  - es.json's owners are T383 and T386 (DONE) and T749 (QUEUED, only retires `business_pick_waiting`).
  - Any task may edit es.json as a shared path, but only its own keys (D-491). T799 changes other keys under the D-5266 exception, and its acceptance says "nothing else in the template files changes".
  - No queued task names `pick_live_reply`, `close_howto`, `cost_answer`, `hold_end_notice` or `business_auto_reply` in its goal.
- **(a) Before T799 is claimed (cheapest):** one plan-change decision adds, word for word, the 2 "propietario" edits plus `pick_live_reply`, `hold_end_notice` and `business_auto_reply` to T799's goal and acceptance. No cap changes, no new owned files, no pins. Then requeue with `--plan-fix`. Each stays `draft: true`.
- **(b) `close_howto` and `cost_answer`:** a new small harness task after T799 (Sonnet worker, Opus review), owning `app/messaging/src/help_answers.rs` and `tests/integration/tests/help_answers.rs`. It changes es.json's two texts, `fix_line`'s Spanish and the test's `fix_line` together, by logged spec change (CLAUDE.md rule 6). Alternatively, T799 takes these 2 files and raises its size budget.

**Cap changes:** none beyond T799's own (es 4→5, 6→7; en 2→3).

Not verified: the report's section 3 (D-5848 chat drafts).
