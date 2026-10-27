# Ask Hava: phone setup before the build (about 15–20 minutes)

You can do all of this from your phone. Use your phone's web browser (Safari or Chrome), not the GitHub app, for steps 1–7, because GitHub's settings pages only exist on the website.

**Tip:** if a GitHub settings page looks cut off, open the browser menu and choose **Request Desktop Website** (Safari: the "aA" button; Chrome: the ⋮ menu, then "Desktop site").

---

## Part 1 — GitHub (in your phone's browser)

**1. Sign in.** Go to **github.com** and sign in. You already have an account from the old askhava project.

**2. Create the empty repository.**
1. Go to **github.com/new**.
2. **Repository name:** `askhava`
3. Choose **Private**.
4. Tick **Add a README file**, so the repo has a starting `main` branch.
5. Tap **Create repository**.

**3. Give Claude access to it.**
1. Go to **github.com/apps/claude**.
2. Tap **Configure** (or **Install** if it was never installed).
3. Pick your account.
4. Under **Repository access**, choose **Only select repositories**, then add **askhava**. If the app was already installed for the old askhava repo, just add `askhava` to the list.
5. Tap **Save** (or **Install**).

**4. Tell Claude the repo name.** Send me the name here in chat as `casey451/askhava`. I'll attach the repo and push all the files:
- `BUILD_BRIEF.md`;
- this guide;
- the full plan package under `sources/`.

I'll tell you when the push is done. **Wait for that before step 5.**

**5. Protect the main branch.**
1. Open **github.com/casey451/askhava/settings/rules** (Desktop site on).
2. Tap **New ruleset**, then **New branch ruleset**.
3. **Ruleset name:** `protect-main`
4. **Enforcement status:** Active
5. **Target branches:** tap **Add target**, then **Include default branch**.
6. Under **Rules**, tick:
   - **Restrict deletions**
   - **Require a pull request before merging**. Set "Required approvals" to **0**.
   - **Require status checks to pass**. Then:
     - tick **Require branches to be up to date before merging**;
     - tap **Add checks** and type each name, adding it one at a time: `ci`, `twin`, `security`, `a11y`.
   - **Block force pushes**
7. Tap **Create**.

If GitHub won't let you add check names that haven't run yet, skip just the four check names for now. Claude will send you an issue after its first task reminding you to add them; everything else in this step still applies.

**6. Merge settings.**
1. Open **github.com/casey451/askhava/settings**.
2. Scroll to **Pull Requests**:
   - untick **Allow merge commits**;
   - leave **Allow squash merging** ticked;
   - untick **Allow rebase merging**;
   - tick **Allow auto-merge**;
   - tick **Automatically delete head branches**.

**7. Actions settings.**
1. Open **github.com/casey451/askhava/settings/actions**.
2. Under **Workflow permissions**:
   - choose **Read repository contents and packages permissions**;
   - tick **Allow GitHub Actions to create and approve pull requests**.
3. Tap **Save**.

**8. Turn on notifications.**
1. In the **GitHub phone app**, open the `askhava` repo.
2. Tap **Watch**, then choose **All Activity** (or at least Issues).
3. Make sure push notifications are on for the GitHub app in your phone's settings.

This is how Claude reaches you during the build.

---

## Part 2 — Claude Code (Claude app, or claude.ai/code in your browser)

**9. Connect GitHub to Claude Code.** In the Claude app, open **Code**. If it asks to connect GitHub, follow the prompts and allow access to `askhava`.

**10. Create the cloud environment.**
1. In Code, open the environment selector (it usually shows **Default**).
2. Choose to add a new environment.
3. **Name:** `ask-hava-build`
4. **Network access:** Trusted (the default).
5. Leave environment variables, network secrets and the setup script **empty**.
6. Save.

**11. Start the build.** You have two ways. Use A if it works on your phone.

- **A. Scheduled task (automatic):**
  1. Open **claude.ai/code/routines** in your browser.
  2. Tap **New task** (or **New routine**).
  3. Fill it in exactly as `BUILD_BRIEF.md`, Appendix A, describes: the name, Sonnet 5.5, the `askhava` repo, the `ask-hava-build` environment, no connectors, the GitHub trigger (pull request closed, merged into `main`), the hourly schedule `7 * * * *`, and the prompt pasted exactly.
  4. Tap **Create**, then **Run now**.
- **B. By hand (if the scheduled-task page won't work on your phone):** follow `BUILD_BRIEF.md`, Appendix C. Start a Code session on `askhava` with Sonnet 5.5 and paste the same prompt. When it finishes, start another. You can switch to A later from a computer.

---

## What happens next

- **The first sessions plan the build:** sources and rules, the spec, the category list, architecture, tests, the task list and the runner. No app code is written until you approve Gate A.
- **When Claude needs you,** a GitHub issue appears and the GitHub app notifies you. Reply in the issue. Don't reply in chat; the build only reads GitHub.
- **Expect these questions during planning:**
  - a few one-at-a-time questions (K3);
  - approval of the category list and worked examples (K2b);
  - Gate A.
- **Progress:** check STATUS.md any time at **github.com/casey451/askhava/blob/claude/kit-state/STATUS.md**.

## Later, at a computer (optional)

Set up the backup runner: run `claude setup-token`, then add the token as the repo secret `CLAUDE_CODE_OAUTH_TOKEN` (repo **Settings → Secrets and variables → Actions → New repository secret**). It only runs if the main runner ever fails to start.
