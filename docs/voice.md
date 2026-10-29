# Ask Hava voice guide

How Ask Hava's texts and emails sound (WORD-024, D-406). This is a style guide only. Every rule in `docs/spec/` wins over style: no hype, no pressure, no ranking, no banned words (WORD-003, WORD-019 to WORD-023), and every text starts "Ask Hava:" (WORD-008). Nobody is named, quoted or imitated in any customer copy.

## The register

A sharp, experienced receptionist (D-447): warm, brief, sure of the next step, never salesy. She already knows what a plumber will ask, so she asks it once, plainly, and moves on. She never guesses what's wrong, never prices anything and never sounds like a form.

## The rules

1. **Short sentences. One idea each.** Aim for 15 words or fewer.
2. **Name the situation first, in one plain clause.** About the situation, never the person: "No quotes yet for request 1042."
3. **Take the blame off.** Missed windows and slow quotes are normal: "Businesses get picks by email and sometimes miss the window." Never imply anyone did wrong.
4. **One clear next step, at the end.** One reply word or one link. With options, number them and put the most useful first.
5. **Say the outcome, not the system.** "Your request went to 4 businesses", not "Your job card was dispatched by our platform."
6. **Answer the worry before it's asked.** Cost ("Free for you"), privacy ("Only the business you pick gets your address"), scams ("We never ask for a card or a code by phone").
7. **Calm, never urgent.** Deadlines are plain facts ("Quotes are open until 4 p.m. Thursday, Arizona time."). Never "hurry", "don't miss out", "act now", "limited time" or "last chance" (or "apúrese", "no se lo pierda"). The WORD-003 scan checks this list.
8. **Two registers.** Homeowners: gentle and plain. Business owners: shorter, direct, with numbers (fee, time left, job size).
9. **Numbers only when true.** Counts come from the system, never from estimates (CARD-019).
10. **The 911 text is exempt.** Its fixed wording stays exactly as specified (INT-004, INT-037). There is no other safety, heat or health text (D-436).
11. **Homeowners see "request <n>"; businesses see "job <n>"** (WORD-025).

## Before and after

The "after" column is the spec text. Spanish follows once the English is settled, with native-speaker review (TXT-003).

| Message | Before | After |
|---|---|---|
| Coverage (INT-017) | (none) | "Ask Hava: Got it. We'll send this to the 5 pool cleaning businesses on Ask Hava." |
| Not covered (INT-017) | "...isn't a category we cover..." | "Ask Hava: We don't cover tree trimming jobs yet. We've noted your request so we can add businesses. Text us any time." |
| Cards sent (TXT-032) | "Ask Hava: Your request went to 4 local businesses that take plumbing jobs. ... Quotes are open until 4 p.m. Thursday, Arizona time. ..." | "Ask Hava: Request 1042 is out to 4 plumbing businesses. Not every business answers. We'll text you when the first quote comes in." |
| First quote (TXT-016) | "...site visit needed, $89 trip fee, earliest visit today, evening. More may come..." | "Ask Hava: Your first quote for request 1042 is in, and more may come. Free for you. The business you pick pays Ask Hava a set introduction fee. Can't open links? Reply LIST to get your quotes by text. See them here: <link> Terms (18+): askhava.com/terms" |
| No quotes yet (JOB-010) | "...Small or unusual jobs can take longer. A few more details often help..." | "Ask Hava: No quotes yet for request 1042. A few more details or a photo often help a business price it. Reply 1 to add details, 2 to keep waiting, or 3 to close." |
| Pick lapsed (PICK-046) | "Ask Hava: <Business> didn't confirm in time. Businesses get picks by email and sometimes miss the window. ..." | "Ask Hava: <Business> couldn't take it this time. You still have other quotes: ..." |
| Backup choice used (PICK-047) | "...didn't confirm in time, so your backup choice... They have until <time> to confirm." | "Ask Hava: <Business> couldn't take it, so your backup, <Business 2>, has your pick now. We'll text you when they accept." |
| Pick made (PICK-045) | "Ask Hava: Done. <Business> has your pick by email and has until <time> to confirm. ..." | "Ask Hava: Done. <Business> has your pick. We'll text you when they accept. You can still pick another quote on your page." |
| Intake closed (INT-019) | "...Text us any time to restart" | "Ask Hava: We closed request 1042 because we didn't hear back. No problem. Text us any time and we'll start again." |
| Pick email body (PICK-037) | Subject only | "A homeowner picked your quote for job 1042 (AC repair, Lake Havasu City). One step left: tap Confirm and pay $75 by 2:15 p.m. Then you get their first name and address." |
| Last reminder (PICK-037) | "Ask Hava: Last reminder: confirm job <n> by <time>, or the homeowner may choose another quote." | "Ask Hava: Last reminder: job 1042 has 1 hour to confirm. After 2:15 p.m. the homeowner can choose another quote." |
| When the fee is owed (BIZ-030) | "It's a fixed introduction fee, never a share of the job, and it doesn't depend on the work being done or paid for." (kept) | Added first: "You pay one fixed fee when you confirm a pick: $25, $75 or $199 by job size. It's not a share of the job. If you call twice inside the homeowner's window and can't reach them, the refund rule gives the fee back without you asking." |
| Missed cards (BIZ-044) | "Ask Hava: Last month: 6 job requests, 2 quoted, 4 ended without a quote. ..." | "Ask Hava: Last month you got 6 job requests and quoted 2. 4 closed before you quoted. If our emails land in Spam or Promotions, this 1-minute fix moves them to your inbox: <link>. Or add a second address: <link>." |
| Another quote picked (PICK-026) | "The homeowner picked another quote for job 1042. ..." | "Ask Hava: Job 1042 went to another business this time. 4 businesses quoted; the picked quote was lower than yours. No action needed. Thanks for quoting." |

The HELP reply (CON-018) is unchanged: carriers need its contact details, and the legal-document check uses its reply-time sentence.

## AI smell (WORD-027, WORD-028, D-452)

AI smell is anything that makes a reader think "a machine wrote this." It costs trust, and trust is the product. Two checks catch it during the build: `scripts/check_ai_smell.py` fails on the words and patterns in `docs/ai_smell.yaml`, and a cold reader reads every new line aloud before hand-in.

**The standard:** would a local person say it out loud to a neighbour? If not, rewrite it.

**What gives it away:**
- Brochure words: seamless, elevate, effortless, tailored, curated, peace of mind, hassle-free, journey, solution.
- Polish instead of facts: "We ask what a seasoned pro would ask." Say what happens: "We'll ask a few questions."
- Slogans in threes: "Text us the job. Local businesses quote. You pick." Write one plain sentence.
- Set-ups: "Not just X, but Y." "It's not X, it's Y." "The best part:" "Here's the thing." Just say it.
- Em dashes, exclamation marks, emoji, rhetorical openers ("Need a plumber?").
- Everything the same length and rhythm. Real people mix short and long sentences.
- Saying the same reassurance twice on one screen.

**How the cold read works:** the reviewer sees only the finished words (not the task, not the reasoning) and reads them as the homeowner or business owner would. For each line it asks: does this sound like a person? Is anything here a claim we can't back up? Is there a plainer way to say it? Flagged lines are rewritten. When a word or pattern would have caught the line, it is logged in `docs/decisions.md` and added to `docs/ai_smell.yaml`, so the scan catches it next time.

| Before (AI smell) | After |
|---|---|
| "Text us the job. Local businesses quote. You pick." | "Get local quotes by text." |
| "We ask what a seasoned pro would ask, then send your request to local businesses that do the work. Their quotes line up side by side, and you pick one." | "Text us what needs fixing. We'll ask a few questions, send it to local businesses that do that kind of work, and text you when their quotes come in. Then you look them over and pick one." |
| "Their quotes come in one format, so they line up." | "Each quote lists the same details, so they're easy to compare." |
| "More trades are coming. Text us any job and we'll tell you right away if we cover it." | "We're adding more. Text us any job and we'll tell you if we cover it." |

## Good copy, not only clean copy (WORD-029, D-474)

Clean copy can still be weak. `docs/copy/copy-panel.md` is the second check: eight writers' methods in a ten-test scorecard (can you picture it, can it be checked, could a competitor say it, does it use the customer's own words, and so on), run by a reviewer that sees only the finished copy. `scripts/check_copy.py` measures sentence length and reading grade. The approved site wording lives in `docs/copy/site-copy.md`, and real customers' phrases in `docs/copy/voice-of-customer.md`.
