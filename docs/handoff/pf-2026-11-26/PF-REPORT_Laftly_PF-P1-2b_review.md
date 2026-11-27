# Laftly pre-flight review — PF-P1-2b

**13 tasks checked; 1 blocker, 2 majors, 5 minors.**

**Recommendation: Hold T734 until its authorization-matrix conflict is resolved. Correct the T723 and T811 plans before assigning them to workers.**

This is a static review of the supplied repository snapshot, task prompts, ownership declarations and test-design excerpts. Findings distinguish confirmed conflicts from checks that require the missing repository files or completed prerequisite tasks.

## 1. Findings

### Blockers

| # | Task | Severity | What breaks | Evidence | Smallest fix to the plan |
|---|---|---|---|---|---|
| 1 | T734 | BLOCKER | The planned pause-reason redirect contradicts the existing authorization-matrix test. The worker cannot satisfy both without changing an unowned test or violating the goal. | `kit/prompts/T734.md:9` requires a 303 when the pause question is not due. `app/server/tests/door_matrix.rs:551–555` requires `status == 200` for an admitted business GET. Its fixtures create active businesses at lines 1001–1014 and 1028–1039. | Explicitly authorize a narrow, route-specific change to `door_matrix.rs` under a logged decision, preserving authentication, no-store and leakage checks. Add the file to ownership and verify its scheduling dependencies. |

### Major findings

| # | Task | Severity | What breaks | Evidence | Smallest fix to the plan |
|---|---|---|---|---|---|
| 2 | T723 | MAJOR | Multiple catalog-only job types can be reduced to one when reconstructing stored candidates. Coverage counting and subsequent card delivery could disagree. | `app/intake/src/order_dispatch.rs:1106–1114`: `plain = Some(j.as_str())` retains only one unmatched label. Lines 1138–1141 reconstruct the set through `rebuilt`. `docs/catalog/categories.yaml:165` lists four pool-repair job types. | Explicitly require `newest_set`, `rebuilt` and `final_pairs` to preserve every expanded catalog candidate. Test that both the coverage count and card recipients use the complete set. All changes fit existing ownership. |
| 3 | T811 | MAJOR | The existing fake Verify fixture cannot prove the email destination or delivery channel. A worker could implement the wrong channel and still satisfy a send-count assertion. | `app/server/tests/action_links_route.rs:1223–1230`: `send_code` ignores the destination and `CodeChannel`, recording only a count. Lines 1247–1251 construct `CodeTo::Email(fake)` using a fake telephone number. | Require the owned test fixture to record the destination and channel, use a fake email address, and assert `CodeChannel::Email`, the correct address and zero SMS/voice sends. Exercise the real `LinkCodeAddress` lookup separately. |

### Minor findings

| # | Task | Severity | What breaks | Evidence | Smallest fix to the plan |
|---|---|---|---|---|---|
| 4 | T688 | MINOR | Router composition references are stale. | `kit/prompts/T688.md:9` cites `RouterParts` at line 123 and `build_router` at lines 559–612. Their current definitions are at `app/server/src/lib.rs:132` and `:661–714`. | Refresh references and retain symbol-based anchors. |
| 5 | T723 | MINOR | The prompt names the wrong helper for rebuilding keyword candidates. | `kit/prompts/T723.md:9` says `order_dispatch::facts` calls `matching::candidates`. `order_dispatch.rs:1267–1273` shows `facts` constructs `CardFacts`; the candidate reconstruction is at `:1093–1115`. | Reference `rebuilt` rather than `facts`. |
| 6 | T746 | MINOR | The prompt identifies a function that does not render the Confirm page. | `app/portals/src/confirm_pay.rs:400–423`: `open_confirm_page` records the first notice opening and returns a boolean. The actual served rendering occurs at `:1280–1298`, through `served_page`. | Anchor the answer-loading and rendering instructions to the served route and `served_page`. Preserve existing public rendering interfaces. |
| 7 | T811 | MINOR | The explicitly authorized test-change line references have moved again. | `kit/prompts/T811.md:9` identifies the `coded(&pool, false)` block at lines 1260–1265. It now resides at `app/server/tests/action_links_route.rs:1402–1407`. | Refresh the lines; retain the existing test-name anchor. |
| 8 | T817 | MINOR | The acceptance sentence omits the permitted opt-out confirmation exception. | `kit/prompts/T817.md:11`: “no carrier call reaches a number listed before the claim.” `docs/spec/consent.md:17`: “except the one opt-out confirmation”. `app/messaging/src/rules.rs:563–566` implements that exception. | Change acceptance to prohibit all **non-exempt** carrier sends after the applicable suppression check. Preserve CON-009 and the documented residual race window. |

## 2. Clean tasks

No definite plan-breaking issue identified in these seven tasks:

| Task | Result |
|---|---|
| T718 | Clean, subject to prerequisite changes |
| T719 | Clean, subject to prerequisite changes |
| T736 | Clean, subject to prerequisite changes |
| T760 | Clean, subject to prerequisite changes |
| T778 | Clean |
| T783 | Clean |
| T824 | Clean, subject to T865 |

## 3. Ownership and scheduling

**PASS — no unordered overlaps between explicitly owned files within this package.**

The dependency declarations correctly serialize the overlapping tasks:

- T723 → T718 → T719 → T736 → T760
- T811 → T746

All 13 prompts also name test commands targeting their intended test files. Actual compilation and execution remain unverified.

## 4. Can't tell

The following require additional evidence before final clearance:

- **Consent chain T856–T865:** Its implementation is absent. Intake behavior and consent-related assumptions must be rechecked after it merges.
- **T760:** Whether the preceding intake tasks already resolve its coverage issue. The existing prompt correctly permits a test-only implementation.
- **Complete dependency graph:** Ownership collisions with tasks outside this package cannot be ruled out.
- **Architecture and test-design documents:** Only excerpts are available.
- **Runtime verification:** The supplied repository is incomplete, preventing compilation, Clippy, PostgreSQL integration tests and Playwright execution.
- **Shared ownership rules:** The review brief lists fewer shared paths than the individual worker prompts. This review uses the worker prompts' explicit permissions for router composition, API exports and messaging templates. Confirm these match the runner's actual enforcement.

**Final disposition: CONDITIONAL PASS.** The queue does not require wholesale replanning. Resolve T734's test-ownership blocker, strengthen T723 and T811, refresh the minor references, and recheck the consent-dependent tasks after their prerequisites land.
