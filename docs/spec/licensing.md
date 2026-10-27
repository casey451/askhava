# Spec: Categories, licensing and routing

Prefix: `LIC` · Modules: `catalog`, `licensing` · Format and checks: [index.md](index.md)

- **LIC-001** Each category stores its job types, license rule (none, ROC class or other state agency), recurring flag, intake questions, fee rule with a set tier for trip-fee quotes, after-hours emergency flag and keyword list. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:35`.
- **LIC-002** Adding or switching off a category is a logged settings change with no code change. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:35`, `BUILD_BRIEF.md:464`.
- **LIC-003** A category that isn't confirmed in the K2b list stays switched off. Source: `sources/02_build_plan/Build_Plan.md:34`.
- **LIC-004** Childcare, home care, massage, mobile hair and nails, mobile vets, catering, towing and property management are not offered at launch. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:34`.
- **LIC-005** Garage doors and junk removal are off at launch, while estate cleanouts and donation pickup stay on. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:240`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:249`, `BUILD_BRIEF.md:460`.
- **LIC-006** A business's license class comes from the state record at sign-up and is never typed in by the business. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:115`.
- **LIC-007** ROC licenses are checked against the ROC's daily downloadable lists plus a throttled single live lookup at each pick (D-017). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:38`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:65`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`.
- **LIC-008** A failed lookup (site down, CAPTCHA or page change) is "unknown": the business stays as it was for up to 24 hours and is then paused, and a misread page never changes a verified status. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:194`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:130`.
- **LIC-009** A license-end or bond-cancellation date on the state record pauses the business on that date. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:129`.
- **LIC-010** ROC lists and the city license list are used for verification only, never for recruiting. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`.
- **LIC-011** A business in a no-license category must match by name on the city's published active business-license spreadsheet. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:37`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:115`.
- **LIC-012** Pest control and any weed job involving pre-emergents or advertised weed control go only to Arizona Department of Agriculture pest-management licensees, with the license number shown. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:203`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:46`.
- **LIC-013** Alarm installs and monitoring go only to Board-certified alarm businesses that also hold an ROC low-voltage class. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:204`.
- **LIC-014** Camera installs go only to R-67 (or broader) licensees, or to handymen under $1,000 with no permit. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:204`.
- **LIC-015** Home inspections go only to Board-certified inspectors. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:204`.
- **LIC-016** Septic pumping goes only to haulers with an ADEQ septage-hauler vehicle license. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:205`.
- **LIC-017** Moves leaving Arizona are refused. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:206`.
- **LIC-018** Home watch and turnover cleaning never include rent collection, finding tenants or leasing. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:207`.
- **LIC-019** A handyman quote for construction-type work whose total (labor plus materials, top of any range) is $1,000 or more is refused with an explanation of Arizona's handyman limit. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:41`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:345`.
- **LIC-020** The $1,000 limit does not apply to non-construction services such as detailing or moving. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:45`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:208`.
- **LIC-021** Gas work, hardwired smoke alarms and fire sprinklers are never sent to handymen at any price. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:42`.
- **LIC-022** Permit-type jobs (water heater replacement, panels, new circuits, re-roofs and similar) are never sent to handymen. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:43`.
- **LIC-023** A split-job check blocks a second handyman job from the same business at the same address with overlapping scope within 30 days (D-027). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:208`.
- **LIC-024** Every handyman quote card shows "Not a licensed contractor. Only for jobs under $1,000 total with no permit." Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:44`.
- **LIC-025** Handymen certify on each quote that it is a full-job price under $1,000 with no permit and no gas, smoke-alarm or sprinkler work, and a business that breaks this is removed. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:241`.
- **LIC-026** A licensed-trade quote card shows "ROC #<number> showed Active on the ROC website when we checked on <date>. Check it yourself." with a link. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:192`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:144`.
- **LIC-027** At sign-up the business hears a read-back of what its license covers and may add extras only if the license covers them or they need no license. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:94`.
- **LIC-028** Dryer-vent cleaning needs no license, while rerouting or replacing a vent is handyman work under the $1,000 cap. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:240`.
- **LIC-029** Plug-in temporary holiday lights need no license, while any new outlet, circuit or hardwiring goes only to R-11 electricians. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:240`.
- **LIC-030** Freestanding appliance repair goes to R-63 licensees or to handymen under $1,000, and never includes gas, plumbing or electrical lines. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:240`.
- **LIC-031** Weekly pool cleaning needs no license; pool repair or equipment work goes to R-6 or a handyman under $1,000, and gas heaters only to licensed businesses. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:240`.
- **LIC-032** Insurance for no-license categories is shown but never required. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:48`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:64`.
- **LIC-033** Licensing code keeps at least 95% branch coverage, and mutation tests turn red when the license filter is broken. Source: `BUILD_BRIEF.md:481`, `BUILD_BRIEF.md:472`.
