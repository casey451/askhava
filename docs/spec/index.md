# Ask Hava spec: numbered requirements

What to build, one file per area (`BUILD_BRIEF.md:80`). Every requirement here restates a source line; it never creates a rule. If a requirement and its cited source disagree, the source wins and the requirement is the bug. Where sources conflict, the choice is logged in `docs/decisions.md` and the requirement names that decision (for example "D-005").

## Format

Each requirement is one line:

```
- **TXT-001** One testable sentence. Source: `path/to/file.md:LINE`, `OTHER.md:LINE`.
```

- **ID:** the file's prefix, a hyphen and three digits. IDs are stable: never renumber or reuse one; retire a requirement by replacing its sentence with "Withdrawn by D-###." and keeping its ID.
- **Sentence:** one rule a test can check.
- **Source:** one or more `path:LINE` citations, paths relative to the repository root, line numbers as of the commit that last changed the line. Order of authority: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md` on what, `BUILD_BRIEF.md` on how, then the v9.3 blueprint, then everything else (`docs/sources.md:22`).
- Each file starts with a `Prefix:` line naming its ID prefix.

## Checks

`python3 scripts/trace_spec.py` fails (exit 1) unless:

- every ID is unique across all files and uses its file's prefix;
- every requirement has at least one citation, and every cited file exists inside the repository with a non-blank line at the cited number;
- every decision named (`D-###`) exists in `docs/decisions.md`;
- no requirement contains a superseded value (old brand, old fees, the old recurring-month count; `docs/sources.md:34`).

It prints counts per file, per cited source, and in total.

## Areas

| File | Prefix | Area |
|---|---|---|
| [intake.md](intake.md) | `INT` | Intake by text and web chat, emergencies, duplicates |
| [consent.md](consent.md) | `CON` | Consent, opt-out, HELP, business consent |
| [quiet-hours.md](quiet-hours.md) | `QH` | Quiet hours |
| [messaging.md](messaging.md) | `TXT` | Text templates, Spanish, links, retries, outbox |
| [job-cards.md](job-cards.md) | `CARD` | Matching and job-card content |
| [quotes-pick.md](quotes-pick.md) | `PICK` | Quotes, quote page, pick, confirm, release |
| [lifecycle.md](lifecycle.md) | `JOB` | Job statuses, timers, closing |
| [fees.md](fees.md) | `FEE` | Fees, charges, refunds, ledger |
| [recurring.md](recurring.md) | `REC` | Recurring services: the 4-month rule |
| [calls.md](calls.md) | `CALL` | Masked calls and voice calls |
| [privacy.md](privacy.md) | `PRIV` | Contact vault, retention, photos, logs |
| [ai.md](ai.md) | `AI` | AI boundary |
| [licensing.md](licensing.md) | `LIC` | Categories, licensing, legal routing, handyman |
| [businesses.md](businesses.md) | `BIZ` | Business sign-up, identity, accounts |
| [wording.md](wording.md) | `WORD` | Wording and claims |
| [accessibility.md](accessibility.md) | `A11Y` | WCAG 2.2 AA |
| [security.md](security.md) | `SEC` | Security (OWASP ASVS L2) |
| [ops-recovery.md](ops-recovery.md) | `OPS` | Monitoring, safe mode, backups, restore |
| [website.md](website.md) | `WEB` | Public website |
| [terms.md](terms.md) | `TERMS` | Terms, privacy policy, texting registration |
