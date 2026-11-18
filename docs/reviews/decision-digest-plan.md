# Decision digest plan (T720, D-3856, D-4447)

The plan every digest piece (T500 to T506, T553 to T558, and the pieces the overseer adds after T558) follows to write `docs/decisions-current.md`, the reader layer over the append-only `docs/decisions.md`. The log stays whole; the digest says what is in force. `python3 scripts/check_decision_digest.py` checks it (every entry up to the header's `Through D-####` counted exactly once; D-4447).

## Digest shape

- Line 1: `# Decisions in force`; the header line `Through D-####` (the highest number the digest covers) comes before the first `## ` heading. A piece moves it up, never down.
- One `## <area>` section per area below, in this order, then `## Superseded`, then `## Task-scoped`.
- Rule line, under its area: `- D-a[, D-b] (area) text`, the text at most 300 characters, saying the choice a later change must respect. A later decision that refines it joins the same line.
- `## Superseded`: one `- D-a -> D-b` line per reversed or replaced decision; the pair is in `kit/superseded_decisions.json` (add it when missing, then regenerate prompts). A pair whose newer number is above `Through` stays pending: keep the older number's rule line until `Through` reaches the newer one, then move it here.
- `## Task-scoped`: `- T###: D-a, D-b` for decisions about how one task was built (its fix rounds, its file choices); the T### exists in `kit/tasks.json`.

## Areas (`## <area>`)

- `## Product and scope`: what Laftly is and is not, the market, launch scope, what was cut (no directory, no trade cap), national-ready data (D-1515).
- `## Brand and wording`: the brand as data, banned words, "introduction fee by job size", the not-a-contractor line, copy rules and the cold read.
- `## Homeowner intake`: the request form, job types, photos, intake channels, scrubbing before AI.
- `## Businesses and licensing`: business sign-up, license and identity checks, the catalog's license scope, suspensions.
- `## Matching, quotes and picks`: who is invited, quotes, the pick, "Can't take it", job states and the lifecycle.
- `## Money`: the $25 / $75 / $199 introduction fee by job size, the 4-month recurring rule, Stripe, refunds, reconcile, the ledger.
- `## Privacy and the contact vault`: what leaves the vault and when, masked calls, retention, deletion, the PII rules.
- `## Texting and email`: consent, opt-out words, quiet hours, Spanish templates, the messaging adapters.
- `## AI`: the Claude API, zero retention, schema-validated output, the stand-in AI, the AI switch.
- `## Pages and accessibility`: Askama pages, routes, WCAG 2.2 AA, the a11y suite and page states.
- `## Security`: ASVS L2, sessions, tokens, rate limits, secrets, scanners, the threat model.
- `## Operations`: environments, Railway, workers and timers, backups, alerts, monitoring, Gate C.
- `## Code and architecture`: crates, dependency rules, shared paths, code docs, allowed crates, quality rules.
- `## Testing`: test design, the twin, coverage floors, e2e and integration suites, stand-ins.
- `## Runner and governance`: the runner, task graph, prompts, plan lock, gates, smoke and prehand-in, how Casey is asked.

A decision that fits two areas goes under the one a later change would look in first; never in both (each number is counted once).

## Digest ranges (the log as it stands, Nov 18, 2026)

Measured on `main` at `ed339f344` (log 4,252,246 bytes, 2,194 entries, D-001 to D-4447). The log is not in numeric order (D-516 precedes D-515), so a range is selected by number, never by file position, with the command T500's prompt gives (each line after an entry belongs to that entry):

```bash
awk -v a=A -v b=B 'match($0, /^(- )?\*\*D-[0-9]+\*\*/) {n = substr($0, RSTART, RLENGTH); gsub(/[^0-9]/, "", n); n += 0} n >= a && n <= b' docs/decisions.md | wc -c
```

Ranges are consecutive and each holds at most 300,000 bytes of entries. Pieces take them in turn; each re-measures at claim (the log grows, and a number reserved earlier may merge late inside an earlier range: the check names it, and the next piece takes it in).

| Piece | Range | Entries | Bytes |
|---|---|---|---|
| 1 | D-001 to D-366 | 366 | 299,761 |
| 2 | D-367 to D-582 | 174 | 296,192 |
| 3 | D-583 to D-945 | 158 | 299,120 |
| 4 | D-946 to D-1292 | 159 | 299,087 |
| 5 | D-1293 to D-1677 | 146 | 299,603 |
| 6 | D-1678 to D-1985 | 187 | 293,197 |
| 7 | D-1986 to D-2406 | 148 | 299,145 |
| 8 | D-2407 to D-2992 | 163 | 299,860 |
| 9 | D-2993 to D-3142 | 97 | 299,388 |
| 10 | D-3143 to D-3357 | 132 | 297,848 |
| 11 | D-3358 to D-3619 | 146 | 298,954 |
| 12 | D-3620 to D-3861 | 121 | 297,124 |
| 13 | D-3862 to D-4086 | 91 | 299,278 |
| 14 | D-4087 to D-4348 | 82 | 292,543 |
| 15 | D-4349 to the end | 24 | 80,618 |

**Piece count: the log needs 15 pieces as it stands.** Thirteen piece tasks exist (T500 to T506, T553 to T558); the overseer adds two after T558, before T514. Piece 15 is open-ended and grows with the log; a piece that measures more than 300,000 bytes at claim stops at the highest number that fits, and the remainder becomes one more piece.

## Size target

`docs/decisions-current.md` stays at about 200 KB or less (204,800 bytes by `wc -c`), so a session can read it with room to work. Budget per piece: about 13 KB for each 300,000-byte range (200 KB over 15 pieces). Ways to stay within it: one rule line per rule, not per decision (later refinements join the earlier line); task-scoped decisions as bare numbers on their task's line; superseded decisions as bare pairs; rule text well under the 300-character cap (about 120 is typical). A piece over its share shortens its own lines first; if the digest still passes the target, it hands in and states the overage in the report body (D-3892), for T514 and T142.
