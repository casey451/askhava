# Havasu Home Quotes: Autonomous Build Plan

Oct 26, 2026 · @Casey Solomon

## What "OK go" covers

Claude builds as much of the app as possible first, asks you to create the remaining accounts at a checkpoint, then finishes the build, tests it against the digital twin and the vendors' test modes, and leaves it running in a test environment, ready to switch on. Going live still waits on a few approvals that no AI can give.

**Claude does without you:** write all the code, the database and the three portals; connect Twilio, Stripe, a stand-in for the Claude API, email and monitoring in test mode; run the twin and every build test after every change; deploy to the test environment; write the runbook, the restore drill and the owner guide; and report progress at each stage.

**Only you can do** (your own guardrails: AI never creates accounts, accepts terms or moves money):

1. Form the LLC, get the EIN, and name a statutory agent.
2. Create each account, pass identity checks, turn on two-factor sign-in, accept terms and add payment.
3. Enter each key straight into the secret store the plan names, never into chat.
4. Approve each stage gate, and give the final "go live."

**Outside parties, on their own clocks:**

- **Attorney (optional, any time):** a later review of the working legal answers in the changes file (section 7.1). Nothing waits on it.
- **Twilio:** texting registration (A2P 10DLC) approval, plus their read on the matching model.
- **Anthropic sales:** the zero-data-retention agreement. Until it's signed, the app uses plain-code scrubbing and keeps photos off job cards.
- **Insurance broker:** binding the policy.
- **Native Spanish speaker:** reviewing the Spanish texts.

The build doesn't wait for any of these. They only gate the switch from test to live.

## Changes after the outside reviews

On October 27, ChatGPT stress-tested this build plan and Grok researched the legal questions. Claude checked their findings against the official docs and the law. What changed:

- **The runner is real code, written and tested in the kit** before any app code (see Pace and quality controls). Sessions hand in work; only the runner marks it done, from GitHub's test results.
- **No real Claude API during the build.** Hidden keys are never attached to calls to api.anthropic.com, so the build uses a stand-in AI. The real AI is tested from the Railway test site at Gate B.
- **A checked category list comes first.** Each of the 25 launch categories gets its license rule, allowed jobs and source. A category that isn't confirmed stays switched off; the AI never guesses.
- **Worked examples you approve** (prices and matching) are written before the twin or the app, so both are tested against something neither one wrote.
- **Fake data only** in the build: no real names, phones, addresses or photos in code, tests or logs, with a scanner that fails the build if any show up.
- **The test site and the live site are kept fully separate:** different Railway projects and keys, and the live site never deploys automatically.
- **The simple askhava.com page (Stage 0.5) goes up only after your separate go.**
- **Texting changes:** a text that is only "cancel" (or stop, quit, end, revoke, opt out, unsubscribe) means stop all texts, as the FCC's new rule requires. Jobs are closed with "CLOSE" plus the request number or the button on the quote page.
- **Legal questions answered:** Grok's research is built in, and all 18 open questions have a working answer with a fallback (changes file, section 7.1). Nothing waits on an attorney.
- **Launch timing:** about 6–12 weeks or more from now. It depends mostly on the LLC and EIN, then Twilio's registration approval, not on the coding.

## The build brief for Claude Code

`BUILD_BRIEF.md` (in the package root, and at the root of the GitHub repo) is the file Claude Code reads first, every session. It fixes:

- **the stack:** Python and FastAPI with server-rendered pages, and PostgreSQL;
- **the runner design:** one task at a time; a GitHub trigger on merged pull requests; auto-merge only on passing tests; state on its own branch;
- **security:** OWASP ASVS Level 2 as the checklist; encrypted contact vault; signed webhooks; strict headers; secret and PII scanning; retention periods;
- **accessibility:** WCAG 2.2 AA, with tests on every page and a VoiceOver check by you at Gate B;
- **compliance**, **testing layers**, **quality rules**, **environments** and the **handover pack**;
- **the exact points where Claude stops and asks you.**

It also holds your 15-minute setup checklist and the scheduled-task prompt to paste in.

## Build order: everything first, accounts at a checkpoint

You asked for as much as possible to be built before you set up accounts. So the build runs in three phases, and Claude stops and asks for accounts only when nothing more can be built without them.

**Before starting, only two accounts:** GitHub (an empty private repo with the Claude GitHub App) and your Claude Max plan with its cloud environment. No vendor keys, no LLC.

**Phase 1 — Build everything against stand-ins (no other accounts).** The kit (K0–K9) and stages 1–5 of the app are built and tested completely. Every outside service is replaced by a faithful stand-in:

- **Texting and calls:** the twin's fake Twilio.
- **Payments:** Stripe's official mock server, plus the twin's fake Stripe.
- **Email and letters:** fake Resend and fake Lob.
- **The AI:** a stand-in AI.
- **The database:** a local Postgres in the cloud session.
- **The website:** tested in a built-in browser.

Each stand-in is tested against the vendor's published API documentation, so swapping in the real service later is a small, predictable step. This phase also produces:

- the website, portals and owner tools;
- the terms, privacy policy and SMS terms;
- the Twilio campaign description;
- the DNS records;
- the runbook and the owner guide;
- a step-by-step account checklist written for you.

**Account checkpoint — Claude stops and asks you.** When Phase 1 is done, the runner marks the build "waiting for you." It opens a GitHub issue titled **"Your turn: set up accounts"** (GitHub emails you and notifies you in its phone app) and puts the same checklist at the top of STATUS.md. The checklist covers, in order, each account to create, what to click, and exactly where each test key goes. Nothing is guessed, and Claude never creates accounts itself. The checkpoint is in two parts:

1. **Test accounts (no LLC needed):** Railway, Stripe (test mode), Twilio (trial), Resend, Lob, Sentry and UptimeRobot, plus a Claude Console workspace for the real-AI test. You add each test key as a network secret or Railway variable; the checklist says which.
2. **Business accounts (for going live; can run in parallel):** the LLC with a registered agent, the EIN, Google Workspace email, the Arizona TPT license, insurance, the Twilio business registration, Stripe activation and the zero-retention request. These gate only Gate C.

**Phase 2 — Final build and testing with real test accounts.** When you reply "accounts done" in the issue, the runner resumes:

- it swaps each stand-in for the real vendor sandbox and runs every vendor test against it;
- it deploys to the Railway test site;
- it tests the real AI from there;
- it runs the restore drill on Railway;
- it fixes whatever the real services revealed.

Then comes **Gate B**, your guided click-through. Once the business accounts are in place, **Gate C** follows: you enter live keys and say "go live," and a real-phone rehearsal runs on your own phone.

**Time:** most of the work (about 80–90%) happens in Phase 1, and you're needed only for decisions it raises. Phase 2 should be the shorter part, about 1–2 weeks, because the stand-ins were built to match the real services.

## Accounts to create

Only GitHub and the Claude Max plan are needed before the build starts. Claude asks for the rest at the account checkpoint, with a step-by-step checklist (see Build order). The full list, in the order below; each later one depends on an earlier one. About $60 to $80 a month once running, plus $200 a month for the Claude plan while building. You create every account, turn on two-factor sign-in, and accept the terms yourself.

| # | Account | Sign up at | Plan and cost | What you do | Keys, and where they go |
| --- | --- | --- | --- | --- | --- |
| 1 | **Bitwarden** + an authenticator app | bitwarden.com | Free | Store every login and recovery code here | Nothing for Claude |
| 2 | **Arizona LLC** (Arizona Corporation Commission) | azcc.gov, Arizona Business Center | $50 filing; registered-agent service about $50–$150 a year (confirm with the Commission, 602-542-3026) | File the Articles naming a commercial registered agent with a Maricopa County address (no newspaper notice needed, and your home address stays private) | — |
| 3 | **EIN** (IRS) | irs.gov "Get an EIN" | Free, issued on the spot | Apply once the LLC is approved, in one sitting | — |
| 4 | **Domain** (Cloudflare Registrar) | dash.cloudflare.com | Renewal only | Not needed: you already own askhava.com. Move its DNS to Cloudflare (see Moving askhava.com) | Claude gives you the DNS records to paste |
| 5 | **Google Workspace** email | workspace.google.com | About $7 a month | Create your branded address (help@ and you@) | — |
| 6 | **GitHub** | github.com/signup | Free | Create an empty private repo; install the Claude GitHub App on it | None: access goes through the app |
| 7 | **Claude Max** plan | claude.ai | $200 a month (Max 20x), chosen so the build runs at full speed | At claude.ai/code, create the cloud environment "hhq-build" | You add the test keys below as **network secrets** |
| 8 | **Railway Pro** (hosting and database) | railway.com (you already have an account) | $20 a month including $20 of usage; Postgres rewind about 4 weeks. Fallback: Render ($26 a month, 3-day rewind) if your plan doesn't show point-in-time recovery | Upgrade to Pro; delete the old askhava project; add a Postgres database and confirm its Backups tab offers point-in-time recovery | Live keys go in the live service's variables at Gate C |
| 9 | **Stripe** | dashboard.stripe.com/register | 2.9% + 30¢ per charge, no monthly fee | Sandbox works at once. Activation later needs the EIN, bank account and live website | A **restricted test key** (rk\_test\_) as a network secret; live restricted key in Railway at Gate C |
| 10 | **Twilio** | twilio.com | Number $1.15 a month; registration $4.50 brand + $15 campaign + $1.50 a month; texts about 1¢ each | Upgrade from trial; buy a 928 number; Business Profile with the LLC name exactly as on the EIN letter; brand **Low-Volume Standard** (an LLC with an EIN can't use Sole Proprietor) | Test credentials as a network secret if your account offers them; live keys in Railway at Gate C |
| 11 | **Claude Console** (the app's AI) | platform.claude.com | Pay as you go; about 2¢ per request | Create a "havasu-prod" workspace with a low spend limit; request zero data retention from sales | Workspace key in Railway at Gate C |
| 12 | **Resend** (email) | resend.com | Free up to 3,000 a month | Add the DNS records Claude gives you | Sending-only key as a network secret |
| 13 | **Sentry** + **UptimeRobot** | sentry.io, uptimerobot.com | Free plans | Create a Python project; add a monitor on the site's health page | Sentry's DSN is safe to share; nothing else needed |
| 14 | **Lob** (mailed identity letters) | lob.com | No monthly fee; about $1.06 a letter, rising slightly on 11/1/2026 | Add a payment method | Test key as a network secret; live key in Railway at Gate C |

**The longest waits:**

- **Twilio's "lead generation" rule:** Claude read Twilio's rules, and Ask Hava doesn't violate them. Consent is only for Ask Hava's own texts, numbers and opt-ins are never shared, businesses never text homeowners, and the homeowner picks the one business that gets a name and address. A rejected campaign can't be resubmitted, so registration waits until the full site is live and uses the careful wording in the changes file. If Twilio ever objects, the app runs on web and email.
- **Twilio's texting registration:** the profile takes 72 hours or more, and the campaign 5–15 days. It needs the live website with the privacy policy and opt-in page, so Claude builds those first.
- **The LLC filing:** about 1–2 weeks by standard filing; faster for a fee. A registered agent with a Maricopa County address avoids the newspaper notice a Mohave County agent would need.
- **Insurance and tax setup:** general liability plus tech E&O insurance, and an Arizona TPT license, both before the first live charge.
- **Zero data retention** depends on Anthropic's sales team.

Start these first, beginning with the LLC.

## Moving askhava.com to the new business

The business is now **Ask Hava**, and askhava.com becomes its home. The old directory site doesn't carry over: you've decided it can all be deleted. This is Stage 0.5, done right after the account checkpoint and before the texting registration, because Twilio needs a live askhava.com with the privacy policy and texting terms.

**What's there now** (checked October 26, 2026):

| Piece | Today | What happens |
| --- | --- | --- |
| Registrar | GoDaddy, paid through May 7, 2029 | Stays. Nothing to move |
| DNS | Already on Cloudflare | Stays. Old records removed, new ones added |
| Website host | Railway | You delete the old project; nothing is saved |
| Code | A GitHub repository from earlier Claude sessions | You delete it (or archive it, if you'd rather keep a private record) |
| Data and scrapers | About 2,000 listings, events, scheduled scrapers | Deleted with the Railway project, so the scrapers stop |
| Email | None on the domain | Nothing to protect; Google Workspace sets up new addresses |
| Google Search Console | Verified | Kept, so the domain's search history carries over |

**Steps:**

1. **You delete the old site:** the Railway project (site, database and scrapers) and the old GitHub repository. Claude can't reach those accounts and doesn't need to.
2. **Claude gives you the new DNS records** (the app host, Google Workspace email and Resend) to paste into Cloudflare, and the old ones to remove.
3. **First switch: a simple Ask Hava page** with what's coming, the privacy policy, the texting terms and the opt-in description. Any old directory address shows this page instead of an error. This is the page Twilio's reviewers check.
4. **Second switch, at Gate C:** the full Ask Hava quote site and app go live on askhava.com.

One side effect: the idea of inviting the businesses listed on the old directory goes away. Recruiting stays as planned, through Facebook group posts.

## How credentials reach the build

You never hand Claude a password or paste a key into chat. You give Claude **access**, through three locked places where it can use a key without ever reading it.

| Where | What goes there | Who enters it | Can Claude read it? |
| --- | --- | --- | --- |
| **Network secrets** on the Claude Code cloud environment | **Test-mode** keys the build needs: Stripe restricted test key, Twilio test credentials, Resend sending-only key, Lob test key | You, at claude.ai/code. Each key is locked to its vendor's web address | No. Anthropic's proxy adds the key to calls to that vendor. The value can't be viewed again after saving ([docs](https://code.claude.com/docs/en/cloud-environments)) |
| **Railway service variables** | **Live** keys: Stripe live, Twilio live, Claude API, Resend, Sentry | You, in the Railway dashboard, at go-live, from a checklist | No. Only the running app reads them |
| **Claude GitHub App** | Access to the code repository | You install it with one click | Claude works in the repo; GitHub access goes through Anthropic's GitHub proxy, with no token to copy |

**Rules the build follows:**

- No keys in chat, code, CLAUDE.md or plain environment variables. Anyone using a cloud environment can read its plain variables, so the docs say to use network secrets instead.
- The build only ever holds **test** keys. A deny rule blocks it from touching live keys or the live service.
- Your own logins and two-factor codes stay in a password manager (Bitwarden or 1Password). Claude never needs them.
- Two separate Anthropic accounts:
  - Your **Claude plan** runs the build.
  - A **Claude Console** API organization runs the app's AI, with zero data retention.

  Cloud build sessions aren't available inside a zero-retention organization, so the two must stay separate.

* **No hosting key for Claude.** Claude never holds a key to the hosting account. The test service deploys automatically from GitHub. The live service has automatic deploys turned off, and you press Deploy at Gate C.
* **Least-privilege keys only:** a Stripe restricted key (Stripe recommends these for keys given to AI agents), Twilio API keys rather than the master token, a Resend sending-only key, and an Anthropic key scoped to one workspace with a spend limit.
* Any key that ever lands in chat or a log gets replaced right away.

## Which Claude feature does what

The build runs in **Claude Code cloud sessions**: Opus plans and reviews, Sonnet writes code, and hooks and deny rules enforce the guardrails. **Cowork** handles operational browser work after launch. Features marked *preview* are new and may change, so nothing in the plan depends on them alone.

| Feature | Used for | Why this one |
| --- | --- | --- |
| **Cloud sessions** (claude.ai/code) | Every build session | Runs in Anthropic's cloud with your computer off. Uses the network secrets. You can check in from your phone |
| **CLAUDE.md** (under 200 lines) | Project rules, your guardrails, where the spec lives | Loaded into every session. The docs recommend keeping it short |
| **Spec files in the repo** | Blueprint v9.3, the 18 changes, the category and license table, prices, the twin's tests | Each session reads only the part it needs, which keeps context small |
| **Plan mode** with Opus 5.5 | Start of every stage: read the spec, write the stage plan, list the tests | Opus is the strongest planner; nothing is edited while planning |
| **Sonnet 5.5** | Writing and fixing code | Handles most coding well at lower usage ([docs](https://code.claude.com/docs/en/costs)) |
| **Subagents** | Test writer; independent reviewer (fresh context, sees only the diff and the spec); privacy and texting-compliance checker; twin runner (Haiku) | Each has its own context window, so long logs don't crowd the main session |
| **Hooks** | A Stop hook sends a session back to work when tests fail, for fast feedback. The official pass or fail comes from GitHub's test run on the exact commit. A pre-edit hook blocks writes to secret files | Deterministic: they run every time, unlike instructions |
| **Deny rules** in settings | No live keys, no deploy to the live service, no force-push, no reading key files | The one control that holds in every mode, even after a long session is compacted |
| **Auto mode** | Running unattended | A separate checker reviews each risky action. It blocks production deploys and data sent outside by default. It "does not guarantee safety," so deny rules back it up |
| **Skills** | Runbooks: deploy to test, run the twin, test Stripe webhooks, Twilio test texts, restore drill | Loaded only when needed |
| **Git worktrees** | Not used: tasks run one at a time (decided after the build review) | Separate copies of the code, so parallel sessions don't collide |
| **GitHub App and Actions** | Every change runs the tests and the twin; @claude reviews each pull request | The record you can audit, and a second check outside the session |
| **Routines** (*preview*) | A nightly twin run and a morning status note; after launch, daily ops checks | Run in the cloud on a schedule or when called through their API. The runner task starts on every merged pull request (GitHub trigger), with an hourly run as a safety net |
| **Cowork with Claude in Chrome** | ROC and pest-license lookups; owner-portal chores | Browser work by your side. Chrome won't create accounts or make purchases ([support](https://support.claude.com/en/articles/12012173-using-claude-in-chrome-safely)) |
| **Claude API, Haiku 5.5** (inside the app) | Reading requests, scrubbing, reading quotes, photo checks | Cheapest model; zero retention via the Console organization |
| **Claude Docs** (this doc) | The plan, stage reports and decisions | One place you can read and comment on |

**Not used:**

- **Agent teams.** Experimental, off by default, and they don't survive a resume.
- **The Agent SDK.** The app never runs an AI agent loop; it reads fixed fields from each AI answer.

## Session layout and context

Each stage gets one lead session that plans, builder sessions that run one task at a time, and short-lived subagents for tests and review. No session ever has to hold the whole project.

&#91;embedded content: build sessions · who does what\]

A failed test or twin run sends the change back to its builder; nothing reaches the test environment until it passes.

**How context stays small:**

1. **The spec is split into files** (lifecycle, money, texting, matching, portals, prices). A session loads only the files its task needs. CLAUDE.md just points to them.
2. **One task per session.** A builder starts fresh, reads its task and spec files, works until the Stop hook passes, then commits and ends. The docs advise clearing between unrelated tasks, and fresh sessions do that automatically.
3. **Subagents absorb the noise.** Test logs, twin results and code searches run in a subagent's own context, and only a summary comes back.
4. **Hand-offs live in files, not memory.** Each session ends by updating `PROGRESS.md` (what's done, what's next, open issues). The next session starts by reading it.
5. **Long sessions compact automatically.** Rules that must survive compaction are deny rules and hooks, not chat instructions.
6. **One task at a time.** Tasks run one at a time, in order. There's no parallel building, so there are no merge conflicts or state races. Only read-only reviews run in parallel, as subagents.

Models: Opus 5.5 and Sonnet 5.5 both have 1-million-token context windows in Claude Code ([docs](https://code.claude.com/docs/en/model-config)). The plan keeps sessions far below that anyway, because smaller context means faster work and fewer mistakes.

## Build stages and gates

Seven stages, each ending in a check that a machine runs, not a judgment call. You approve three gates: the spec, the finished test environment, and going live.

| Stage | What gets built | Done when | Your part |
| --- | --- | --- | --- |
| **0. Setup** (Phase 1) | Repo, the spec split into files, CLAUDE.md, subagents, hooks, deny rules, CI, the runner, and stand-ins for every outside service | CI passes on an empty app; every stand-in passes its contract tests from the vendor's API docs | **Gate A:** approve the kit summary and rehearsal proof, then "OK go" |
| **1. Twin first** (Phase 1) | The simulator updated for 25 categories, the new prices and all changes; its rules become the app's rules module | Every twin check passes, and any failed check makes the test run fail | — |
| **2. Core** (Phase 1) | Database, job lifecycle, timers, contact vault, audit log, text intake, job cards, the quote page, picking with a code | Build tests T01–T03, T12, T20–T24 pass against stand-ins | — |
| **3. Money** (Phase 1) | Stripe customers, saved cards, confirm-to-pay, the $25 / $75 / $199 tiers, refunds, disputes, ledger, nightly check | T04–T07, T13, T17 pass; the ledger matches the Stripe mock on 1,000 simulated jobs | — |
| **4. Calls, checks, portals** (Phase 1) | Masked calls with job PIN, license and identity checks, the three portals, email fallback, Spanish texts, web-and-email mode | T08–T11, T14, T18 pass; portals match the ledger | — |
| **5. Website, owner tools, documents** (Phase 1) | Public site from the prototype (WCAG 2.2 AA), owner portal, alerts, pause switches, safe mode, terms, privacy and SMS terms, Twilio campaign wording, DNS records, runbook, owner guide, the account checklist | T15, T16, T19 pass; automated accessibility scan clean, plus keyboard, 200% zoom and 320px checks; a full twin chaos run with zero new problems; a fresh Opus audit of the whole codebase | — |
| **Account checkpoint** | The runner stops and opens the "Your turn: set up accounts" GitHub issue with the checklist | You reply "accounts done" | Create the test accounts and add their keys where the checklist says; start the business accounts |
| **6. Final build and testing** (Phase 2) | Each stand-in swapped for the real vendor sandbox; Railway test site deployed; real AI tested from there; restore drill on Railway, including the switchover to the restored copy; fixes | Every vendor test passes against the real sandboxes; the restore drill passes; screen-reader check done | **Gate B:** a guided click-through of the test site |
| **7. Go live** | Live keys entered, live config, smoke tests, monitoring on, real-phone rehearsal | All business steps done (LLC and EIN, Twilio registration, Stripe activation, insurance, TPT license, zero retention, Spanish review) | **Gate C:** enter live keys in Railway and say "go live" |

**Two limits to know:**

- **Real texts to real phones need Twilio's registration approval.** Until then, testing uses Twilio's test credentials (which never send real texts, and which newer accounts may not offer) and the twin's fake Twilio. One real-phone rehearsal on your own phone happens right after approval.
- **No real charge happens before Gate C,** and the first live charge still needs your explicit go, per your guardrails.

After every stage, Claude updates STATUS.md in the GitHub repo (the official record) and copies a short report into this doc when it can: what's done, the test results, anything that needs you, and what's next. If a stage fails its check twice, the build stops and asks rather than guessing.

## Pace and quality controls

The build runs at the full limits of a Max 20x plan until it's done, and every rule below exists to keep the result clean, consistent code rather than an AI-made mess.

**Pace: as fast as the plan allows**

1. **A runner, not a timer, moves the queue.** Scheduled cloud tasks can't run more often than once an hour, so the scheduled task has a GitHub trigger: every merged pull request starts the next runner session. GitHub merges each pull request automatically once its required tests pass, and the hourly run is the safety net. No API token is needed (hidden keys are never sent to Anthropic's own address). Full design in BUILD\_BRIEF.md, section 7.
2. **Only the runner marks a task done**, and only after GitHub's tests pass on the exact commit that was merged. A session that says it finished proves nothing: the docs say a run's "Succeeded" only means the session ended normally.
3. **One task at a time per file area.** The runner takes a lease on a task before starting it, so two sessions can't grab the same task, and merges go through one at a time with the tests rerun after each merge.
4. **Usage limits:** when the plan's limit is hit, new runs are rejected, not queued. The runner marks the task "retry after reset" and the hourly safety net restarts it once the window resets. No pay-per-use.
5. **Failures:** a Sonnet task that fails twice gets one Opus try. After that the task is marked blocked, every task that depends on it waits, and you get one plain question about it. Flaky tests, vendor sandbox outages and wrong tests are logged as their own kinds of failure, not retried blindly.
6. **Backup runner:** if scheduled tasks (a preview feature) misbehave, a GitHub Actions schedule runs the same runner using a subscription token you create with `claude setup-token`. It uses your Max plan, not pay-per-use.
7. **Time:** the rehearsal in kit step K9 measures real usage per task. Estimates so far range widely: about 2–8 weeks for the app build (our first guess of 1–2 weeks was optimistic; ChatGPT's review estimates 3–8). Your own Claude chats draw from the same limits.

**Context: every session stays small**

- Each task has a measured token budget: its spec files plus the code it touches plus room to work, kept to about 15–20% of the context window, so automatic condensing never kicks in mid-task.
- CLAUDE.md stays under 200 lines. Everything else lives in spec files that are loaded only when needed.
- One task per session; long logs go to subagents; hand-offs go through the progress file.

**Quality: no mess at the end**

1. **The architecture is fixed before code starts.** Every module, its job, and which files it owns are written in ARCHITECTURE.md. A task may touch only its own files.
2. **Tests come first.** Each task starts from tests already written from the spec and the twin. Code is done when they pass, not when it looks done.
3. **Machines enforce the style.** Hooks run the formatter, linter, strict type checks and the test suite on every change. Coverage can't drop, and functions over a set size or complexity are refused.
4. **No surprise libraries.** Only packages on the approved list in the spec can be added. A security scan checks them on every pull request.
5. **Every change gets a fresh-eyes review** by a reviewer that sees only the change and the spec. It checks for duplicated logic, dead code, leftover TODOs, unclear names and drift from the spec. A second review runs on GitHub.
6. **Small pull requests, main always passing.** Nothing is pushed straight to main. A change that breaks anything is rolled back, not patched over.
7. **A cleanup task after every stage:** dead-code scan, duplicate check, dependency audit, docs brought up to date. Mess never gets a chance to pile up.
8. **One source of truth.** ARCHITECTURE.md and a decision log are updated with every merge, so the code and the docs never disagree.
9. **A final independent audit before Gate B.** A fresh Opus session reviews the whole codebase against the spec, security and accessibility scans run, and you can hand the code to Grok or ChatGPT for an outside read.
10. **A handover pack at the end:** a plain-English guide to how everything works, the runbook, the restore drill, and how to change prices or add a category. Any developer, or a future Claude session, can pick it up.

**Models: the cheapest one that can do the job well**

Every task in the queue is assigned a model and an effort level ahead of time, and every helper agent has its model fixed in its settings file. Stronger models use up the plan's limits faster, so they're saved for work where a mistake is expensive.

| Model | Used for | Share of work (planned) |
| --- | --- | --- |
| **Haiku 5.5** | Running the twin and tests and summarizing results; formatting; dependency and dead-code scans; updating the progress file and docs | About 30% of sessions |
| **Sonnet 5.5** | Most coding: features, tests, portals, website, fixes | About 60% |
| **Opus 5.5** | Planning each stage; architecture; money and ledger code; restore replay; same-moment collisions; security and privacy review; the final audit | About 10% |
| **Fable 5.1** | Not used | 0% |

**Rules:**

- **Step up only when needed.** If a Sonnet task fails its checks twice, that one task gets one try with Opus, then drops back to Sonnet. Every step-up is logged, so the plan can be corrected if a task was mis-sized.
- **Step down by default.** Routine runs use low or medium effort. High effort is only for the Opus tasks listed above.
- **Fixed in files, not left to chance.** Models are set in each helper agent's settings and in each task's prompt, so a session never quietly defaults to the most expensive model.
- The usage measured in the first three tasks is broken out by model, so the pacing schedule accounts for the mix.

## Decisions to settle before OK go

Settled on October 27, 2026, after Grok's simulation review. Nothing is open.

| Decision | Choice |
| --- | --- |
| Profit goal | $20k–$50k **a year** |
| Homeowner details and the AI | Allowed only to the zero-retention AI; plain code scrubs obvious details first |
| Fee tiers | $25 / $75 / $199 by quote size. Ongoing services count **4 months** of their price |
| Legal check on tiered fees | Kept as a fixed introduction fee by job size, never a percentage; a flat fee per job type is a ready settings switch |
| Insurance for non-licensed businesses | Shown on the quote card as yes or no, not required |
| License checks | ROC daily lists, plus a live lookup at each pick |
| Quote window | 24 hours, counted every day |
| Free first win | Only for businesses that join in the first 90 days after launch |
| Build timing | Build in test mode alongside the hand-run test |
| Usage limits | Pause and wait for the reset (no pay-per-use overflow) |
| App host | Railway Pro if its Backups tab shows point-in-time recovery; otherwise Render |
| Legal name | Form the LLC as **Ask Hava LLC** |
| Backup person | None for now. Alerts wait for you, and safe mode pauses risky actions after 24 hours |
| Website design | Clearwater Modern (ChatGPT prototype) as the design reference. No more ChatGPT rounds; the build applies the 8 revision notes itself |
| Simple askhava.com page (Stage 0.5) | Goes up only after your separate go, once you've read the privacy policy and texting terms |
| Twilio registration | Not lead generation under Twilio's rules (Claude's reading). Register once the full site is live, with the careful wording; web-and-email fallback |
| Texting "cancel" | A text that is only "cancel" means stop all texts; jobs close with "CLOSE" plus the request number, or the button on the quote page |
| Tenant requests | Job card says "Landlord approval may be needed"; the "skip tenant requests" option is dropped |
| Spanish preference | Shown only to businesses that ticked "We speak Spanish" |
| Open legal questions | All 18 answered with the safest workable choice and a built-in fallback (changes file, section 7.1). Attorney review is optional |
| Statutory agent | A commercial registered-agent service with a Maricopa County address: no newspaper notice, home address private |

## Sources

**Claude**

- [Cloud environments and network secrets](https://code.claude.com/docs/en/cloud-environments) · [Claude Code on the web](https://code.claude.com/docs/en/claude-code-on-the-web) · [Permission modes and auto mode](https://code.claude.com/docs/en/permission-modes)
- [Subagents](https://code.claude.com/docs/en/sub-agents) · [Hooks](https://code.claude.com/docs/en/hooks) · [Skills](https://code.claude.com/docs/en/skills) · [Memory and CLAUDE.md](https://code.claude.com/docs/en/memory) · [Worktrees](https://code.claude.com/docs/en/worktrees)
- [Best practices](https://code.claude.com/docs/en/best-practices) · [Models and context windows](https://code.claude.com/docs/en/model-config) · [Costs](https://code.claude.com/docs/en/costs) · [GitHub Actions](https://code.claude.com/docs/en/github-actions) · [Routines](https://code.claude.com/docs/en/routines) · [Agent teams](https://code.claude.com/docs/en/agent-teams)
- [Claude in Chrome safety](https://support.claude.com/en/articles/12012173-using-claude-in-chrome-safely) · [Max plan](https://support.claude.com/en/articles/11049741) · [Claude Console workspaces](https://platform.claude.com/docs/en/manage-claude/workspaces) · [Zero data retention](https://platform.claude.com/docs/en/manage-claude/api-and-data-retention)

**Vendors**

- Twilio: [A2P fees](https://support.twilio.com/hc/en-us/articles/1260803965530) · [Registration types](https://support.twilio.com/hc/en-us/articles/4407882914971-Comparison-between-Starter-Low-Volume-Standard-and-Standard-registration-for-A2P-10DLC) · [Required information](https://support.twilio.com/hc/en-us/articles/13718729624091-Required-information-for-United-States-A2P-10DLC-registration) · [Test credentials](https://www.twilio.com/docs/iam/test-credentials) · [Webhook security](https://www.twilio.com/docs/usage/security)
- Stripe: [Account activation](https://docs.stripe.com/get-started/account/activate) · [Restricted keys](https://docs.stripe.com/keys/restricted-api-keys)
- Render: [Pricing](https://render.com/pricing) · [Backups and recovery](https://render.com/docs/postgresql-backups) · [Environment variables](https://render.com/docs/configure-environment-variables)
- [GitHub fine-grained tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) · [Cloudflare Registrar](https://www.cloudflare.com/application-services/products/registrar/buy-com-domains) · [Resend pricing](https://www.resend.com/pricing.md) · [Sentry DSN](https://docs.sentry.io/concepts/key-terms/dsn-explainer/) · [Lob pricing](https://help.lob.com/print-and-mail/ready-to-get-started/pricing-details)
- Arizona: [A.R.S. 29-3201 (LLC formation)](https://www.azleg.gov/ars/29/03201.htm) · [IRS EIN](https://irs.gov/businesses/small-businesses-self-employed/get-an-employer-identification-number?dpm=9044) · [Bitwarden Send](https://bitwarden.com/help/about-send)

Not verified from the vendor's own page: Google Workspace and Cloudflare .com prices, Sentry paid tiers, the Mohave County newspaper requirement and the LLC processing time. Confirm these as you sign up.
