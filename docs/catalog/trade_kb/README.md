# Ask Hava: Trade Knowledge Base (Lake Havasu City, AZ)

**Ask Hava is not a contractor.** Ask Hava asks a homeowner a few questions, builds an anonymous job card, and sends it to opted-in local businesses, which quote in their own words. Ask Hava never diagnoses a problem, prices a job, promises results, suggests or ranks a business, gives repair advice or supervises work. Licensing facts in this package are for routing and wording only; they are not legal advice. Confirm current license status with the Arizona Registrar of Contractors (ROC) or the agency named.

## What this package is
- **37 trade files** (`trades/*.yaml`), one per trade, all in the same shape (`foundation/schema.yaml`, v2): 309 job types, 370 test messages.
- **Shared foundation** (`foundation/`): Arizona license master, all-reasons/edge-case playbook, photo playbook, schema, and the numbered source list (S1-S51).
- **`test_set.csv`**: all 370 test messages in one table (id, message, trade, job_type, emergency, first_question).
- **`trade_summaries.md`**: the short narrative for each trade.
- Scope: Lake Havasu City and nearby Mohave County. Utilities: **UniSource Energy Services (UES)** for electric and natural gas (emergency/outage line [number removed, see source]); **Lake Havasu City** for water and sewer.

## How to use it with an AI assistant
1. Load `foundation/schema.yaml`, `foundation/part1_all_reasons.md`, `foundation/az_license_master.md` and `foundation/part3_photo_playbook.md` as standing context. Add `trades/<slug>.yaml` for the trade(s) in play (each file is under 90 KB).
2. **Safety first.** Check the message against `emergency_signals` (and `photo_red_flags` for photos). If one matches, give its `safety_step` word for word, with its source, before any intake question. `then_route_to` names the trade to card after everyone is safe ("none" = no card).
3. **Pick the trade and job type** with `triggers_en` / `triggers_es`. A trigger with `job_type: null` only picks the trade; ask the first question to pick the job type. A message can belong to another trade: test messages show the right trade + job type.
4. **Ask `questions` in order** (highest price impact first), respecting `ask_if`. Request the photos in `photos_to_request`; describe photos only with neutral, observable wording (`photo_cues`, part3).
5. **Build the card** like `sample_card`: request ID (e.g. REQ-0142), general area, facts only. No names, phones, emails, exact addresses, plates or gate codes.
6. **License and handyman lines** come only from `license_classes` and `handyman_ok` (A.R.S. 32-1121(A)(14) and (D)). Never tell a homeowner a business is licensed; tell them how to check.
7. **Test** with `test_set.csv`: for each message the assistant should land on the listed trade/job_type, treat `emergency` correctly, and ask something close to `first_question`.

## File index (trade -> file, job types, license classes)
| Trade | File | Job types | License classes / agency (one line) |
|---|---|---|---|
| Appliance Repair | `trades/appliance_repair.yaml` | 8 | R-63 CR-63 R-37 R-11 |
| Boat & dock services | `trades/boat_dock.yaml` | 10 | Boat repair: no ROC [judgment]; docks: Army Corps Section 10, city code 12.08.100, BLM/Reclamation [open question]: R-11 CR-11 C-11 A-7 A-11 R-17 R-56 |
| Cabinets & Countertops | `trades/cabinets_countertops.yaml` | 7 | R-60 CR-60 B-3 R-61 R-48 R-37 |
| Carpet Cleaning | `trades/carpet_cleaning.yaml` | 6 | Cleaning: no ROC [judgment]; re-stretch/repair: R-8 |
| Drywall | `trades/drywall.yaml` | 6 | R-10 CR-10 B-3 R-61 |
| Electrical (panels, outlets, wiring, lighting) | `trades/electrical.yaml` | 12 | R-11 CR-11 R-16 |
| Swamp coolers (evaporative coolers) | `trades/evaporative_coolers.yaml` | 11 | R-39 R-39R CR-39 CR-58 C-58 CR-79 R-42 R-45 |
| Flooring | `trades/flooring.yaml` | 7 | R-8 CR-8 B-3 R-48 CR-48 R-60 |
| Garage Doors | `trades/garage_doors.yaml` | 9 | R-7 R-60 B-3 R-61 R-14 R-11 |
| Gas lines (natural gas and propane piping) | `trades/gas_lines.yaml` | 8 | UniSource owns meter/service line: R-37 CR-37 CR-77 R-37R |
| Generators and EV chargers | `trades/generator_ev.yaml` | 6 | R-11 CR-11 R-37 CR-77 |
| Golf cart repair | `trades/golf_cart.yaml` | 8 | Cart repair: no ROC [judgment]; charger circuits: R-11 |
| Gutter & Solar Panel Cleaning | `trades/gutter_solar_cleaning.yaml` | 6 | Cleaning: no ROC [judgment]; gutter/solar repair: R-45 R-11 CR-11 |
| Handyman | `trades/handyman.yaml` | 10 | B-3 R-61 R-62 R-11 CR-11 R-37 R-63 R-16 I-10C I-10D |
| Home Inspection | `trades/home_inspection.yaml` | 6 | No ROC class; Board of Technical Registration (A.R.S. 32-122.02); WDIIR: AZDA Pest Management |
| House Cleaning | `trades/house_cleaning.yaml` | 6 | No ROC class [judgment]; city business/tax licensing may apply |
| Heating & cooling (AC, heat pumps, mini-splits, rooftop units) | `trades/hvac.yaml` | 13 | R-39 R-39R CR-39 C-39 CR-79 C-79 R-45 CR-45 CR-58 C-58 |
| Junk Removal | `trades/junk_removal.yaml` | 7 | Hauling: no ROC [judgment]; demo and disconnects: R-57 R-11 R-37 |
| Landscaping & irrigation | `trades/landscaping_irrigation.yaml` | 10 | R-21 CR-21 C-21 R-37 |
| Locksmith | `trades/locksmith.yaml` | 7 | No state locksmith license found [unverified]; door work R-7/R-60, access systems R-67: R-7 R-60 R-67 |
| Moving Help | `trades/moving.yaml` | 6 | No state mover license; Arizona Corporation Commission registration, AZ DPS enforcement; FMCSA interstate: R-37 R-63 R-11 |
| Painting | `trades/painting.yaml` | 9 | R-34 CR-34 B-3 R-8 R-10 |
| Pavers & concrete | `trades/pavers_concrete.yaml` | 9 | R-21 CR-21 C-21 R-9 CR-9 C-9 R-48 R-31 R-62 B-5 B-6 R-36 |
| Pest control | `trades/pest_control.yaml` | 9 | No ROC class; AZ Dept of Agriculture Pest Management Division (A.R.S. 3-3615) |
| Plumbing (leaks, drains, fixtures, pipes) | `trades/plumbing.yaml` | 13 | R-37 R-37R CR-37 C-37 CR-77 C-77 CR-80 A-12 |
| Pool cleaning | `trades/pool_cleaning.yaml` | 7 | R-6 CR-6 R-36 B-5 B-6 |
| Pool & spa repair | `trades/pool_spa_repair.yaml` | 10 | R-6 CR-6 C-6 B-5 B-6 KA-5 KA-6 A-9 A-19 B-10 R-36 R-37 R-11 |
| Roofing | `trades/roofing.yaml` | 11 | R-42 CR-42 R-45 I-10C I-10D |
| RV services | `trades/rv_services.yaml` | 10 | RV repair: no ROC [judgment]; site hookups and covers: R-11 R-37 R-3 |
| Security & Alarm Systems | `trades/security_alarm.yaml` | 6 | BTR alarm business/agent certification (A.R.S. 32-122.05/.06) plus ROC: R-67 CR-67 R-16 |
| Septic pumping & service | `trades/septic.yaml` | 7 | ADEQ septage hauler vehicle license + Mohave County permit; repairs/new systems: R-41 CR-41 C-41 A-12 R-37 CR-80 |
| Solar (PV panels, inverters, batteries, solar water heating) | `trades/solar.yaml` | 9 | R-11 CR-11 R-37 CR-77 C-77 R-37R CR-78 C-78 B-6 |
| Tree & palm trimming | `trades/tree_trimming.yaml` | 7 | No tree-specific ROC class found [open question]; ISA arborist certification is voluntary |
| Water heaters (tank and tankless, gas and electric) | `trades/water_heater.yaml` | 11 | R-37 R-37R CR-37 CR-77 C-77 R-11 CR-11 |
| Water softeners, RO and filters | `trades/water_treatment.yaml` | 9 | R-54 CR-54 R-37 CR-37 R-37R |
| Window Cleaning | `trades/window_cleaning.yaml` | 5 | Cleaning: no ROC [judgment]; glass repair: R-65 R-7 |
| Windows & Doors | `trades/windows_doors.yaml` | 8 | R-7 CR-7 R-60 B-3 R-61 R-65 CR-65 |

Class codes: R- residential (A.A.C. R4-9-103 [S2]), CR-/KA- dual (R4-9-104 [S3]), C-/A- commercial (R4-9-102 [S1]), B-/B-3 general residential, I-10C/I-10D AZ Dept of Housing manufactured-home installer. Full scopes: `foundation/az_license_master.md`.

## Conventions
- **`safety_step_first`**: a test message with `first_question_id: safety_step_first` means: give the matching safety step first; ask intake questions only after everyone is safe. Safety steps come only from official sources (UES, fire dept, NFPA, CPSC, EPA, CDC, NWS, Poison Control [number removed, see source], manufacturer) and carry a source.
- **`[judgment]`**: not backed by a cited source; the research team's reasoning. Treat as a default, not a fact.
- **`[unverified]`**: we looked but could not confirm in an official source (often because the official site blocked automated reading).
- **`[open question]`**: a known gap; listed below.
- **S-numbers** (`[S1]`-`[S51]`) point to `foundation/sources_foundation.md`. Trade-specific source tags (e.g. `[T-...]`, `[ARS-1170]`, `[EPCOR-LH]`) and plain URLs are listed in each file's `sources`.
- **Slugs**: lower_snake_case, file name = `trade` value. Test IDs are `<slug>_tNN`. `job_type: unknown` is reserved for one-word messages ("boat", "pool", "septic"); the first question picks the job type.
- **Wording**: banned words and phrasing rules are at the top of `foundation/schema.yaml`. No hard calendar dates; seasons are fine.

## OPEN QUESTIONS / needs verification (merged from all workers)
**Blocked official sources (read from copies or snippets instead)**
1. **AZ Secretary of State rule PDFs and ROC site** (apps.azsos.gov, roc.az.gov) were blocked; ROC class scopes were read from the Cornell LII / Justia copies of R4-9-102/103/104. Re-check against the official text, and check the ROC classification list for any newer classes.
2. **AZ Dept of Agriculture** and **ADEQ** pages were blocked; pest categories (third-party) and the ADEQ septage-hauler page (snippet only) need an official read.

**License classes**
3. **CR-41 typo**: the copy of R4-9-104 repeats the welding (C-56/R-56) text under "CR-41 Septic Tanks and Systems". Treated as dual C-41/R-41 [judgment].
4. **Solar PV**: whether ROC requires a separate solar qualification or exam on top of R-11/CR-11 for PV (A.R.S. 32-1170.01 refers to a solar contractor qualification) is unverified; CR-78 / R-37R solar liquid-only classes are not in the license master.
5. **Garage doors**: no garage-door class found; R-7 / R-60 (door), R-11 (opener circuit), R-14 (automatic gates) is [judgment].
6. **Locksmith**: no Arizona state locksmith license found; third-party sources only.
7. **Appliance repair**: whether a repair-only visit on an existing appliance needs R-63 is unverified.
8. **Generators / EV chargers**: R-11 plus R-37 for the gas side is [judgment].
9. **Docks and boat lifts**: no dock class found (A-7 / A-11 / R-17 / R-56 are guesses). Army Corps Section 10, Bureau of Reclamation and BLM shoreline roles need confirming; Lake Havasu City Code 12.08.100 is written for commercial docks and the rules for private residential docks are not confirmed.
10. **Pool classes**: KA-5/KA-6 and A-9/A-19 are in the pool file but not in the license master.
11. **No-license trades** (cleaning, junk hauling, moving labor, RV, golf cart and boat repair, tree trimming): "no ROC class" is [judgment] based on no matching class; boat repair needing no ROC license is a judgment call. City business/tax licensing may still apply.
12. **Manufactured-home roofs**: how ADOH (I-10C/I-10D, A.R.S. 41-4047/4048) and ROC R-42 split roof work on manufactured homes needs confirming.
13. **Pigeon / bird netting**: whether exclusion-only work (no pesticide or trap) counts as pest management needing an AZDA license is open.

**Local rules and utilities**
14. **Water provider**: Lake Havasu City is the water provider in the city; some files mention EPCOR (water-quality data and an "if that is their provider" fallback). Confirm which areas, if any, EPCOR serves near Lake Havasu.
15. **Septic share**: how many homes still use septic (city sewer conversion finished around 2011; Desert Hills and other county areas still use septic) is not confirmed.
16. **Alarm city code**: Lake Havasu City Code Ch. 8.12 (alarms, false alarms, possible user permits) details not confirmed.
17. **Golf carts on city streets**: street rules in LHC came from a search snippet only.
18. **Permits**: driveway work in the city right-of-way, sewer hookup permits, green-waste rules and dump fees, and watering rules or rebates were not checked.
19. **Pest category codes** (B1-B9 style) came from a third-party source.

## Folder layout
```
README.md
trade_summaries.md
test_set.csv
foundation/  az_license_master.md, part1_all_reasons.md, part3_photo_playbook.md, schema.yaml, sources_foundation.md
trades/      37 trade files (<slug>.yaml)
```
