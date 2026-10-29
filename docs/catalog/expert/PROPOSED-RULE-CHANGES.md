# Proposed rule and catalog changes (not applied)

Nothing in this folder changes a rule, a category, a license-map row or a question set. The expert files follow the catalog as it stands; where the catalog has no fit, the file picks the nearest job type and this document records the better fit. Each item names the rule or file it would change and the evidence. The changes file and the brief stay authoritative until Casey accepts an item and it is logged in `docs/decisions.md`.

Items are grouped: intake rules (1 to 6), catalog job types (7), question bank (8), license map (9), card and template (10), harness (11).

## Intake rules

### 1. Let the intake choose its questions, guided by the expert file (INT-039, D-430, D-431, INT-044)

**Today:** the intake asks the category's fixed question set in order, skips only what the opening answered, may ask one expert follow-up, and asks the set's three photos.

**Evidence:** `TEST-RESULTS.md`. Across nine judged categories the scripted intake produced a quotable card (score 2 or 3 from a blind three-person trade panel) in 54% (electrical) to 93% (hvac repair) of scenarios; the same Haiku model under the standing rules with no script scored 93% to 100%; a human estimator scored 100% everywhere. Traps (the hidden condition the scenario planted) surfaced on 21% to 60% of scripted cards. Most compliance hits in the scripted run were re-asked questions (C11): the script asks the bank question even when the opener covered it. Tweaking the script (item 2 below, run as `playbook_v2`) made plumbing worse (71% to 62%) and house cleaning worse (71% to 54%). A guided free intake (`playbook_v3`: the AI asks its own questions using the expert file's ranked must-know facts, cap of eight, keep stated numbers, blanks say "not asked") is reported in the same table for the three categories it ran in.

**Proposed rule:** for a category with an expert file, the intake AI picks its questions from the job type's `must_know` list in rank order, reusing bank and follow-up wording where it fits, stops when the facts are covered, never exceeds eight questions including timing and photo asks, and asks the timing question only when the opener did not give it. The fixed set remains the fallback when the AI switch is off (AI-008) and is what the Spanish templates cover; AI-written questions in Spanish wait for TXT-003 review. Every AI-written question is logged for review (`docs/for-casey-review.md`) during Phase 2.

**Risk:** less predictable wording; more review surface. Mitigation: the cap, the standing rules, the `never_say` lists and the compliance judge codes (C1 to C12) applied to every logged intake in Phase 2.

### 2. If the script stays: bank questions apply per job type (INT-039)

Ask a bank question only when the job type's `must_know` names it (`bank_question`) or the question is `emergency: true`. Evidence: in plumbing, the floor and fixture questions were asked for clogs and faucet swaps where they changed nothing; in electrical, "what is going on" was re-asked after the opener answered it (C11, 15 hits). Tested as `playbook_v2` rule 1; on its own it cut wasted questions but, with rule 3 below, dropped facts (plumbing lost the floor question it still needed for leaks). Adopt only with item 1 rejected, and without rules 3 and 4.

### 3. Two follow-ups instead of one (D-430)

Tested in `playbook_v2`; did not raise quotable rates. Not recommended on the evidence; recorded so it is not re-tested.

### 4. Photo asks follow `photos_priority` (INT-044)

INT-044 already says "in order of usefulness"; the sets fix three ids per category. The hvac judge found the expert file's better photo (the outdoor unit label) was never asked because it is not in the set's three. Proposed: the question set's `photos` becomes the fallback; when an expert entry has `photos_priority`, those ids (up to three) are the asks. In `playbook_v2` the run claimed photos it never sent (a harness fault, not a rule fault); re-test under item 1.

### 5. Keep stated numbers on the card (CARD-007)

Judges in electrical, landscape and roof repair scored "facts not stated" (C6) when the card turned "built in 1974" into "1970s" or "about 1,800 sq ft" into "1,500 to 2,000". Proposed wording for CARD-007: the card carries the homeowner's number as said, and the band only when she gave a band. Already written into `playbook_v3`.

### 6. A routed or two-trade request still produces a card (INT-039, routing)

Landscape s15 (an active irrigation flood routed to plumbing) and electrical s11 (outlet plus AC) stopped at the route with no card. Proposed: when the intake routes a request to another category, it continues in that category with that category's expert file and writes the card there; a two-trade request writes the card for the first category and names the second request in a card line so the business sees it (a second card is a later feature). Written into `playbook_v3` rule 6.

### 7. "Not asked", never a default (CARD-007, card templates)

Pool repair's judge counted eleven C6 hits that were the card template's defaults ("not compared", "not checked"). Proposed: a blank card field reads "not asked" or "not said". The templates in the expert files already use this wording where the judge caught it; the rule belongs in CARD-007 so the app's renderer does the same.

## Catalog job types (`docs/catalog/categories.yaml`)

Each is a job type a homeowner asks for in Havasu that has no row, so the expert file maps it to the nearest category job type and the bank label on the card is loose. Format in brackets is the proposed `job_type_formats` entry where it differs from the category's format.

| Category | Add | Why | Expert entry today |
|---|---|---|---|
| plumbing_repair | re-pipe [range]; add or move a fixture [range]; water heater (see DECISIONS 1) | visit-fee quote tells the homeowner nothing for a re-pipe | mapped to "leak repair" |
| electrical | EV charger circuit [range]; low-voltage yard lighting | both common; EV sits under "new circuit" | new_circuit |
| hvac_repair | swamp cooler service; swamp cooler replace or convert; mini-split install (or in hvac_replacement); tune-up [firm] | category lists cooler repair, trade file has none; shops quote tune-ups flat, not as a visit | new_job_type cooler_* |
| hvac_replacement | condenser only; mini-split install | scenarios expected "ac_replacement" for outdoor-only | new_job_type |
| appliance_repair | electric range or oven repair; appliance install; dryer vent cleaning (own category exists now) | only gas appliances are refused; electric ranges fall through | nearest |
| alarm_systems | alarm repair or service | only install and monitoring setup exist | install |
| pest_control | bee removal; bird exclusion; bed bugs; rodent trapping (all `one_time_job_types`); termite report for a sale or loan (fee-free; DECISIONS 7) | routing judged wrong in 31% of pest scenarios because these map to "general pest service" | general pest service |
| pool_repair | equipment repair; heat pump | coarse job types force poor fits | nearest |
| pool_cleaning | one-time cleanup (rename "green pool cleanup"); filter clean [firm]; drain and refill [firm] | one-time rebalance has no firm home | green pool cleanup |
| landscape_maintenance | irrigation repair [firm, one-time]; trimming (add to `one_time_job_types`); palm trimming (DECISIONS 1) | "trimming" one-time has no format | nearest |
| house_cleaning | after-remodel clean [range] | firm quote cannot carry a walk-through fee for heavy-condition homes | deep cleaning |
| turnover_cleaning | turnover clean in `one_time_job_types`, or drop the "Once" option | bank offers once, catalog has no one-time entry | nearest |
| roof_repair | job types by roof type (tile, shingle, foam, metal) or a roof-type bank question used for opt-in | a tile-only roofer gets foam cards | nearest |
| gutters | gutter cleaning (no license) | unlicensed cleaners never get cards under the R-42/R-45 rows | nearest |
| painting | roof coating; popcorn-ceiling removal (own set: asbestos test question) | no job type | nearest |
| handyman | caulk and small fixes [firm] | most common ask | nearest |
| solar_screens | blinds and shutters; glass repair (R-65) | window cleaning s14 routed foggy glass here | nearest |
| water_treatment | misting system (R-21 row, item 9); softener repair or service | expert file adds new_job_type | new_job_type |
| septic_pumping | septic repair or new system (R-41); transfer inspection for a sale (fee-free; DECISIONS 9) | pumpers and installers are different licenses | tank inspection |
| boat_detailing | full detail | bank offers it, catalog lacks it | detailing |
| boat_repair | engine service (rename "outboard service") or add inboard/sterndrive | bank engine list omits inboards | outboard service |
| car_detailing | drop "RV roof sealing" from the bank until an RV category exists | belongs to rv_services (unwired) | none |
| pressure_washing | roof wash; dock or ramp | both common here; roof wash licensing in UNVERIFIED 13 | nearest |
| dog_waste_pickup | one-time yard cleanup [firm] | bank offers it | nearest |
| trash_can_cleaning | one-time bin wash [firm] | bank offers it | nearest |
| home_watch | rename "storm check" to "one-time check"; add open-up, close-up, meet-a-vendor, key holding | storm is one reason among many | storm check |
| security_cameras | fix existing cameras | no job type | install |
| epoxy_floors | none; set `kb_trade: painting` so the garage-floor photo (painting p4) is available | flooring KB has no epoxy photos | n/a |

## Question bank (`docs/catalog/question_bank.yaml`)

Questions the judges found a pro always asks and the set lacks; each eats the single follow-up today. Adding them raises the fixed-set count, which is one more reason for item 1.

- electrical: a free-text "anything else" question (handyman and appliance sets have one); skip the symptom menu when the opener answers it.
- hvac_repair: "weak cooling" in the symptom list; home-age question.
- plumbing_repair: house age; what was already tried.
- house_cleaning: merge bedrooms and bathrooms into one size question; add a catch-all.
- pool_cleaning: filter type and chlorine method (salt, tabs, liquid); gate and dog access; off-season months for seasonal homes.
- pool_repair: combine pump brand, speed and age into one question; access question.
- roof_repair: "is water coming in now"; roof age; roof equipment (solar, AC on roof).
- landscape_maintenance: "anything the crew should watch for" (dogs, gates, pools); photos should be the yard-wide shots p1 and p6, not the irrigation photos; keep stated square footage.
- pest_control: photos add solar panels (p4), droppings (p5), rock yard (p6).
- local_moving: option "Under 50 miles" to "Under 50 miles, in Arizona"; heavy-items question.
- handyman: promote `fu_touches_utility` (wiring, gas, water, AC) into the set.
- painting: "about when was the home built?" (EPA lead-safe RRP applies before 1978).
- dryer_vent_cleaning: "under the home" as a vent-location option; run length; fuel.
- dog_waste_pickup: "every other week" frequency; move the last-pickup question into the set.
- trash_can_cleaning: bin condition and curb-stay as set questions; add photo p4 (bin area).
- holiday_lights: areas question; "rent each year" option; photos p3 and p5.
- window_cleaning: service question multi-select (inside, outside, screens); count beyond "Over 20".
- car_detailing: inside-condition question; interior and paint close-up photos.
- boat_detailing: interior photo (the set's engine-label photo is useless for detailing).
- appliance_repair: fuel (gas or electric) and warranty questions; hookups photo p4.
- donation_pickup: condition question; floor or elevator.
- estate_cleanout: hazards and appliances question; "garage or shed only" size option.
- home_watch: home size; thermostat and storm photos.
- security_cameras: size question (number of cameras, wired or wireless) so wired multi-camera jobs do not reach handymen; drop the smoke-alarm photo p3.
- insulation: none beyond photos [p1_attic_wide, p2_existing_depth, p3_access] already wired.
- hvac_replacement: mini-split spot and panel photos.

## License map (`docs/catalog/license_map.yaml`)

Only the license map routes; these rows are proposals for Casey and counsel (see UNVERIFIED.md for the open readings).

- alarm_systems: accept the BTR alarm business certificate alone (32-1121(A)(17)); DECISIONS 8.
- pest_control: termite report for a sale or loan routed to pest-licensed businesses; DECISIONS 7.
- water_treatment / misting: add R-21 (scope item 11 covers residential misting up to 1,000 PSI).
- electrical / low-voltage yard lighting: add R-21 (in its scope) as a receiver.
- pool_repair: B-10 hot-tub-only businesses receive hot tub job types.
- gutters: a no-license row for gutter cleaning.
- hvac_repair: "furnace repair" cards for heat-pump heating faults may hide from R-39R A/C-only holders; map heat-pump heating to the AC rows or add a heat-pump job type.
- insulation: roof vents (R-42) and attic fans (R-11) as alternate receivers for those job types.
- septic: R-41 for repair and new systems; DECISIONS 9.
- boat_repair: the refused dock/lift row has no receiver; add R-9 / marine contractor or drop the row.
- painting: whether R-34 painters receive epoxy-floor cards.

## Card and templates

- Card blanks read "not asked" or "not said" (item 7).
- Card keeps the homeowner's numbers (item 5).
- The card header line "house" appeared in nine pool-cleaning scenarios where no one said it: the renderer should write "home" only when the homeowner's words or the bank answer say so.
- Permit lines ("Permit-type work: yes") come off every template; they were a judgment the AI is not allowed to make (fixed in `plumbing_repair.yaml`; check the rest at the next pass).

## Harness

- Scenario files and expert files were written independently on purpose; the mismatches listed in the eval notes (water heater, palm trimming, swamp coolers, faucet replacement, electric ranges) are the catalog gaps above, not writer errors. Reconcile scenario `expected.job_type` only when the catalog changes.
- The 911 hold: the run prompt now sends the fixed text and waits for SAFE; a scenario whose facts do not say everyone is out stops there (hvac s11). Scenario writers should state whether the household is out so the hold can resolve.
- Run prompts say "use only file tools; never call a scheduling or messaging tool" after two sub-agents created stray reminders.
