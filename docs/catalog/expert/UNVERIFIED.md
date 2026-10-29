# Facts that could not be confirmed in an official source

Each line is something an expert file says, or leans on, that the writer could not read in a primary source (statute text, the agency's own page, the city code, the utility's tariff). Each is marked `unverified: true` or `[judgment]` in the file that uses it, so no customer-facing text depends on it. Before launch, each needs one of: a primary source read and cited, a change to the file, or an attorney's answer. Items marked **launch-blocking** change routing, fees or refusals if they turn out differently.

## Licensing and law

1. **launch-blocking** Handyman exemption amount. HB 2120 (2025 session) may raise the A.R.S. 32-1121(A)(14) limit from $1,000 to $3,500 (labor and materials). The azleg.gov statute page still showed $1,000 when read. Affects LIC-019/LIC-020 and the handyman refusal logic. Files: `handyman.yaml`, `painting.yaml`, `gutters.yaml`. Who checks: Casey or counsel, by reading the current statute text at azleg.gov and the ROC's "unlicensed work" page.
2. **launch-blocking** Alarm businesses. A.R.S. 32-1121(A)(17) exempts alarm agents from the contractor statute; the Board of Technical Registration certifies alarm businesses and agents. `license_map.yaml` requires an ROC R-67/CR-67 plus the BTR certificate, so a BTR-only alarm business never receives a card. The expert file follows the license map. Needs a reading of 32-1121(A)(17), the BTR alarm rules and LHC Code ch. 8.12 (not read). File: `alarm_systems.yaml`. See DECISIONS-NEEDED.md item 8.
3. **launch-blocking** Termite reports for a sale or loan (WDIIR). A.A.C. R3-8-307 lets only a certified wood-destroying-organism applicator at a licensed pest business complete the report. Today `home_inspection` carries the fee-free termite report and routes to BTR home inspectors only, so the card may reach no one who can sign it. The rule itself was read; what is unverified is whether any BTR inspector in Havasu also holds the pest license. File: `pest_control.yaml`, `home_inspection.yaml`. See DECISIONS-NEEDED.md item 7.
4. Solar screens. ROC R-65 (Glazing) scope names glass, film, blinds and shutters but not exterior solar screens. Whether a screen-only shop needs any ROC license, or falls under the handyman exemption by job size, is open. File: `solar_screens.yaml`. Attorney item.
5. Misting systems. ROC R-21 (Landscaping and Irrigation) item 11 covers residential misting up to 1,000 PSI; the category routes R-54 (water conditioning) only. The scope text was read; what is unverified is whether R-21 holders in Havasu do misting work. File: `water_treatment.yaml`. See PROPOSED-RULE-CHANGES.md.
6. Movers. The Attorney General's moving guide says Arizona does not register intrastate movers; a DPS page points to the Corporation Commission. The two agency pages conflict. File: `local_moving.yaml`.
7. Security cameras. Whether a camera-only (no alarm) installer needs a BTR certificate, and whether a low-voltage permit is needed in LHC, were not confirmed. File: `security_cameras.yaml`.
8. Boat work. The reading that boat repair, dock and lift work need no ROC license (docks may be R-9 or marine contractors) is a judgment. File: `boat_repair.yaml`. Attorney item.
9. Appliance repair. Whether repair-only appliance businesses need any ROC class (trade KB open question 7) stays open. File: `appliance_repair.yaml`.
10. Home watch. Whether a home-watch business doing "security checks" touches A.R.S. 32-2601 (security guard licensing) is a counsel question. File: `home_watch.yaml`.
11. Vermiculite insulation. Whether removal needs an asbestos-abatement contractor rather than an R-40 insulation contractor was not settled. File: `insulation.yaml`.
12. Manufactured homes. Roof-overs, gutters and foundation work on manufactured homes may need an ADOH-licensed installer rather than (or as well as) the ROC class; VA manufactured-home foundation certificates need an engineer. Files: `roof_repair.yaml`, `gutters.yaml`, `home_inspection.yaml`.
13. Pressure washing. Whether soft-washing with algaecide needs an Arizona Department of Agriculture pesticide license, and whether a roof wash is R-42 work when no coating is applied, are judgments. File: `pressure_washing.yaml`.
14. Pest control license categories were read from a third-party summary, not the Department of Agriculture's page (trade KB open question 19). Whether exclusion-only bird work needs a license is open (question 13). File: `pest_control.yaml`.
15. Septic. The 750-gallon threshold the trade KB cites was not found in the R18-13-1103 text as read. Septic-area boundaries (which parts of town are on septic) were not sourced. File: `septic_pumping.yaml`.
16. Septic transfer inspections (R18-9-A316) are a RESPA settlement service (12 CFR 1024.2). Both texts were read; what is unverified is whether this makes the introduction fee a RESPA problem the way the home-inspection fee was judged to be. Counsel item. See DECISIONS-NEEDED.md item 9.
17. Electrical. LHC Ordinance 25-1370 (2023 NEC adoption with amendments, outside service disconnect) and its effect on an inside panel change-out is a judgment. Permit rules for work outside the city limits were not checked. File: `electrical.yaml`.
18. Painting. The share of Havasu homes built before 1978 (EPA lead-safe RRP rule) was not sourced. File: `painting.yaml`.
19. Insulation. Climate zone is 3B, not the 2B some sources give; the IECC edition LHC enforces and whether added insulation needs a permit were not confirmed. No UES attic insulation rebate was found (duct sealing only; weatherization through WACOG). File: `insulation.yaml`.
20. HVAC. The 2024 IMC condensate-line rule and permit practice outside LHC were not read. Files: `hvac_repair.yaml`, `hvac_replacement.yaml`.
21. Dryer vents. Whether a reroute needs a permit, and which ROC class does reroutes, are judgments; the 2024 IMC was not read. File: `dryer_vent_cleaning.yaml`.
22. Pools. The city permit list is silent on equipment swaps; whether a heater swap needs a permit is a judgment. File: `pool_repair.yaml`.
23. Trash. LHC Code ch. 8.04 (trash franchise) was not read against the changes file's 7.1 item 13; whether bagged pet waste and bin-wash water rules exist in the city code is unchecked. Files: `estate_cleanout.yaml`, `dog_waste_pickup.yaml`, `trash_can_cleaning.yaml`.
24. City business licence and TPT for mobile and out-of-city businesses (detailers, holiday-light crews, bin cleaners, pressure washers) was not checked against the city's licensing page. Files: `car_detailing.yaml`, `holiday_lights.yaml`, `trash_can_cleaning.yaml`, `pressure_washing.yaml`.
25. Stormwater. Whether mobile detailers and pressure washers must capture wash water under the city's stormwater rules was not confirmed. Files: `car_detailing.yaml`, `pressure_washing.yaml`.

## Local facts

26. Water hardness. The 19 grains-per-gallon figure is cited to a Consumer Confidence Report; the city water page itself does not state it. Files: `house_cleaning.yaml`, `water_treatment.yaml`, `window_cleaning.yaml`.
27. Trash zone and pickup-day map comes from a news article of about 2021; may be stale. File: `trash_can_cleaning.yaml`.
28. Evaporation and water-loss numbers for pools come from Scottsdale, not Havasu. File: `pool_repair.yaml`.
29. Short-term rental counts (597 listings) come from a market-data vendor's April 2026 page. File: `turnover_cleaning.yaml`.
30. Vacancy and seasonal-resident figures were not pulled from the Census. File: `home_watch.yaml`.
31. The National Home Watch Association claims come from its home page only. File: `home_watch.yaml`.
32. Seasonality in several files (house cleaning, pest pre-emergent timing, pool opening and closing) is marked `[judgment]`.

## Business-side figures

33. Customer-acquisition costs come from vendor pages and forum posts (Angi, Thumbtack, Google Local Services Ads, search ads): electrician ~$39 to ~$94 per lead; HVAC replacement ~$190 to ~$542 per booked customer; house cleaning ~$60 to $70 per lead; home inspection ~$40 per lead; pool repair $35 to $55 (two inconsistent figures). None of these is audited; each is marked `unverified: true` in `business_side.customer_acquisition_benchmarks` and shows up in `business_signup_knowledge.yaml` with the same flag. No figures were found for holiday lights, pest control, window cleaning (forum and secondhand only), dog waste or bin cleaning.
34. Code layering limits (how many roof layers a re-roof may go over) are behind paywalled code text. File: `roof_repair.yaml`.
