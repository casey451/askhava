# Model tiering of the queued tasks, November 10, 2026

**Source:** Casey, in chat about 17:00Z on November 10, 2026: "Can we go to a two-lane approach and have each task briefly reviewed to see what the lowest level agent can successfully complete it is, and have that agent do it before it's checked?" Decision D-2463 holds the rubric; D-2462 the second lane.

**Scope:** every task QUEUED on `claude/kit-state` when this was written (148 tasks), none claimed; tasks in flight, DONE tasks and T246 keep their recorded model. Every reviewer stays Opus. Result: **15 Sonnet, 133 Opus** (after the PR #424 review); the model window from Thursday November 12, 17:00Z still runs every worker and reviewer on its own model while it is open (D-2225).

**Rubric (D-2463):** Opus for review areas money, privacy, security, recovery, licensing or harness; harness tasks; anything owning `runner/`; a migration that moves data; concurrency, transactions or locks; vendor adapters or webhooks (Stripe, Twilio, email); work across more than one crate; a budget of about 500 lines or 35% of context or more; high effort; a task that already failed an attempt. Sonnet for docs, copy, templates, catalog data, tests-only tasks, small single-crate changes and page wiring with no money or privacy logic. No Haiku for code.

| Task | Was | Now | Reason |
|---|---|---|---|
| T099 | Opus | Opus | areas money; size 600 lines/7% |
| T100 | Opus | Opus | areas money,privacy,recovery,security; size 400 lines/41% |
| T101 | Opus | Opus | size 600 lines/9% |
| T102 | Opus | Sonnet | single-crate small change (site pages, 300 lines); the concurrency word match was a false positive |
| T103 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; multi-crate (intake,site); size 500 lines/16% |
| T104 | Opus | Sonnet | docs/templates/tests/data only |
| T105 | Opus | Opus | areas money,privacy,security; multi-crate (businesses,jobs,messaging,ops,payments,picks); high effort |
| T106 | Opus | Opus | areas security; multi-crate (portals,server) |
| T107 | Opus | Opus | areas privacy |
| T108 | Opus | Opus | areas money,security; vendor adapter or webhook |
| T109 | Opus | Opus | areas privacy,recovery,security; concurrency/transactions/locks; multi-crate (portals,recovery); high effort |
| T110 | Opus | Opus | areas money,recovery; high effort |
| T111 | Opus | Opus | areas money; size 650 lines/12% |
| T112 | Opus | Opus | areas privacy,security |
| T113 | Opus | Sonnet | docs/templates/tests/data only |
| T114 | Opus | Opus | areas money |
| T115 | Opus | Sonnet | single-crate small change (site, 400 lines) |
| T116 | Opus | Sonnet | docs/templates/tests/data only |
| T117 | Opus | Opus | areas recovery,security |
| T118 | Opus | Opus | areas money,security; size 550 lines/6% |
| T119 | Opus | Opus | areas money,recovery; size 550 lines/6% |
| T120 | Opus | Opus | areas privacy,security; high effort |
| T121 | Opus | Opus | areas security |
| T122 | Opus | Opus | areas security |
| T123 | Opus | Sonnet | docs/templates/tests/data only |
| T125 | Opus | Opus | areas money,privacy,security; high effort |
| T126 | Opus | Opus | areas money,privacy,recovery,security; size 400 lines/56% |
| T129 | Opus | Opus | areas harness,security; harness; vendor adapter or webhook |
| T131 | Opus | Opus | areas harness,security; harness; vendor adapter or webhook |
| T132 | Opus | Opus | areas harness,money,security; harness; vendor adapter or webhook; size 500 lines/15%; high effort |
| T133 | Opus | Opus | areas harness,security; harness; vendor adapter or webhook; multi-crate (adapters,businesses) |
| T134 | Opus | Opus | areas harness,recovery,security; harness; concurrency/transactions/locks; vendor adapter or webhook |
| T135 | Opus | Opus | areas security; vendor adapter or webhook |
| T136 | Haiku | Sonnet | moves up from Haiku: a docs task that drives the four journeys (J1 to J4) on the test site and records what it finds, which needs more judgement than Haiku gives; Opus reviewer |
| T137 | Opus | Opus | areas privacy,security |
| T138 | Opus | Opus | areas harness,security; harness |
| T139 | Opus | Opus | areas recovery; high effort |
| T140 | Opus | Opus | areas money,privacy,security; high effort |
| T141 | Opus | Opus | size 550 lines/3% |
| T142 | Opus | Opus | areas money,privacy,recovery,security; size 400 lines/41% |
| T151 | Opus | Opus | areas privacy,security; vendor adapter or webhook; multi-crate (intake,messaging,ops,portals); high effort |
| T155 | Opus | Opus | areas licensing,security; multi-crate (businesses,portals,quotes); high effort |
| T157 | Opus | Opus | areas money,privacy,security; vendor adapter or webhook; multi-crate (picks,portals); high effort |
| T158 | Opus | Opus | areas money,security; multi-crate (portals,quotes) |
| T159 | Opus | Sonnet | docs/templates/tests/data only |
| T161 | Opus | Opus | size 500 lines/13% |
| T162 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; high effort |
| T164 | Opus | Opus | areas money,privacy,security; multi-crate (portals,quotes); size 550 lines/22%; high effort |
| T165 | Opus | Opus | areas money,privacy,security; multi-crate (businesses,portals,quotes); high effort |
| T166 | Opus | Sonnet | single-crate small change (site, 200 lines) |
| T171 | Opus | Sonnet | docs/templates/tests/data only |
| T200 | Opus | Opus | areas privacy,security; vendor adapter or webhook; multi-crate (portals,site); high effort |
| T201 | Opus | Sonnet | single-crate small change (site, 300 lines) |
| T202 | Opus | Opus | areas security; concurrency/transactions/locks; multi-crate (portals,site) |
| T203 | Opus | Sonnet | docs/templates/tests/data only |
| T204 | Opus | Opus | areas money,security; vendor adapter or webhook; multi-crate (messaging,ops,portals,server); high effort |
| T205 | Opus | Opus | areas money,security; concurrency/transactions/locks; vendor adapter or webhook |
| T206 | Opus | Opus | areas money,recovery; high effort |
| T207 | Opus | Opus | areas licensing,security; high effort |
| T209 | Opus | Opus | areas money,privacy,security; vendor adapter or webhook; multi-crate (payments,picks,portals,quotes); high effort |
| T210 | Opus | Opus | areas money,security; multi-crate (payments,portals) |
| T212 | Opus | Opus | areas privacy,security; vendor adapter or webhook; multi-crate (intake,portals,privacy,quotes); size 500 lines/24%; high effort |
| T214 | Opus | Opus | areas money,privacy,security; multi-crate (picks,portals); high effort |
| T215 | Opus | Opus | areas money,security; multi-crate (businesses,intake,messaging); high effort |
| T216 | Opus | Sonnet | single-crate small change (site, 100 lines) |
| T218 | Opus | Sonnet | docs/templates/tests/data only |
| T221 | Opus | Sonnet | single-crate small change (site, 350 lines) |
| T222 | Opus | Opus | areas privacy; size 500 lines/7% |
| T228 | Opus | Opus | areas money,privacy,security; multi-crate (businesses,picks,portals,quotes) |
| T229 | Opus | Opus | size 600 lines/13% |
| T232 | Opus | Opus | areas money,privacy,security; vendor adapter or webhook; multi-crate (messaging,portals); high effort |
| T234 | Opus | Opus | areas harness,privacy,security; harness; size 800 lines/13%; high effort |
| T235 | Opus | Opus | areas harness,money; harness; size 600 lines/6%; high effort |
| T236 | Opus | Opus | areas security; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (ai,intake); size 600 lines/21%; high effort |
| T237 | Opus | Opus | areas harness,money,privacy,recovery,security; harness; concurrency/transactions/locks; vendor adapter or webhook; size 800 lines/12%; high effort |
| T238 | Opus | Opus | areas harness,money,privacy,recovery,security; harness; concurrency/transactions/locks; vendor adapter or webhook; size 800 lines/12%; high effort |
| T239 | Opus | Opus | areas harness,money,privacy,recovery,security; harness; concurrency/transactions/locks; vendor adapter or webhook; size 800 lines/12%; high effort |
| T242 | Opus | Opus | size 800 lines/17%; high effort |
| T248 | Opus | Opus | areas privacy,security; migration with data movement; concurrency/transactions/locks; multi-crate (businesses,catalog,portals); size 600 lines/34%; high effort |
| T249 | Opus | Opus | areas licensing,security; multi-crate (intake,portals,quotes); size 600 lines/39%; high effort |
| T251 | Opus | Opus | areas licensing,privacy; concurrency/transactions/locks; multi-crate (intake,quotes); size 600 lines/36%; high effort |
| T255 | Opus | Opus | areas money,privacy; vendor adapter or webhook; multi-crate (intake,payments,server); size 500 lines/34%; high effort |
| T261 | Opus | Opus | areas money,security; vendor adapter or webhook; multi-crate (portals,server); size 600 lines/36%; high effort |
| T266 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; multi-crate (picks,portals,privacy,server); size 400 lines/40%; high effort |
| T268 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; multi-crate (businesses,messaging,portals); high effort |
| T269 | Opus | Opus | areas security |
| T271 | Opus | Opus | areas money,security; vendor adapter or webhook; high effort |
| T272 | Opus | Opus | areas security; migration with data movement; concurrency/transactions/locks; vendor adapter or webhook; size 600 lines/37%; high effort |
| T273 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; high effort |
| T277 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (intake,privacy); size 550 lines/13%; high effort |
| T278 | Opus | Opus | areas privacy; concurrency/transactions/locks; multi-crate (intake,picks,quotes); high effort |
| T280 | Opus | Opus | areas privacy,security; vendor adapter or webhook; multi-crate (messaging,server); high effort |
| T281 | Opus | Opus | areas money,privacy; concurrency/transactions/locks; vendor adapter or webhook; size 500 lines/18%; high effort |
| T294 | Opus | Opus | areas harness,security; harness; multi-crate (portals,site); size 500 lines/27%; high effort |
| T295 | Opus | Opus | areas harness,money,privacy; harness; vendor adapter or webhook; multi-crate (ai,intake,payments,picks); size 600 lines/31%; high effort |
| T296 | Opus | Opus | areas security; vendor adapter or webhook |
| T297 | Opus | Opus | areas harness,privacy,security; harness; size 600 lines/10%; high effort |
| T298 | Opus | Opus | areas privacy,security; vendor adapter or webhook |
| T299 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (intake,server,site); size 600 lines/20%; high effort |
| T300 | Opus | Opus | areas privacy,security; vendor adapter or webhook; size 600 lines/14%; high effort |
| T301 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; size 600 lines/14%; high effort |
| T302 | Opus | Opus | areas privacy,security; vendor adapter or webhook; size 600 lines/14%; high effort |
| T303 | Opus | Opus | areas privacy,security; size 600 lines/13%; high effort |
| T304 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; vendor adapter or webhook; size 600 lines/13%; high effort |
| T305 | Opus | Opus | areas privacy,security; size 600 lines/14%; high effort |
| T306 | Opus | Opus | areas privacy,security; size 600 lines/14%; high effort |
| T307 | Opus | Opus | areas privacy,security; vendor adapter or webhook; size 600 lines/14%; high effort |
| T308 | Opus | Opus | areas privacy,security; high effort |
| T309 | Opus | Opus | areas privacy,security; vendor adapter or webhook; size 600 lines/13%; high effort |
| T310 | Opus | Opus | areas privacy,security; vendor adapter or webhook; size 600 lines/13%; high effort |
| T311 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (calls,server); high effort |
| T312 | Opus | Opus | the emergency word check (INT-002) is a safety rule; review areas security and compliance (PR #424 review) |
| T313 | Opus | Opus | areas privacy; high effort |
| T314 | Opus | Opus | areas money,privacy; concurrency/transactions/locks; multi-crate (payments,picks); high effort |
| T315 | Opus | Opus | areas privacy,security; migration with data movement; concurrency/transactions/locks; multi-crate (jobs,portals); size 500 lines/32%; high effort |
| T316 | Opus | Opus | reads licensing data and screens a public name across crates; review areas privacy and licensing (PR #424 review) |
| T317 | Opus | Opus | areas harness,privacy,security; harness; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (intake,server); size 600 lines/24%; high effort |
| T318 | Opus | Opus | areas privacy; concurrency/transactions/locks; multi-crate (calls,messaging,picks,quotes); size 550 lines/19%; high effort |
| T319 | Opus | Opus | areas security; vendor adapter or webhook; multi-crate (core,portals,server); size 550 lines/22%; high effort |
| T320 | Opus | Opus | concurrency/transactions/locks; vendor adapter or webhook |
| T321 | Opus | Opus | areas privacy; concurrency/transactions/locks; multi-crate (intake,messaging,privacy); size 500 lines/31%; high effort |
| T322 | Opus | Opus | areas security; concurrency/transactions/locks |
| T323 | Opus | Opus | the AI adds-no-fact guard (AI-019) is a security control; review area security (PR #424 review) |
| T324 | Opus | Opus | areas harness,money,privacy; harness |
| T325 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; vendor adapter or webhook |
| T326 | Opus | Opus | areas harness,security; harness; concurrency/transactions/locks; size 300 lines/38% |
| T327 | Opus | Opus | areas harness,security; harness |
| T328 | Opus | Opus | areas money,privacy,security; vendor adapter or webhook; multi-crate (picks,server); size 600 lines/32%; high effort |
| T329 | Opus | Opus | areas privacy; size 500 lines/31%; high effort |
| T330 | Opus | Opus | areas privacy,security; vendor adapter or webhook; multi-crate (intake,picks); size 500 lines/31%; high effort |
| T331 | Opus | Opus | areas harness,money,security; harness; concurrency/transactions/locks; vendor adapter or webhook; high effort |
| T332 | Opus | Opus | areas harness,money,security; harness; concurrency/transactions/locks; vendor adapter or webhook; size 500 lines/25%; high effort |
| T333 | Opus | Opus | areas harness,money; harness; vendor adapter or webhook; high effort |
| T334 | Opus | Opus | areas money; concurrency/transactions/locks; vendor adapter or webhook; size 500 lines/18%; high effort |
| T335 | Opus | Opus | areas harness,money,security; harness; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (adapters,payments); size 550 lines/18%; high effort |
| T336 | Opus | Opus | areas harness,privacy,security; harness; concurrency/transactions/locks; vendor adapter or webhook; multi-crate (adapters,server); size 600 lines/19%; high effort |
| T337 | Opus | Opus | areas money,security; high effort |
| T338 | Opus | Opus | areas harness,security; harness; vendor adapter or webhook |
| T339 | Opus | Opus | areas money; concurrency/transactions/locks |
| T340 | Opus | Opus | areas money,privacy; multi-crate (payments,picks); size 600 lines/35%; high effort |
| T341 | Opus | Opus | areas harness,money,privacy,security; harness; concurrency/transactions/locks; multi-crate (picks,portals); size 700 lines/27%; high effort |
| T342 | Opus | Opus | areas privacy |
| T343 | Opus | Opus | areas harness,money; harness |
| T344 | Opus | Opus | areas privacy,security; concurrency/transactions/locks; vendor adapter or webhook |
| T345 | Opus | Opus | areas harness,security; harness |
| T346 | Opus | Opus | areas privacy,security; migration with data movement; concurrency/transactions/locks; vendor adapter or webhook; size 550 lines/36%; high effort |
| T347 | Opus | Opus | areas harness,licensing,money; harness; concurrency/transactions/locks; multi-crate (licensing,quotes); size 250 lines/35% |
| T348 | Opus | Opus | areas harness,privacy,security; harness; vendor adapter or webhook; size 800 lines/36%; high effort |
