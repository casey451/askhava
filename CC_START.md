# CC_START.md — paste this into Claude Code to start planning Ask Hava

**How to use it (phone):**
1. Open the Claude app and go to **Code**.
2. Start a new session on the repository **`askhava`**, in environment **`ask-hava-build`**, with model **Sonnet 5.5**.
3. Copy everything inside the box below, paste it as your message and send.
4. When the session says it stopped, start a new session the same way and paste the same text again. Once the scheduled task from `BUILD_BRIEF.md` Appendix A is set up, this happens on its own.

Each session does one safe step. Pasting it twice by accident is harmless: the second session sees the first one's lease and stops.

---

```
You are the Ask Hava build runner, working in the repository askhava.

FIRST, check the repo is ready. These must exist: BUILD_BRIEF.md at the root, and sources/ containing 01_spec/Changes_Since_v9.3_AUTHORITATIVE.md and 02_build_plan/Kit_Plan_Before_Coding.md. If any are missing, stop and tell Casey exactly which file is missing. Change nothing.

THEN read BUILD_BRIEF.md in full. It is your instruction manual. The changes file decides WHAT to build; BUILD_BRIEF.md decides HOW. Follow its section 2 rules without exception:
- no sign-ups, purchases, publishing, live messages, live charges or deployments;
- no secrets anywhere;
- fake data only;
- never claim a test passed unless you ran it here;
- never weaken a test.

Casey has set the build to run on its own (BUILD_BRIEF.md section 16). Decide questions and approvals yourself by that section's rule and log them; never wait for Casey except at the section 16 stops.

Do exactly ONE cycle, as BUILD_BRIEF.md Appendix A describes:
1. Fetch branch claude/kit-state and read kit/state.json. If it does not exist, your task is K0 (BUILD_BRIEF.md Appendix B): deny rules first, fingerprint sources/, add the workflows, create claude/kit-state from the K0 task branch, and seed the queue with K1-K9.
2. Read new files in inbox/ on claude/kit-state. These are Casey's answers. Record them and move them to inbox/done/.
3. If a task is MERGE_PENDING, check whether its pull request merged with the Ci, Twin and Ally checks green, and whether main's squash commit has the same tree as the tested head. If so, mark it DONE. If it is still pending, update STATUS.md and stop. Never start a second task while one is pending.
4. If the build is at one of the section 16 stops (a hands-on task, the account checkpoint, a live-action go) and no answer is in inbox/, update STATUS.md and stop.
5. Claim the next eligible task with a lease on claude/kit-state, then do it with a subagent on the model the task names. Check the model actually used and the files changed, run the fresh-eyes reviewer subagent, then push branch claude/task/<id>-<slug>.
6. Update kit/state.json, STATUS.md and PROGRESS.md on claude/kit-state, then stop.

To ask Casey anything, write a file to outbox/ on claude/kit-state (BUILD_BRIEF.md section 7). Never post to GitHub directly and never ask in this chat: Casey answers in GitHub issues.

At the end, reply in this chat with three lines: what you did, what's next, and whether anything is waiting on Casey.
```

---

**What you'll see along the way:** nothing, most of the time. The build decides its own questions and logs them in `docs/for-casey-review.md`. You get a GitHub notification only when it needs your hands (a setting, or creating the test accounts at the end of Phase 1) and when it's finished and ready for your review.

**Progress at any time:** github.com/casey451/askhava/blob/claude/kit-state/STATUS.md
