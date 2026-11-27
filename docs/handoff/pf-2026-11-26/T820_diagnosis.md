# T820 diagnosis (attempt 3, PR #1479, head 2ac3305)

## Cause: a test-design flake on main, not T820 and not T848
- Failing assert: `app/server/tests/action_links_route.rs:545-548`, `!again.text.contains("990")` ("the second price is not stored").
  It searches the WHOLE replayed job page for the substring "990".
- That page (`quote_again`, `app/server/src/action_links.rs:629-656`; template `app/portals/templates/business_actions.html:34-43`)
  carries random text on every run:
  - the CSRF token: 64 hex chars of a SHA-256 over a fresh random pre-session cookie
    (`app/portals/src/session.rs:315-322`; a new jar per `send`, test `:165-168`), template `:40`;
  - the not-a-fit form action with the job UUID (`Uuid::new_v4`) and the link token
    (`app/portals/src/not_a_fit.rs:60-64`), template `:39`.
  P("990" in 64 random hex) = 62/4096 = 1.5%; the UUID adds about 0.5%, the token more. About 2% or more per run.
- The assert just before it passed in the same run: "Quotes so far: 1." (`:540-544`), and the replay path stores nothing
  (`replay`, `action_links.rs:590-623`, read-only). So no second price was stored; the "990" was in a token.
- T820's diff (origin/main...branch, 30 files) touches no file on this path: nothing in `action_links*`, `job_page_route`,
  `business_actions`, `not_a_fit`, `session.rs`; in `app/server/src` only `homeowner_portal.rs`.
- T848 (94a9886) is not needed to explain it; the same test and code are on main 8d88d3d unchanged.
- Not run here: no test run was made in this session (reasoned from code); main's check-runs could not be read (API call denied).

## Recommendation: (b) plain requeue
The branch's work is complete on the evidence in state: Coverage, E2e, Ally, Twin, Lint, Python all green on attempt 3;
the only red is this one flaky test in a file T820 does not own.
Requeue note must say: switch to the prepared branch `claude/task/T820-data-download-contents` (never recreate/rebase),
change no product code, let the runner merge main, re-run Checks; td_sec_44 at `:546` is a known random-substring flake
(evidence above) and must not be "fixed" by T820 (it does not own `action_links_route.rs`; rule 6/7).

## Follow-up (separate, not a T820 plan fix)
A small logged task for the owner of `action_links_route.rs` (last owner T811) to make the assert exact, not weaker:
check the stored quote amount in the database is still 32500 cents, or search for "$990"/"990.00" in the quote line only.
Until then every Checks run on any branch has about a 2% chance of this failure.

## T862
T862 (conversation email in the data download) depends on T820 and owns `data_download.rs` after it; it stays blocked
until T820 merges, so requeue T820 ahead of it. No change to T862 needed.
