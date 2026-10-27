# SPEC.md: what Ask Hava is

> **Status: draft skeleton.** Sections marked **OPEN** need Casey's answers
> before any app code is written. Fill them in by replying in a session. Claude
> updates this file.

## 1. One-line summary

**OPEN:** What is Ask Hava, and what does someone use it for?

## 2. Users

**OPEN:** Who uses it (customers, business owner, staff)? What can each kind of
user do?

## 3. Core flows

**OPEN:** The 3–5 most important things a user does, step by step.
Example shape: *Customer picks a service → sees price → books → gets confirmation.*

## 4. Services, categories and prices

**OPEN:** The list of categories, with a few worked price examples
(fake numbers are fine for now).

## 5. Messages

**OPEN:** What messages are sent (text, email, in-app), to whom, and when?
All of these stay fake or test-mode during the build (see `BUILD_BRIEF.md` §2).

## 6. Payments

**OPEN:** Does it take money? When, and how much? Refunds?
Test mode only during the build.

## 7. Platform

**OPEN:** Website, phone app, or both? Any existing design, mock-up or older
version to reuse?

## 8. Out of scope (for the first version)

**OPEN:** What we're deliberately *not* building yet.

## 9. Build plan

Filled in once sections 1–7 are answered. Each item is one session and one PR.

- [ ] Project skeleton: chosen stack, test runner, lint, CI running tests on every PR
- [ ] (next items added after the spec is agreed)

## 10. Decisions log

| Date | Decision | Why |
|---|---|---|
| 2026-10-27 | Simple flow: one session = one PR, Casey merges; no runner automation yet | Starting from zero; automation can come later |
