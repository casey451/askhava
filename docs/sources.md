# Sources: fingerprint and order of authority

`sources/` is the read-only input package. It must never change in this repository (BUILD_BRIEF section 2, rule 7).

## Fingerprint

- Files under `sources/`: **104**
- List: `kit/sources.sha256` (one `sha256sum` line per file, sorted by path, paths relative to the repository root)
- Combined digest (sha256 of `kit/sources.sha256` itself):
  `ed9f15e377d96323a9d46609e0e78d93d4378e6b1ea9c7d6e2f66926317d863d`

Verify from the repository root:

```
sha256sum -c --quiet kit/sources.sha256      # silent and exit 0 if every file is unchanged
sha256sum kit/sources.sha256                 # must print the combined digest above
find sources -type f | wc -l                 # must print 104 (catches added files)
```

The `Ci` workflow runs the first command on every push and pull request. Note that `sha256sum -c` catches edited or deleted files; an added file is caught by the file count.

## Order of authority

When two inputs disagree, the higher number wins.

1. **Decision layer.**
   - `BUILD_BRIEF.md` decides **how** the build is done.
   - `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md` decides **what** is built. Where it and any other source disagree, it wins (its "How to read this file").
   - If `BUILD_BRIEF.md` and the changes file conflict with each other: **stop and ask Casey** (outbox message), do not choose.
2. **Base spec:** `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md`, except where the changes file overrides it.
3. **Tests:** the digital twin `sources/01_spec/hhq_twin/` and its review tests (`review_tests.py`). The twin is stale on brand, trades and fees (see below) until it is updated in build Stage 1.
4. **Everything else:** the website prototype, business and design documents, reviews, the legal record, and history (`sources/03_business_and_design/`, `04_reviews/`, `05_history/`, `06_briefs_sent_to_other_ais/`, `07_legal/`, `02_build_plan/`). `02_build_plan/Kit_Plan_Before_Coding.md` states the same order: changes file, then v9.3, then tests, then the prototype and history.

## Superseded rules

Each row is a conflict listed in the table in "How to read this file" of `Changes_Since_v9.3_AUTHORITATIVE.md` (row numbers below count that table's rows from the top; "section 3" means that file's section 3, final decisions). Do not build the old value.

| # | Old rule (where it appears) | Authoritative value | Table row / section |
|---|---|---|---|
| 1 | Ongoing services count as **2 months** (`All_Services_Expansion_and_Role_Play_Changes.md`, Fees section) | Ongoing services count as **4 months** (a $150/month pool account counts as $600, so the fee is $75) | Row 1; section 3 |
| 2 | Quote window and other items listed as open (same file's decision checklist) | All decisions are settled | Row 1; section 3 |
| 3 | Ask Hava is a directory plus quotes; invite about 690 directory listings (`Ask_Hava_Quotes_Brand_and_Copy_Brief.md`) | The old directory is **deleted**; Ask Hava is the quote service only | Row 2; section 1 |
| 4 | "Your first win is free" for everyone (same file) | Free first win only for businesses joining in the **first 90 days** | Row 2; section 3 |
| 5 | "Up to 4 contractors per trade" (`Havasu_Home_Quotes_Business_Plan_and_Blueprint_v9.md`) | **No cap**: every qualified, opted-in business gets every matching card | Row 3 |
| 6 | Refund needs "2 attempts on 2 different days" (same file) | **2 attempts at least 3 hours apart within 72 hours** | Row 3 |
| 7 | Homeowner picks by replying YES (same file) | Pick by a **code texted to the homeowner's phone** (`2 YES` by text still works) | Row 3 |
| 8 | Fees $45 / $59 / $149 (same file) | **$25** if the chosen quote is under $500; **$75** for $500 to $1,500; **$199** over $1,500; no percentage cap | Row 3; section 3 |
| 9 | Brand "Havasu Home Quotes" (same file, and the twin) | Brand **Ask Hava** (and Ask Hava for Business) | Rows 3 and 4; section 1 |
| 10 | Twin: 5 trades, old fees, old brand (`01_spec/hhq_twin/`) | Twin is updated in build Stage 1 to 25 categories, new fees and all changes in the changes file | Row 4 |
| 11 | "Every quote shown, never ranked"; "Insurance on file: Yes/No"; "skip tenant requests"; texting "cancel" to close a job (website prototype, brand brief, revision notes) | The wording and rules in the changes file's section 7 win | Row 5 |
| 12 | Any build-plan copy exported before the Oct 27 reviews: sessions flip "passes"; K9 runs real app tasks; build 1 to 2 weeks (the copies in `02_build_plan/` are already updated) | Runner-only completion, practice-task rehearsal, revised estimates (changes file section 8) | Row 6 |
| 13 | Grok review, "Conflicts" item 1: goal possibly $20k to $50k **a month** | Goal is **$20k to $50k a year** | Row 7; section 3 |
| 14 | Grok review, "Conflicts" item 2: no homeowner details may go to any AI | Zero-retention AI is allowed, **after** a plain-code scrub | Row 8; section 3 |
