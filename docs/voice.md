# Ask Hava voice guide

How Ask Hava's texts and emails sound (WORD-024, D-406). This is a style guide only. Every rule in `docs/spec/` wins over style: no hype, no pressure, no ranking, no banned words (WORD-003, WORD-019 to WORD-023), and every text starts "Ask Hava:" (WORD-008). Nobody is named, quoted or imitated in any customer copy.

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
10. **Safety texts are exempt.** Their fixed wording stays exactly as specified (INT-004, INT-031, INT-034, INT-037, INT-042).
11. **Homeowners see "request <n>"; businesses see "job <n>"** (WORD-025).

## Before and after

The "after" column is the spec text. Spanish follows once the English is settled, with native-speaker review (TXT-003).

| Message | Before | After |
|---|---|---|
| Cards sent (TXT-032) | "Ask Hava: Sent to 4 local businesses. They have until 4 p.m. Thu to quote. We'll text you when the first one is in." | "Ask Hava: Your request went to 4 local businesses that take plumbing jobs. Not every business answers. Quotes are open until 4 p.m. Thursday, Arizona time. We'll text you when the first one comes in." |
| No quotes yet (JOB-010) | "...some small or unusual jobs get no quotes... 1) Keep waiting 2) Add details 3) Close" | "Ask Hava: No quotes yet for request 1042. Small or unusual jobs can take longer. A few more details often help a business price it. Reply 1 to add details, 2 to keep waiting, or 3 to close." |
| Pick lapsed (PICK-046) | "Ask Hava: <Business> didn't confirm in time. Your other quotes: ..." | "Ask Hava: <Business> didn't confirm in time. Businesses get picks by email and sometimes miss the window. You still have other quotes: ..." |
| Backup choice used (PICK-047) | (new) | "Ask Hava: <Business> didn't confirm in time, so your backup choice, <Business 2>, has your pick now. They have until <time> to confirm." |
| Pick made (PICK-045) | "Ask Hava: <Business> gets your pick by email and has until <time> to confirm. ..." | "Ask Hava: Done. <Business> has your pick by email and has until <time> to confirm. Businesses check email at different times. Nothing else to do for now. You can still choose another quote before then." |
| Intake closed (INT-019) | "...Text us any time to restart" | "Ask Hava: We closed request 1042 because we didn't hear back. No problem. Text us any time and we'll start again." |
| Pick email body (PICK-037) | Subject only | "A homeowner picked your quote for job 1042 (AC repair, Lake Havasu City). One step left: tap Confirm and pay $75 by 2:15 p.m. Then you get their first name and address." |
| Last reminder (PICK-037) | "Ask Hava: Last reminder: confirm job <n> by <time>, or the homeowner may choose another quote." | "Ask Hava: Last reminder: job 1042 has 1 hour to confirm. After 2:15 p.m. the homeowner can choose another quote." |
| When the fee is owed (BIZ-030) | "It's a fixed introduction fee, never a share of the job, and it doesn't depend on the work being done or paid for." (kept) | Added first: "You pay one fixed fee when you confirm a pick: $25, $75 or $199 by job size. It's not a share of the job. If you call twice inside the homeowner's window and can't reach them, the refund rule gives the fee back without you asking." |
| Missed cards (BIZ-044) | "Ask Hava: Last month: 6 job requests, 2 quoted, 4 ended without a quote. ..." | "Ask Hava: Last month you got 6 job requests and quoted 2. 4 closed before you quoted. If our emails land in Spam or Promotions, this 1-minute fix moves them to your inbox: <link>. Or add a second address: <link>." |
| Another quote picked (PICK-026) | "The homeowner picked another quote for job 1042. ..." | "Job 1042 went to another business this time. 4 businesses quoted; the picked quote was lower than yours. No action needed. Thanks for quoting." |

The HELP reply (CON-018) is unchanged: carriers need its contact details, and the legal-document check uses its reply-time sentence.
