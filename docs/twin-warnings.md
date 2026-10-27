# Twin warnings and the hard-failing Twin check

Written in kit step K5 (`BUILD_BRIEF.md:599`; `sources/02_build_plan/Kit_Plan_Before_Coding.md:34`). It answers stress-test finding F03 (`sources/04_reviews/ChatGPT_Build_StressTest_Report.md:62`): the twin reports real violations and still exits 0, so a green `Twin` check proves nothing (`BUILD_BRIEF.md:475`). This file explains every baseline matching warning, gives each one a disposition (resolve, document or reject), and defines the rule and the harness change that make the `Twin` check fail on any non-waived invariant failure. Decisions are D-101 to D-110 in `docs/decisions.md`. The test plan around it is `docs/test-design.md`.

"The twin" is `sources/01_spec/hhq_twin/` (read-only). Line citations into it are `path:LINE`.

---

## 1. How this was investigated

- The twin was copied to a scratch directory outside the repository and run there, because `twin.py` writes `results/` next to itself (`sources/01_spec/hhq_twin/twin.py:1178`) and `sources/` is read-only. Nothing in `sources/` changed.
- Data: only the twin's own simulated people and businesses (`sources/01_spec/hhq_twin/README.md:5`). No real person's details were used or produced.
- Interpreter: Python 3.11.17 in the build VM (the twin is standard library only; CI uses 3.12).
- **Determinism.** `python3 twin.py all 5` run twice, with `PYTHONHASHSEED=0` and `PYTHONHASHSEED=12345`, printed identical output, and that output matched the committed `sources/01_spec/hhq_twin/results/console.txt` exactly (no diff). The twin seeds its own random generator per run (`sources/01_spec/hhq_twin/twin.py:48`), so a scenario and seed always reproduce the same instances. The full run takes about 73 seconds.
- **Stress-test numbers reproduced.** F03's figures come from small seed counts: `python3 twin.py normal_fixed 1` prints 6.0 `qualified_business_left_out` (the "6 business left out" warnings, `sources/02_build_plan/Kit_Plan_Before_Coding.md:34`), and `python3 twin.py vendor_chaos_fixed 2` prints 4.5 left out and 3.5 outside-license per run. With the committed 5 seeds the rates are 5.4 and 2.8 (`normal_fixed`) and 5.2 and 2.4 (`vendor_chaos_fixed`). Both commands exit 0.
- **Cause analysis.** A scratch script re-ran each scenario with `twin.build` and `twin.check` (`sources/01_spec/hhq_twin/twin.py:1073`, `sources/01_spec/hhq_twin/twin.py:688`), then, for every warning, read the request's two AI readings from the off-database intake log, the plain-keyword matches, the intake questions sent and the homeowner's replies, and the job types the app finally stored. Every warning in `normal_fixed` and `vendor_chaos_fixed` (seeds 1–5) fell into one of the causes in section 3; none was left unexplained.

**Reproduce one warning** (from the repository root; scratch copy so `sources/` stays untouched):

```
cp -r sources/01_spec/hhq_twin "$TMPDIR/hhq_twin" && cd "$TMPDIR/hhq_twin"
python3 twin.py normal_fixed 1        # prints 6.0 x qualified_business_left_out, exits 0
python3 -c "import twin,json; c=twin.deep_update(json.load(open('twin_config.json')), json.load(open('twin_scenarios.json'))['normal_fixed']['overrides']); s=twin.build(c,1); s.run(c['days']); r=s.app.requests['1247']; print(r['original_text'], r['trade'], r['job_types'])"
                                      # AC blowing warm air 86405 hvac ['hvac_other']
```

## 2. What the two matching checks compare

- `job_card_outside_license` (`sources/01_spec/hhq_twin/twin.py:858`): a card went to a business whose license class does not allow the request's **true** job type.
- `qualified_business_left_out` (`sources/01_spec/hhq_twin/twin.py:877`): a business that was active, legitimate and licensed when the first card went out, serves the town, whose license allows the **true** job type and which ticked that job type, got no card.

Both judge against the simulated person's true job (`_sim_truth`), not against what the app read. The app routes on what it read: two AI readings, plain keyword rules, and the homeowner's answers to the trade question and the "Which fits best?" question (`sources/01_spec/hhq_twin/app.py:586`–`sources/01_spec/hhq_twin/app.py:606`, `sources/01_spec/hhq_twin/app.py:704`–`sources/01_spec/hhq_twin/app.py:731`), then sends a card to every business whose license covers every candidate job type and which ticked one of them (`sources/01_spec/hhq_twin/app.py:762`–`sources/01_spec/hhq_twin/app.py:767`; CARD-001). So every matching warning is a case where the job types the app stored differ from the truth.

**A blind spot found on the way.** The outside-license check imports the app's own license table (`from app import CLASS_ALLOWS` at `sources/01_spec/hhq_twin/twin.py:853`, used at `sources/01_spec/hhq_twin/twin.py:858`), and the simulated businesses tick job types from that same table. In a scratch copy, the AC-only sub-license `R-39R-ac` was given `heating_repair` on purpose (the LIC-035 mistake): 81 heating-repair cards went to AC-only licenses over `normal_fixed` seeds 1–5, yet the twin's outside-license count moved only from 2.8 to 3.4 per run (the random path shifts with any rule change), because the check reads the same broken table and so cannot flag any of the 81. This is a scratch-copy observation, not a committed test; the gate's self-tests (6.5) make it one. The oracle shares the code it is checking, which is the circular-oracle risk of F07 (`sources/04_reviews/ChatGPT_Build_StressTest_Report.md:66`). Disposition: **resolve** (D-107): the gate's license oracle is an independent table, never imported from the code under test.

## 3. The six baseline warnings

`normal_fixed`, seed 1. All six have one cause, C1.

| # | Request (true job) | Homeowner's first message | Both AI readings | Business left out (license; job types ticked) |
|---|---|---|---|---|
| 1 | 1044 (handyman, doors) | "Fix sticking door 86406" | plumbing / plumbing_other | Lake Handyman Co 14 (HANDYMAN; doors, drywall, mounting) |
| 2 | 1247 (hvac, ac_repair) | "AC blowing warm air 86405" | electrical / fixture_install | Lake Hvac Co 7 (R-39R-ac; ac_repair, system_replacement, thermostat) |
| 3 | 1247 | same | same | Lake Hvac Co 6 (R-39; ac_repair, heating_repair, system_replacement, thermostat) |
| 4 | 1268 (hvac, ac_repair) | "AC blowing warm air 86405" | electrical / fixture_install | Lake Hvac Co 7 |
| 5 | 1268 | same | same | Lake Hvac Co 6 |
| 6 | 1284 (hvac, heating_repair) | "Heater makes a loud rattle 86406" | plumbing / leak_repair | Lake Hvac Co 6 |

**What happened, each time:**

1. Both AI readings named the same wrong trade (the fake model repeats its own mistake on a re-read, `sources/01_spec/hhq_twin/vendors.py:124`).
2. The plain keyword trade check disagreed, so the app asked "is this mainly 1) … or 2) …?" (`sources/01_spec/hhq_twin/app.py:591`–`sources/01_spec/hhq_twin/app.py:596`). The homeowner answered correctly ("handyman", "it's the AC", "2").
3. With a trade chosen, `_job_types_for` throws away every type found so far, including the plain-keyword job type ("door" → doors, "warm air" → ac_repair, "heater" → heating_repair, added at `sources/01_spec/hhq_twin/app.py:713`–`sources/01_spec/hhq_twin/app.py:715`), keeps only AI readings of the chosen trade (there are none, both readings were another trade) and falls back to `<trade>_other` (`sources/01_spec/hhq_twin/app.py:722`–`sources/01_spec/hhq_twin/app.py:726`).
4. With the job stored as `hvac_other` or `handyman_other`, the AC-only license does not cover it and the other businesses never ticked "other", so only a business that ticked "other" got the card: one card each for 1247, 1268 and 1284, which all ended with no quotes, and three for 1044.

**Disposition: resolve (D-102).** This is a bug in the twin's reference rules, and it breaks two rules the spec already has: plain keyword rules are "an independent second opinion on trade **and job type**" (AI-011; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:119`), and every qualified, opted-in business gets the card (CARD-001; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:13`). The rule for the build: a trade answer narrows the candidates to that trade's job types, but the plain-keyword job types of the chosen trade stay; `<trade>_other` is used only when neither the readings nor the keywords give a job type of that trade, and then the "Which fits best?" question lists that trade's job types.

**Observed in a scratch copy** (not a committed test; the stage 1 twin task proves it through the gate): re-adding the chosen trade's keyword job types after a trade answer (three lines in `_job_types_for`) removed all six; `normal_fixed` seed 1 then reported no problems at all, and cause C1 disappeared from seeds 1–5 of both scenarios.

## 4. Every matching warning in the v9.3 scenarios, by cause

Counts are instances over seeds 1–5 (divide by 5 for the per-run rate the twin prints).

| Cause | What happens | `normal_fixed` left out / outside | `vendor_chaos_fixed` left out / outside | Disposition |
|---|---|---|---|---|
| **C1** | A trade answer drops the plain-keyword job type; the job is stored as `<trade>_other` (section 3) | 18 / 0 | 11 / 0 | **Resolve** (D-102) |
| **C2a** | The homeowner picked the wrong numbered trade (the simulated person answers wrongly about 5% of the time, `sources/01_spec/hhq_twin/twin.py:258`) | 8 / 11 | 0 / 0 | **Document** (D-106): the homeowner's own answer decides; narrowed further by D-105 |
| **C2b** | The re-ask "Sorry, which one? Reply 1 or 2." (`sources/01_spec/hhq_twin/app.py:511`) does not repeat the options, so the reply is a guess | 1 / 2 | 1 / 3 | **Resolve** (D-103) |
| **C2c** | A word answer is misread: "just a small repair" matches heating/AC because "rep**air**" contains "air" (`sources/01_spec/hhq_twin/app.py:67`, `sources/01_spec/hhq_twin/app.py:507`), and "small repair" never matches the handyman option the question itself offered | 0 / 0 | 3 / 3 | **Resolve** (D-103) |
| **C2d** | The two AI readings named two different wrong trades; the question offered only those two, so the keyword trade (the right one) was never offered (`sources/01_spec/hhq_twin/app.py:591`) | 0 / 0 | 3 / 2 | **Resolve** (D-104) |
| **C2e** | Both readings agreed on a wrong trade and no question was asked, because the keyword trade check matched several trades including the AI's ("Water heater not getting hot" matches plumbing "water" and heating/AC "heat") | 0 / 0 | 4 / 4 | **Resolve** (D-105) |
| **C3** | The homeowner picked the wrong job type in "Which fits best?" (about 10% wrong, `sources/01_spec/hhq_twin/twin.py:250`) | 0 / 1 | 4 / 0 | **Document** (D-106) |
| **Total** | | 27 / 14 | 26 / 12 | |

Scratch-copy observation, not a committed test (the stage 1 twin task proves it through the gate): after C1, C2b and C2c were fixed in the scratch copy, the remaining instances were only C2a, C2d and C2e, as expected (the run's random path shifts with any rule change, so the counts move: `normal_fixed` 2.4 left out and 1.8 outside per run, `vendor_chaos_fixed` 1.4 and 2.6). C2d and C2e were not patched in the scratch copy, because the fix needs the simulated homeowner to read three options and a catalog-based keyword list; they are left for the stage 1 twin task, which proves them with the gate.

**Why C2a and C3 are documented, not resolved.** The homeowner is asked because "they know their job" (`sources/01_spec/hhq_twin/app.py:601`; INT-013, INT-014); the app cannot know better than an explicit wrong answer, and the oracle knows the truth only because it wrote the person. Two limits keep this from becoming a loophole:

- after D-105, a wrong trade answer can't send a job whose words name a licensed job type to a license that doesn't cover it: the keyword job type stays a candidate, the candidates then span licenses, and "Which fits best?" is asked (INT-014). Only a second wrong answer gets through;
- the waiver needs evidence per instance (section 6.4): the simulated homeowner must have recorded that it answered wrongly, and the stored job type must be the one it chose. A warning with no such record is not waived.

**Vendor faults play no part.** The chaos scenario adds failed and late texts, duplicate and late webhooks, a Stripe outage, an AI outage and ROC CAPTCHAs (`sources/01_spec/hhq_twin/README.md:36`); every matching warning in it traced to intake classification, and its rates (5.2 and 2.4) are close to `normal_fixed`'s (5.4 and 2.8). The extra causes C2c to C2e show up there only because its random path produced those wordings.

## 5. The v9.1 scenarios (`vendor_chaos` and the other names without `_fixed`)

`vendor_chaos` reports 233.0 outside-license and 115.4 left-out per run (`sources/01_spec/hhq_twin/results/console.txt:91`–`sources/01_spec/hhq_twin/results/console.txt:92`). Those scenarios run the design as it was before v9.3: no job-type matching and a cap on cards per request (`sources/01_spec/hhq_twin/app.py:806`–`sources/01_spec/hhq_twin/app.py:807`), both superseded (CARD-001, CARD-003; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:13`). They also use the old brand, old fees and old texts (`docs/sources.md:34`).

**Disposition: reject as a gate input (D-107).** Their failures are the expected result of a superseded design. They are kept as **negative controls**: the gate runs `normal` seed 1 and fails if it does **not** report `job_card_outside_license` and `qualified_business_left_out`, which proves the checks still fire.

## 6. The rule: any non-waived invariant failure fails the Twin check (D-107)

### 6.1 Definition

- An **invariant failure** is any instance reported by any of the twin's checks (the 56 in `sources/01_spec/hhq_twin/README.md:13`, listed at `sources/01_spec/hhq_twin/README.md:55`–`sources/01_spec/hhq_twin/README.md:70`, plus any the stage 1 twin adds) in any gate scenario and seed.
- The **gate scenarios** are every scenario whose name ends in `_fixed` (11 today), seeds 1–5, with `PYTHONHASHSEED=0`. The stage 1 twin replaces them with scenarios for the changes file and keeps the same rule.
- An instance is **waived** only by an entry in `twin/waivers.json` that names a decision in `docs/decisions.md`. Nothing else waives: no environment variable, no command-line flag, no "known flaky" list.
- The `Twin` check fails (non-zero exit) when any instance is not waived, when a negative control does not fail, when a required check is missing from the report, or when the waiver file is wrong (6.4). It passes only with zero unwaived instances.
- **Status today:** this rule is not yet in force. `.github/workflows/twin.yml` is still the informational smoke run of `normal_fixed` seed 1, and the gate lives in protected paths, so K8 builds it (D-107). The rule covers only the stand-alone twin, not `app/`, until stage 1 links the two.

### 6.2 Two kinds of waiver

| Kind | Matches | Allowed for | Lifetime |
|---|---|---|---|
| **pinned** | One exact instance: scenario, seed, check key and the instance's reference (request, business, card or pick ID) | A failure explained in this file whose fix belongs to a later task (C1, C2b–C2e, and the not-yet-triaged items in section 7) | Until the stage 1 twin task, whose done criterion is `gate.py --no-pinned` passing (no pinned entry left) |
| **class** | Any instance of the named checks whose evidence predicate holds | C2a and C3 only (`homeowner_answered_wrong`) | Permanent, with an integer ceiling per scenario and seed: more instances in one seed than the ceiling fails the gate even if each has evidence. K8 sets the ceilings from its measured run; the stage 1 twin task re-measures them after its fixes |

A pinned waiver that no longer matches any instance is **stale** and fails the gate: if a fix (or any change) moves the random path, the stale entries must be deleted and anything new triaged, so nothing hides behind an old list.

### 6.3 Harness change for K8 (protected paths: K8 is a `harness` task, `BUILD_BRIEF.md:70`)

1. **`twin/`:** copy `sources/01_spec/hhq_twin/` verbatim to `twin/` (the layout's twin folder, `BUILD_BRIEF.md:92`), so the gate and stage 1 edit the copy and `sources/` stays fingerprinted.
2. **Structured instances.** In `twin/twin.py`, give `check()`'s `bad()` a third argument with the instance's IDs (request, business, card, pick, call session) and return it with each instance; the printed report stays as it is.
3. **Actor evidence.** In `twin/twin.py`, where the simulated homeowner answers the trade question and "Which fits best?" (`sources/01_spec/hhq_twin/twin.py:250`, `sources/01_spec/hhq_twin/twin.py:258`), record on its job (`_sim_truth`) whether the option it sent differs from the option matching its own job, and which option it sent.
4. **Independent license oracle.** The gate checks `job_card_outside_license` and `qualified_business_left_out` against `twin/oracle/license_scope.json`, never a table imported from `twin/app.py` or `app/`. For the old twin it is written by hand from v9.3's license table (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:133`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:142`), not copied from `CLASS_ALLOWS`; in stage 1 it is generated from `docs/catalog/categories.yaml`. The simulated businesses' ticked job types also come from this file. A gate self-test (6.5) checks it against every row of `docs/catalog/matching_examples.yaml` (D-110) that its job types can express.
5. **`twin/gate.py`** (standard library only):
   - refuses to run (exit 2) unless `PYTHONHASHSEED` is `0`, which the workflow sets;
   - runs every gate scenario and seed in-process through `build`, `run` and `check`;
   - holds a frozen list of the required check keys (the 56 today, plus any stage 1 adds through a harness change) and exits 2 if the twin no longer declares one of them, so removing or renaming a check can't make the gate pass;
   - applies `twin/waivers.json` (6.4) to every instance;
   - runs the negative control (section 5) and the self-tests (6.5);
   - prints a table per scenario (instances, waived by which entry, unwaived) and every unwaived instance with its IDs, then writes `twin-report.json` (uploaded as a workflow artifact);
   - exit 0: no unwaived instance, controls fired, waiver file valid; exit 1: any unwaived instance, a class ceiling exceeded, or a negative control that did not fire; exit 2: `PYTHONHASHSEED` not `0`, a required check missing, or a waiver-file error (unknown check key, unknown evidence predicate, decision ID missing from `docs/decisions.md`, stale pinned entry, malformed entry);
   - `--no-pinned` ignores pinned entries (used by the stage 1 done check); `--seeds N` widens the seed range.
6. **`twin/waivers.json`:** written by K8 from one gate run with the twin as-is: one pinned entry per current instance, each with its cause code from sections 3, 4 and 7 and decision ID; the class entry for `homeowner_answered_wrong` (D-106) with ceilings set from the measured counts. An Opus reviewer checks that every pinned entry has a cause listed in this file.
7. **`.github/workflows/twin.yml`:** replace the smoke run (`python3 twin.py normal_fixed 1` on a copy) with `PYTHONHASHSEED=0 python3 twin/gate.py`; drop the "INFORMATIONAL" comment; keep the job name `Twin` and `permissions: contents: read`. `nightly-main.yml` already calls it. Once the pinned list is empty, the nightly run adds `--seeds 20`.
8. **Path protection:** add the whole of `twin/` to the runner's protected paths: the checks (`check()` and `bad()` in `twin/twin.py`), the simulated people's wrong-answer evidence, `twin/gate.py`, `twin/waivers.json` and `twin/oracle/license_scope.json`. Only a `harness` task with an Opus review may change any of it (rule 7, `BUILD_BRIEF.md:70`; rule 6, `BUILD_BRIEF.md:69`), so K6 types the stage 1 twin task as `harness`.
9. **PII scanner:** the twin's generated phone numbers (for example `+1928700` followed by four digits) look real. The scanner allowlists the twin's generator patterns explicitly, never the whole `twin/` path, and new fixtures use the fictional `555-01xx` range (D-109).
10. **Running and summarizing:** the gate is run in sessions by the Haiku twin-runner subagent, which returns only the summary table and unwaived instances (`sources/02_build_plan/Build_Plan.md:189`).

### 6.4 `twin/waivers.json` format

```
{
  "version": 1,
  "class": [
    {"id": "WV-HO-ANSWER", "checks": ["qualified_business_left_out", "job_card_outside_license"],
     "evidence": "homeowner_answered_wrong", "decision": "D-106",
     "max_per_seed": {"normal_fixed": 0, "...": 0}}
  ],
  "pinned": [
    {"id": "WV-0001", "scenario": "normal_fixed", "seed": 1, "check": "qualified_business_left_out",
     "ref": {"request": "1044", "business": "Lake Handyman Co 14"}, "cause": "C1", "decision": "D-102",
     "until": "stage 1 twin task"}
  ]
}
```

`homeowner_answered_wrong` holds for an instance when its request's simulated homeowner recorded a wrong answer for that conversation and the request's stored trade or job type is the option it sent. The `max_per_seed` values are integers per scenario and seed; the zeros above are placeholders that K8 fills from its measured run and the stage 1 twin task re-measures.

### 6.5 Tests of the gate itself (`tests/twin/`, owned by K8)

- An instance with no waiver → exit 1.
- A pinned entry whose instance no longer occurs → exit 2.
- A waiver naming a decision ID not in `docs/decisions.md` → exit 2.
- A class waiver whose evidence is absent for an instance → that instance is unwaived → exit 1.
- More class-waived instances in one seed than the ceiling → exit 1.
- `PYTHONHASHSEED` unset or not `0` → exit 2.
- A required check key removed from the twin → exit 2.
- `twin/oracle/license_scope.json` disagrees with a row of `docs/catalog/matching_examples.yaml` it can express → the self-test fails.
- The negative control replaced by a scenario that reports nothing → exit 1.
- **Deliberate breaks of the twin's rules** (each in a temporary copy) must make the gate exit 1: the AC-only sub-license given `heating_repair` (section 2); the license check at card time removed; job-type matching switched off (`job_matching: false`); the opt-out check removed from sending; the quiet-hours check removed; the per-attempt Stripe idempotency key reused across picks so a business is charged twice (`double_charge`); contact details released before payment succeeds (`details_released_wrongly`). The first one is exactly the break the current twin cannot see.

## 7. Other failures in the v9.3 scenarios (not investigated in K5)

The rule in section 6 also catches these, from the committed 5-seed results (reproduced in this session). K5 did not trace their causes. Each is pinned by instance at K8 with cause code **R-TRIAGE** and must be resolved, or documented with a decision, by the stage 1 twin task. The first two are money and privacy invariants and go first.

| Check | Scenarios (per run) | Rule it protects |
|---|---|---|
| `details_released_to_unlicensed` | `crowded_fixed` 0.2 | PRIV-005, PICK-020: details reach only a licensed business on a paid pick |
| `charged_but_not_introduced` | `texting_suspended_fixed` 0.4 | FEE-006, FEE-021: every kept charge bought an introduction |
| `call_blocked_unlinked_staff_phone` | 0.2–0.4 in 9 of the 11 `_fixed` scenarios | CALL-011 (changed by D-078) |
| `texter_never_answered` | `texting_suspended_fixed` 2.2; `busy_fixed`, `adversarial_fixed`, `crowded_fixed` 0.2 | INT-023, TXT-023 |
| `promise_missed` | `texting_suspended_fixed` 2.0 | The twin's 9 p.m. promise; the stage 1 twin re-states it for D-011 and D-030 |
| `help_not_answered` | `texting_suspended_fixed` 0.6 | CON-018 |
| `duplicate_open_requests` | `races_fixed` 0.4, `crowded_fixed` 0.2 | INT-009, INT-010 |
| `pick_not_intended_by_homeowner` | `races_fixed` 0.2 | PICK-013, PICK-015 |

## 8. What the stage 1 twin task inherits

- Fix C1 (D-102), C2b and C2c (D-103), C2d (D-104) and C2e (D-105) in the twin's rules, and the same rules in the app's intake through its tests (`docs/test-design.md`, TD-INT-03 to TD-INT-06).
- Triage every R-TRIAGE item in section 7 to resolve or document.
- Run as a `harness` task with an Opus review, since all of `twin/` is protected.
- Re-measure the D-106 class-waiver ceilings after the fixes.
- Pass `gate.py --no-pinned`: no pinned waiver left, only the D-106 class waiver.
- Keep the negative control and the gate self-tests green.
