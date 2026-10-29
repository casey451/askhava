# Decisions only Casey can make

Each item has the research summary and a recommendation. The build proceeds on the recommendation until Casey says otherwise (BUILD_BRIEF.md §16: decide and log). None of these is applied in this folder; the expert files follow the catalog as it stands.

## 1. Thirteen trades have a knowledge file and no category

The Grok base covers trades the catalog does not route: water heaters, gas lines, drywall, cabinets and countertops, carpet cleaning, evaporative coolers, garage doors (off, D-404), generators and EV chargers, golf carts, locksmith, RV services, solar, tree and palm trimming. Today a homeowner texting "palm needs trimming" or "water heater is leaking at the bottom" gets "We don't cover <trade> jobs yet" (INT-017) or lands in a near category with a coarse job type (water heater replacement is a licensed-only refused job under plumbing repair, quoted as a diagnostic visit).

Casey's standing direction is "we need to run all service businesses" (D-459) and "nearly every popular Havasu service business type" (Oct 26). A category costs: a `categories.yaml` row, `license_map.yaml` rows, a question set, an expert file, scenarios, and worked fee examples; about one Claude session per category now that the pattern exists.

**Recommendation:** add six categories before launch, in this order of likely Havasu demand: `water_heaters` (R-37, diagnostic_visit, with replacement as range), `tree_palm_trimming` (no ROC class found; city business license; firm), `garage_doors` is off for a legal reason and stays off, `drywall_cabinets_remodel` (B-3/R-10/R-60, range, design: true), `locksmith` (no state license found; firm), `evaporative_coolers` as job types inside hvac_repair rather than a category (already partly there), `solar_generator_ev` later (permit and utility rules make it a Phase 2 category). Leave carpet cleaning, golf carts and RV services as "not offered at launch" until a request count justifies them (OPS-041 counts them).

## 2. Plumbing repair's quote format does not fit two of its own jobs

`plumbing_repair` is `diagnostic_visit` with drain clearing and faucet replacement `firm`. A whole-house re-pipe and an add-or-move fixture job are range quotes everywhere in the trade; a visit-fee quote for a re-pipe tells the homeowner nothing. The expert file maps them to "leak repair" to stay inside the catalog.

**Recommendation:** add job types "re-pipe" and "add or move a fixture" to `plumbing_repair` with `job_type_formats: range` (PICK-053 allows per-job formats). Logged as a catalog change; no other rule moves.

## 3. Swamp coolers, palms and other work that sits between files

`hvac_repair` lists "evaporative (swamp) cooler repair" but the hvac trade file has no swamp-cooler job types; they live in `evaporative_coolers.yaml` (unwired). The expert file adds them as `new_job_type` entries. Palm and tree trimming has no home (item 1). Irrigation repair is split between yard maintenance (no irrigation job type) and pavers and hardscape ("irrigation install").

**Recommendation:** let a question set name a second trade file (`kb_trade_also: [evaporative_coolers]`) so the intake can load it for those job types; add "irrigation repair" to `landscape_maintenance` (firm, one-time). Both are catalog changes.

## 4. Spanish review

Every follow-up and scope line carries a Spanish draft (TXT-003 requires native-speaker review before Spanish goes live). Casey has said he does not speak Spanish and knows no one who does.

**Recommendation:** budget one paid review by a Spanish-speaking Arizona tradesperson or translator (about 3,000 lines, a few hours) before the Spanish path goes live; until then the app's "we continue in English" line (T162) stands. This is a spend decision, so it is Casey's.

## 5. Pest control categories and pigeons

The Department of Agriculture license categories were read from a third-party page (trade KB open question 19); whether exclusion-only bird work needs a license is open (question 13). Pest businesses attest their license (D-417).

**Recommendation:** no change to the attestation model; the pest expert file routes bird exclusion to pest control with the attestation line, and the sign-up checklist lets a business untick it. Casey decides whether to spend a session reading the Department's official category list once the site is reachable.

## 6. How the intake chooses its questions (the biggest finding)

See `TEST-RESULTS.md` for the numbers and `PROPOSED-RULE-CHANGES.md` item 1 for the proposal. In nine judged categories, the plan's scripted intake (the fixed question set in order, one expert follow-up, the set's three photos; INT-039, D-430, D-431, INT-044) produced a card a pro could quote without a call in 54% to 93% of scenarios, against 93% to 100% for the same model with no script and 100% for a human estimator. Tweaking the script (per-job-type bank questions, two follow-ups, expert photo order; tested as `playbook_v2`) made two categories worse. Letting the AI ask its own questions, guided by the expert file and capped at eight (`playbook_v3`), is the variant under test in three categories; its numbers are in the results table.

This is a plan change, not a catalog change: it moves question choice from the catalog's fixed list to the AI, inside the standing rules (facts only, no diagnosis, no price, no advice, 911 text only, cap of eight). The fixed set stays as the fallback when the AI switch is off (AI-008). Casey has to decide because the fixed set was chosen for a reason the plan records (predictable intake, reviewable wording, Spanish templates per question); the trade-off is review surface against quotable cards.

**Recommendation:** adopt the guided intake (item 1 in PROPOSED-RULE-CHANGES.md) for the Phase 2 hand-run test on Casey's own phone, with every AI-written question logged and reviewed in `docs/for-casey-review.md`, and keep the fixed set as the fallback. If the hand-run test shows wording drift, fall back to the fixed set with the per-job-type skip rule (v2 rule 1) only.

## 7. Termite reports for a sale or loan may reach no one who can sign

A.A.C. R3-8-307 lets only a certified WDO applicator at a pest-licensed business complete the WDIIR. The catalog puts "termite report for a sale or loan" under `home_inspection` (fee-free, FEE-045) and routes home-inspection cards to BTR inspectors. Unless an inspector also holds the pest license, the card goes to nobody who can do the job.

**Recommendation:** move the job type to `pest_control` as a fee-free entry (`fee_free_job_types` already exists in the schema), routed through the pest rows in `license_map.yaml`, and leave a note under home inspection that points there. Catalog change plus a `license_map.yaml` row; logged in `docs/decisions.md` when applied.

## 8. Alarm businesses that hold only the BTR certificate get no cards

Statute exempts alarm agents from the ROC (32-1121(A)(17)); `license_map.yaml` requires ROC R-67/CR-67 plus BTR for `alarm_systems`. Most alarm-only businesses in Arizona are BTR-certified and not ROC-licensed, so the category may have no receivers.

**Recommendation:** counsel confirms the exemption; if confirmed, `license_map.yaml` accepts BTR alarm business certificate alone for alarm install and monitoring setup, with the ROC class as an alternative. Until then the expert file follows the map as written.

## 9. Septic transfer inspections are a RESPA settlement service

An R18-9-A316 transfer-of-ownership inspection is required at sale and is a settlement service under 12 CFR 1024.2, the same reasoning that made sale and refinance home inspections fee-free (LIC-015, FEE-045, D-403). `septic_pumping` has "tank inspection" as a paid job type with no sale/no-sale split.

**Recommendation:** split "tank inspection" into "tank inspection (no sale)" and "transfer-of-ownership inspection for a sale" and put the second in `fee_free_job_types`, with the bank's existing "is this for a sale?" wording. Counsel confirms; catalog change when applied.

## 10. Three things homeowners say that are not on the 911 list

D-436 allows only the fixed 911 text, and only for the listed signs. Scenario writers and expert-file writers hit three cases that are urgent in the trade and not on the list: a shock or tingle in pool water, a child who ate paint chips (Poison Control, not 911), and a burning smell with no smoke (dryer, outlet). Today the intake says nothing special and continues; the card carries the fact and `urgent: true`.

**Recommendation:** keep D-436 as written (the AI judges no safety) and add "shock or tingle in or near water" to the 911 list in the changes file, since it is an electrocution sign. Leave paint chips and smell-without-smoke as urgent facts on the card. Casey's call because it edits the 911 list.

## 11. Dumpsters, HOA and business bins

Bin-cleaning requests come from HOAs and small businesses as often as homes; the service is homeowner-only (PRIV and the sign-up flow assume a home). Same question for pressure washing of commercial pads and holiday lights on a storefront.

**Recommendation:** homes only at launch; the intake's coverage line says "homes only for now" and OPS-041 counts the requests. Casey decides whether and when business customers are in scope.

## 12. Water heaters, EV chargers, misting, RV roofs: jobs with a trade file and no category row

Water heater replacement is refused under plumbing repair (licensed-only) though `water_heater.yaml` exists; EV chargers sit under "new circuit" in electrical (`generator_ev.yaml` unwired); RV roof sealing is a bank option in car detailing with no category; misting has no job type (expert file adds `new_job_type: misting_system`). These are the sharp edges of item 1.

**Recommendation:** fold into item 1 (water heaters as a category), add "EV charger circuit" as an electrical job type (range), drop the RV roof option from the car detailing bank until an RV category exists, and add "misting system" to water treatment with an R-21 license-map row (see UNVERIFIED.md 5).
