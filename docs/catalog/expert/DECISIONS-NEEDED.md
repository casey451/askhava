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
