# Seventh audit of November 10, 2026: dispositions

**Input:** Casey's seventh audit, a static read of `main` at `05c4ed68` (new findings N7-01 to N7-05 and the design note N7-D1), condensed in the overseer's scratchpad and treated as untrusted input. A verifier confirmed every item on `main` at `63b7c55a` and found that N7-01, N7-02 and N7-04 are gaps D-2210 logged with no owner task; this plan change spot-checked the lines it acts on. Decisions D-2329 to D-2335.

| Item | Verdict | Evidence on `main` 63b7c55a | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| N7-01 (P0 before payments) Fee-free job types charged | New (D-2210's logged gap) | `fee_inputs` (`app/picks/src/service.rs:396`) uses default facts; no table stores a job's category or job type; `home_inspection`'s `fee_free_job_types` (`docs/catalog/categories.yaml:665`); intake can keep several candidates (INT-040) | The job's codes stored as append-only narrowing rows, the first at card send; resolved from the catalog at the pick; any fee-free candidate makes the pick fee-free; no codes refuses a non-emergency pick, while an emergency pick proceeds on default one-time facts with a High alert; Confirm waits for it | T339 (new), T251 and T162 (extended), T343 (new, harness, fixtures), T340 (new); T261 and T328 depend on T340 | D-2330, D-2331 |
| N7-02 (P1) Tiered quotes cannot be picked | New (D-2210's logged refusal) | `Price::Tiers` gives no fee (`service.rs:417`); TD-FEE-48 pins it (`tests/integration/tests/fee_fixed_at_pick.rs:357`) | The page pick names a tier, stored on the pick; no default tier; Confirm shows it; a text pick of a tiers quote replies with the quote-page link (the spec gives no text syntax; Casey's call) | T341 (new, harness) | D-2332 |
| N7-03 (P1) Multiline contact details pass the quote gate | New | `gate_once` gates line by line (`app/quotes/src/gate.rs:63`); `text_bounce` (`app/quotes/src/intake.rs:369`) | A second whole-field view with whitespace folded; `gate.rs` unchanged; homeowner text already safe | T342 (new) | D-2334 |
| N7-04 (P1) Per-visit quotes cannot be picked | New (D-2210's logged refusal) | `fee_period(Period::Visit)` is `None` (`service.rs:439`); no schedule stored; payments converts weekly per-visit (`app/payments/src/rules.rs:198`) | The form requires a schedule (T164, stored in 0157); the parse carries it and bounces without one (T342); the pick converts it (T341) | T164 (extended), T342, T341 | D-2333 |
| N7-05 (P2) Global daily Verify cap not atomic | New | `send_code` takes only the contact lock (`service.rs:676`); the global count is a plain count (`app/picks/src/models.rs:183`) | A transaction-level lock keyed to the day before the count; Verify after commit | T344 | D-2335 |
| N7-D1 (note) Handyman split-job check is pairwise | Casey | `split_job` (`app/licensing/src/handyman.rs:227`) checks pairs as LIC-023 and D-154 word it | Open question for Casey: should the check add up all of a business's handyman jobs at an address in the window? No rule change | Casey | D-2329 |

## Answers the brief asked for

- **Does intake narrow to one job type before cards go out?** No. Under INT-040 a card can go with every remaining candidate, marked "Job type not certain" (`app/intake/src/routing_confidence.rs:111`), so D-2331's tie rules stand.
- **Is the category always known at emergency release?** Yes. The trade check (step 5) comes before "How soon?" and the emergency choice (INT-016, INT-030), and `offer_choice` (`app/intake/src/service.rs:604`) needs the category; only the job type can still be open, because the trade questions follow the cards (INT-029). The first emergency row holds the category and the candidates intake still holds; T162 narrows it later (review of PR #406).
- **Text-pick tier syntax:** the spec gives none (PICK-015 is `<n> YES`; PICK-038 and PICK-053 name the tier but no text form). A text pick of a tiers quote stores nothing and replies with the quote-page link; listed for Casey.

## Plan changes made

- New tasks T339, T340, T341 (harness), T342, T343 (harness, fixtures only) and T344 (the Verify day lock); T251 writes the first facts row; T162 writes narrowing rows; T164 requires and stores the visit schedule or a monthly "Other" price.
- T340 depends on T343; T341 on T340; T344 on T341; T261 depends on T340 and T341; T328 on T340; T308 on T341; T100 on T339 to T344.
- Migrations for V9 (nothing written): T251 0164 → 0165, T266 0165 → 0166, T103 → 0179, T105 → 0180, T320 → 0181, T204 → 0182, T151 → 0183, T321 → 0184, T109 → 0185, T110 → 0186; T339 0164, T341 0178.
- IC-82 to IC-84 and risk R-35 added.
- Independent (Fable) reviews when built: T340 and T341 (money); T339 has the money review area.
- No task IN_PROGRESS or MERGE_PENDING was edited (T284 IN_PROGRESS).
