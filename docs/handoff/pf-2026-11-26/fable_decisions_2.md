# Fable decisions 2 (for Casey, standing delegation) — 2026-11-26

Read-only pass on origin/main. Evidence: `/tmp/claude-0/verify/pf8/bf7a5732-laftly_COMPLIANCE-1_review.md`, `/tmp/claude-0/verify/pf8/COMPLIANCE_verdicts.md`, `/tmp/claude-0/verify/pf4/0dbe4f28-laftly_RUNNER-1_review.md:196-202`, `/tmp/claude-0/verify/pf4/3f0dc295-laftly_P2-DRAFTS-1_plan_fixes.md:625-631`.
Limits: no law was re-fetched (statute cites below are from the review plus my own knowledge of Cal. Civ. Code §1798.140, not re-read today); `claude/kit-state` was not readable here, so task statuses come from the verdict files. None of these is one of Casey's own stops; two items are labelled as launch-time rechecks that land at his Gate C stop.

---

## 1. CCPA / CPRA (Q5-02)

**Decision: CCPA most likely does not apply to Laftly at launch. Do not add a CCPA section, and do not state anywhere that CCPA "does not apply". Record an AI memo plus this decision (D-367), and recheck at Gate C.**

Reasoning, prong by prong (a "business" needs to do business in California AND meet one threshold, §1798.140(d)):

- **Revenue above $26,625,000** (CPPA adjusted figure, review §5): no. Fees are $25/$75/$199 per pick.
- **Buy, sell or share the PI of 100,000+ California consumers or households a year:** no. One Arizona town; and "consumers" means California residents only.
- **50%+ of annual revenue from selling or sharing consumers' PI:** this is the only live one, and it fails for two independent reasons.
  1. The paid transfer is consumer-directed. §1798.140(ad)(2)(A) says a business does not sell PI when the consumer "uses or directs the business to intentionally disclose personal information". The homeowner picks one business and asks Laftly to pass her first name and address; `docs/legal/privacy-policy.md:40` says "Only after you pick it, ask us to share them, and it confirms". The fee is for the introduction, the same act she directed.
  2. The prong counts revenue from California residents' PI. Most homeowners are Arizona residents, so even on the worst reading the share is far under 50%.
- **"Doing business in California":** probably not (Arizona LLC, Arizona-only service area, no California targeting), but I do not rest on it. Lake Havasu City sits on the California line and has many California-resident second-home owners, so California "consumers" will be in the data. The reviewer is right that an Arizona job address does not prove the person's residence.
- **Pre-pick job cards:** scrubbed text and photos with location data removed (`privacy-policy.md:32`), sent free and at her request, for cross-context advertising to nobody. Not a sale, not sharing.

Confidence: high on the outcome, medium on each single reason; the outcome needs only one of them. This is AI research, not legal advice (D-367).

**What the policy and spec say now**

- `docs/legal/privacy-policy.md`: no CCPA text. Keep the words "you pick it, ask us to share them" at :40 and "never sells or rents" at :64 unchanged; they are the record of consumer direction. Keep the voluntary copy/delete right for everyone (:99). Never write "CCPA does not apply".
- `docs/spec/privacy.md`: no new requirement IF the pick screen already names what goes to whom and the pick event logs her direction (PICK-018 / PRIV-020, `docs/spec/privacy.md:24`). The plan session checks that; if the pick screen does not state "your first name and job address go to <business>" before her confirming tap, add that as a small PICK spec change. Otherwise spec unchanged.
- `docs/legal/incident-response-plan.md:32` (verdict's optional nit): add "check for a shorter deadline in the affected person's home state before using 45 days (California: verify Civ. Code §1798.82 as currently amended)". The 30-day California figure in the review is from secondary sources, so the plan must say "verify", not state a number.

**Overseer plan:** one docs-only task in the existing compliance DOC-FIX batch (with Q2-06, Q3-03 and the `privacy-policy.md:84` photo-check fix), Sonnet with Opus review (privacy), before the laftly.com page go:
1. `docs/decisions.md` entry "CCPA applicability" with the reasoning above and the recheck triggers below.
2. `docs/for-casey-review.md` item, informational: "Decided by Fable under your standing delegation; recheck at Gate C."
3. One line in the Gate C part of the account checklist: "CCPA recheck (see D-…)".
4. The incident-plan sentence above.
No code, no policy text change for CCPA.

**LAUNCH-TIME RECHECK FOR CASEY (at Gate C, and each January after):**
- Current CPPA threshold figures (the revenue figure is CPI-adjusted each January) and any change to §1798.140's "sell" exception.
- Any California nexus: ads aimed at California, a California service area, California businesses on the platform, a California office or worker.
- Any new revenue that is not a homeowner-directed introduction: selling leads, ads, data deals, paid placement. Any of these reopens the 50% prong and must not ship without a new decision.
- Count of California-resident homeowners per year (far below 100,000; just log it).
- California breach-notice deadline (above).

---

## 2. Catch-all opt-out revocation (Q1-02)

**Decision: build it, in two tiers, as a hard gate before homeowner SMS at Gate C. Not phase 1.**

Why: the gap is real. `app/messaging/src/rules.rs:381-421` matches 12 provider words, "opt out" and 14 fixed phrases; "please cease all communication" and "leave me alone" match nothing, and CON-032 (`docs/spec/consent.md:36`) already promises "any reasonable revocation is honored". CON-035 (`consent.md:39`) covers other contacts only if the owner notices them. FCC 26-67 took effect Nov 25, 2026 (review §1). No text goes out before Gate C, so the legal exposure today is zero.

**Scope**

- **Tier 1, conclusive (widen CON-010's list).** Add phrases that cannot fit a normal request, English and Spanish: "cease" + contact/communication forms, "leave me alone", "stop contacting", "stop calling", "do not contact", "don't contact me", "quit texting", "lose my number", "déjenme en paz", "no me contacten", "no me llamen", "dejen de mandarme". Each gets a pattern ID (CON-036). The CON-011 guard stays (`consent.md:15`): the plan session drops any phrase that also fits a job request, and negative fixtures pin it ("can stop by at 3", "please cancel my request", "leave me a message", "wrong number of outlets").
- **Tier 2, "possible revocation" hold (new requirement, next free CON number).** A fixed signal list for messages that may be a revocation but are not conclusive: "wrong number", "número equivocado", "who is this", "how did you get my number", and any message that holds a CON-008 word, is not a sole word, matches no Tier 1 phrase, and that intake could not use as an answer. Effect:
  - all automated texts and automated calls to that number are held at once (checked in the same place as the suppression list, `rules::may_send_to`); the only exception is the existing 911 safety reply to a danger message;
  - no confirmation text and no clarifying text is sent (it is not yet an opt-out, and CON-009's 5-minute confirmation must not fire);
  - the message goes to the owner queue with two choices: "opt her out" (suppression list, channel "owner review", logged per CON-022) or "not a revocation" (hold released, queued texts resume under quiet hours and staleness rules);
  - owner alert at once and daily; **if not disposed within 5 business days the hold converts to an opt-out by itself.** Fail closed, so the 10-business-day limit never depends on Casey noticing.
- **Same detector on every channel** already named in CON-035 (help email, email replies, help form, web chat).
- **Spec changes:** CON-010 (longer minimum list), CON-035 (adds the hold, the queue and the auto-convert), CON-032 (its docs test maps the new item to a test-design entry), new CON requirement for the hold. `docs/decisions.md` entry, `kit/plan.lock` rewrite, `stand_ins/provider_optout.json` untouched (provider words do not change).
- **Not in scope:** AI classification of revocations. Deterministic only for launch; an AI "raise a hold" signal can be proposed later and may never clear a hold or opt anyone out. Log that as a deferred idea in the decision.
- **Q1-03 stays as the verdict says:** the "text START" line in the confirmation is not marketing (D-007, D-372); no change.

**Timing:** phase-2 / Gate C batch. Reasons: nothing real is texted before Gate C; `rules.rs` is already owned by the T801 chain's task A and by T704, T236, T823, T825, T680, T742, T818 and T817 (`fable_decisions.md` section C.3), so a phase-1 edit under the freeze adds conflicts and retries for no benefit.

**Overseer plan:** put the spec change (above) in the phase-2 plan PR, two Opus review rounds. One new Opus task (consent), rough 30-45M: owns the phrase list and hold in `app/messaging/src/rules.rs`, the owner-queue item, the alert/auto-convert job, fixtures from the review's acceptance gate 1 ("please cease all communication", "wrong number, leave me alone", mixed language, punctuation, email and web form), plus race tests with queued retries. Deps: after T801-A and T817. Mark it a Gate C blocker; CON-032's test must fail until it is done. Split the owner-queue UI into a second task if the measured context goes over 45%.

---

## 3. Runner batch open choices (RUNNER-1 review :196-202)

All five recommendations approved, with these fixed points. Build after phase 1, as `harness` tasks with Opus review (protected path `runner/`).

1. **Ambiguous CI evidence:** bounded wait, then operational BLOCKED, no charge. Use the existing 4-hour empty-evidence deadline (`cycle.py:1382`). The attempt is not counted against retries; reason recorded as ambiguous CI evidence; the PR is closed safely with the branch kept; independent-proof protections unchanged. Ambiguous is never treated as green, and there is no unbounded hold.
2. **Same-SHA replacement identity:** trusted app plus logical workflow/suite relationship plus coherent ordering; a same name or ID alone is not enough. If the strict projection never goes green, escalate; never filter the red out. Until a sanitized real incident payload (check-run, suite, workflow IDs) is captured as a fixture, the exemption stays off and strict behaviour is the default. Capturing that payload is the task's first step.
3. **Retro representation:** one optional `failure_reason` field (runner cause plus a short worker note), explicit empty string on new successes so legacy rows are distinguishable, 1,500-character cap, header redaction, original first-line report and retry log kept.
4. **Turn budget:** aggregate worker-only turns per attempt, not reset on fix rounds; reviewer turns excluded; cleanup and FIX-MAIN have no default cap; per-task override allowed. Version 1 is advisory only (the runner cannot enforce a live hard cap, review "Can't tell"). No cap number is fixed now: the 141/170/232 figures are not broken out by task type, so the task first reports per-type distributions and I set thresholds then.
5. **Threshold label:** append a factual threshold note to unsuccessful measured events and keep the real outcome; no automatic "budget failure" class. Use `turn_limit` graph metadata and the handoff, not persistent state. If the graph schema needs a new field, that is a separate harness plan-metadata change with its own `docs/decisions.md` entry; authorized.

**Overseer plan:** one runner batch after phase 1; no decision needed from Casey; record the five choices in one `docs/decisions.md` entry in the batch's plan PR.

---

## 4. Phase-2 draft open choices (P2-DRAFTS-1 :625-631)

1. **Checklist ownership/order: A.** The overseer makes a narrow plan-time edit to the account checklist now, before T127; T141's later work stays. T127 must not go out with the old checklist. (T127 itself remains Casey's account checkpoint; this only fixes what he is handed.)
2. **T236 reservation transaction: A.** One short atomic transaction; update the AI-029 clause by a logged decision. Cap values unchanged. Fewer partial-reservation paths and a simple race proof.
3. **T379 bilingual renderer: B, fallback A.** If a queued portal owner of the renderer exists, amend it, order it after T379 and give it the served-bilingual assertion. If none exists, add the verified renderer file to T379 with a measured budget. Either way TD-TXT-37 stays open until the served bilingual assertion passes on `main`.
4. **T704 producer ownership: B by default.** The 38% estimate excludes five added files, so split into narrow producer tasks after each current owner and before the final TD-QH-07 integration (T402 waits on the final one). Use A only if the measured context is 45% or less and the full graph shows no cycle. TD-QH-07 is not weakened and consent logic stays in T857.
5. **Unsupported/deferred vendor proof: A.** Each unavailable case stays NOT RUN with a follow-up attached to T130/T135; NOT RUN never counts as proof and never closes a launch requirement. Option B (a test-project setup and hands-on runs) needs accounts and Casey's hands, so it is **not delegated**: list it for Casey at the account checkpoint. No production key in any case.

**Overseer plan:** apply 1 now as a plan-time edit; fold 2-5 into the phase-2 plan PR with one `docs/decisions.md` entry; add the option-B vendor runs to Casey's account-checkpoint list in `docs/for-casey-review.md`.

---

## For Casey (labelled)

- **Launch-time recheck, at Gate C:** CCPA triggers (item 1) and the FCC revocation rule text and date (already required by CON-032).
- **At the account checkpoint:** whether to do the hands-on vendor test-project runs (item 4.5, option B).
- Nothing else here needs him.
