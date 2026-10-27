# BUILD_BRIEF.md: how Ask Hava gets built

This file says **how** we build. `SPEC.md` says **what** we build. If they
disagree, stop and ask Casey.

## 1. How work happens

- Casey starts a Claude Code session on `casey451/askhava` and says what to do
  next, or "next step" to take the next unchecked item in `SPEC.md` section 9.
- One focused change per session, on its own branch, opened as a **draft pull
  request**. Casey reviews and merges. Nothing lands on `main` without that.
- Questions go to Casey in the session chat or on the pull request. If an answer
  changes what we build, it is written into `SPEC.md` in the same PR.
- Automation (scheduled runs, required checks, a state branch) is added later,
  only if it clearly saves Casey time.

## 2. Safety rules (no exceptions)

1. **Nothing real.** No sign-ups, purchases, publishing, deployments, live
   messages (SMS, email, push) or live charges. Payment and messaging code runs
   against test modes or fakes only.
2. **No secrets** in the repo, in commits, in logs or in chat. Config reads
   from environment variables, and `.env*` files are git-ignored. An example
   file may list variable names with empty or fake values.
3. **Fake data only.** No real customer names, phone numbers, emails or
   addresses. Use clearly fake values (for example `555-0100`, `@example.com`).
4. **Honest results.** Never claim a test, build or check passed unless it was
   run in that session and the output was seen. Say what was skipped.
5. **Never weaken a test** to make it pass: no deleting, skipping or loosening
   assertions. Fix the code, or explain to Casey why the test is wrong.

## 3. Definition of done for a change

- It does what the matching part of `SPEC.md` says.
- Tests for it exist and were run in the session (once a test setup exists).
- The PR description says what changed, how it was checked, and anything left open.
- `SPEC.md` is updated if behaviour or decisions changed.

## 4. Commits and branches

- Clear commit messages that say what changed and why.
- Never rewrite history on `main`. Never force-push someone else's branch.
