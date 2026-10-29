# Hand-run test: prove it by hand before the build (D-455)

**For:** Casey. **When:** now, before you reply `approved to build` at Gate P. **How long:** 4 to 6 weeks. **Cost:** $0. **Your time:** about 6 to 8 hours a week.

The build is held at Gate P until this test answers three questions with real people:
1. Will homeowners in Havasu send a job to an unnamed local service?
2. Will local businesses quote an anonymous job card sent by email, and fast?
3. Will the business that gets picked say the introduction was worth the fee?

Nothing here is charged, nothing is texted, and nothing is automated. You do every step by hand with a free web form, one email inbox and one spreadsheet. The same rules as the real service apply (section 2).

---

## 1. The test at a glance

| Week | What you do | What you count |
|---|---|---|
| 0 (one evening) | Set up the forms, the inbox and the sheet (section 3). Join the Facebook groups and message each admin. | Groups joined, admins who said yes |
| 1 to 2 | Post 3 to 5 times a week. Sign up businesses. Ask people you know for real jobs. | Business sign-ups by group, requests by source |
| 2 to 6 | Send job cards, pass quotes to homeowners, record picks. Talk to every picked business. | The scorecard in section 8 |
| End | Fill in the scorecard. Decide: go to Gate P, change the plan first, or stop. | Go or stop |

All service businesses are open (D-459). Your posts push one or two trades that fit the season (section 6), so at least those get 3 or more quotes a job.

---

## 2. Rules that hold even by hand

- **Ask Hava is not a contractor.** You never bid, set a price, schedule, supervise or promise anyone's work. Every message says the homeowner hires the business directly.
- **No ranking.** Homeowners get every quote in the order it came in, in the same format, with no "top pick". Never write "best", "vetted", "trusted", "recommended", "top-rated", "leads", "referral", "exclusive", "concierge" or "connect you with".
- **Homeowners never pay.** Businesses pay nothing during the test either.
- **No marketing texts and no calls to businesses.** Everything goes by email and the web form. You may visit a few businesses in person (section 7). Phone recruiting needs a state seller registration first (A.R.S. 44-1272, D-374), so don't call.
- **Say who you are.** Every post and every reply in a group starts with "I run Ask Hava" (FTC endorsement rules, pack file 10 section D2). One real profile. No second accounts and no anonymous posts.
- **Post by hand.** Facebook bans automated posting to groups. AI may draft a post; you paste and post it yourself.
- **Never pay a group admin** to let you post. Meta bans selling permission to post.
- **Homeowner details stay in the sheet.** Never paste a homeowner's name, phone, email, address or photos into an AI tool (`BUILD_BRIEF.md:66`). Remove them first if you want AI help wording a job card.
- **The homeowner's details go only to the business she picks:** her first name, the job address and her email, once she says yes. Never her phone number (`BUILD_BRIEF.md:151`).
- **Licensed trades go only to licensed businesses.** Check the ROC number by hand (section 3, step 5). Pest control: the business tells you its Department of Agriculture license number, and the card says it isn't independently checked (D-417).

---

## 3. Setup (one evening, about 3 hours)

1. **An inbox.** Make a free Gmail address for the test, for example askhava.quotes@gmail.com. Use it for everything below. Turn on 2-step verification, and turn off Gmail's smart features and Gemini in Settings, since this inbox will hold homeowners' details.
2. **The request form (homeowners).** A free Google Form named "Ask Hava: get quotes". Questions:
   - What do you need done? (paragraph)
   - Which kind of work is it? (dropdown of the enabled trades in `docs/catalog/categories.yaml`, which leaves out junk removal and garage doors, plus "Not sure")
   - How soon? (Today, it's urgent / This week / This month / Just getting prices)
   - Is the job in Lake Havasu City? (Yes / Another town)
   - Your first name; the job address; your email (no phone: during the test everything goes by email)
   - "Photos help businesses price it. Reply to our email with photos if you have them." (text only, no upload)
   - A hidden source field (step 6)
   - The note at the top, word for word: "Ask Hava is not a contractor. We send a description of your job, without your name or contact details, to local businesses that do that kind of work. You get every quote and pick one if you like. Free for you. Only the business you pick gets your first name and the job address."
3. **The sign-up form (businesses).** A second Google Form, "Ask Hava: get job requests". Business name; owner's name; email; which kinds of work you want requests for (checkboxes); ROC license number, or "I don't hold a contractor license"; city business license number; Google and Yelp page links (optional); "Do you take after-hours emergency jobs?"; "Where did you hear about Ask Hava?" (a Facebook group, which one / another business / a friend / I met Casey / other); and this note: "Free during the test. We email you anonymous job cards. You quote if you want to. If the homeowner picks you, we send you her first name and the job address. Later, the picked business will pay a fixed introduction fee by job size (currently planned at $25, $75 or $199). There is no fee during the test."
4. **The sheet.** One Google Sheet, shared with nobody, with tabs: Requests, Businesses, Cards, Quotes, Picks, Posts, Weekly. Link both forms to it.
5. **License check.** For each business with an ROC number, look it up on the Arizona ROC contractor search and note the class and "active" in the Businesses tab. Only send it cards its class covers (`docs/catalog/license_map.yaml`). No number and a licensed trade: no cards for that trade.
6. **Tracking links.** In each form, add a short-answer question "Source" and make a pre-filled link per place you post (for example `oo` for Havasu Orchids & Onions, `cp` for the Community Page). Shorten each link. This is how you know which group brings sign-ups and requests.

---

## 4. Running a job by hand

| Step | What you do | Template |
|---|---|---|
| 1. Request comes in | Read it within a few hours. If it's in a trade with no businesses yet, reply with template A and count it in the Requests tab as "not covered". | A |
| 2. Questions | If key facts are missing, ask up to 3 follow-up questions by email, the way a good receptionist would (sizes, age of the unit, access, photos). The question bank in `docs/catalog/question_bank.yaml` lists the right ones per trade. | B |
| 3. Job card | Write the anonymous card. No name, no street number, no email or phone. Send it to the homeowner first: "Here's your request as businesses will see it. Reply if anything is wrong. It goes out in 30 minutes." | C |
| 4. Send it | Email the card to every signed-up business whose kinds of work and license cover it, in BCC. Note the time sent. | D |
| 5. Quotes | Businesses reply with a price (fixed, range, or "visit needed, $X to look"), when they can come, and a short note. Note the time each one arrives. An unlicensed handyman may quote only jobs under $1,000 in total (labor and materials) that need no permit and involve no gas, hardwired smoke alarms or fire sprinklers (LIC-019 to LIC-025); a handyman quote of $1,000 or more isn't passed on, and you tell the business why. | |
| 6. Quotes to the homeowner | Email her every quote in arrival order, same format, with each business's license number and its Google and Yelp links. Send the first one as soon as it arrives, then the rest together. | E |
| 7. Pick | She replies with the business she picks and says yes to sharing her first name, the job address and her email. | |
| 8. Introduction | Email the picked business her first name, the job address and what she shared. Ask one question: "If this weren't free, would this introduction have been worth $<fee> to you? Yes or no, and why." Tell the others the job is no longer open, and whether the picked quote was lower, about the same or higher. | F, G |
| 9. Day 3 | Ask her: "Did <business> get in touch? Reply DONE, NO or PROBLEM." Only after a DONE, ask whether you may share what she says on askhava.com with her first name and town (no reward, ask everyone who replies DONE). | H, I |

**The quote clock.** A business has 24 hours from when you sent the card. If no quotes come in 24 hours, tell the homeowner and offer to keep waiting or close it.

**Templates** (plain, short, signed "Ask Hava"):

- **A, not covered:** "Thanks for sending this. We don't have a <trade> business on Ask Hava yet, so we can't get you quotes for this one. We've noted it so we can add businesses. If it can't wait, it's fine to call a company you know."
- **B, questions:** "Thanks. To get you useful quotes, a few quick questions: 1) ... 2) ... 3) ... Photos help too, if you have them. Reply to this email."
- **C, card to the homeowner:** "Here's request <n> as businesses will see it. Your name and contact details aren't on it. Reply if anything's wrong; otherwise it goes out in 30 minutes. <card>"
- **D, card to businesses:** subject "Ask Hava job <n>: <trade>, <job type>, Lake Havasu City". Body: the card, then "Reply to this email with your price, when you could do it and a short note. Sent to <N> businesses. Free during the test. Ask Hava is not a contractor; the homeowner hires you directly. Reply STOP to get no more job cards."
- **E, quotes:** "Your quotes for request <n> (<k> of <N> businesses quoted so far). In the order they came in: [business, license number, price, when, note, Google link, Yelp link] ... Reply with the one you pick, and say yes to sharing your first name, the job address and your email with that business. We never share your phone number. Ask Hava is not a contractor; you hire the business you pick directly."
- **F, introduction:** "You were picked for job <n>. Homeowner: <first name>, <address>. Email: <her email>. Please get in touch within 24 hours. One question: if this weren't free, would this introduction have been worth $<fee> to you?"
- **G, not picked:** "Job <n> is no longer open. The homeowner picked another quote; it was <lower / about the same / higher> than yours. Thanks for quoting."
- **H, day 3:** "Did <business> get in touch about request <n>? Reply DONE, NO or PROBLEM."
- **I, after DONE:** "Thanks. May we share a line from your reply on askhava.com, with your first name and town? Yes or no is fine."

The fee for a job, if it weren't free, follows the real table (FEE-001): $25 when the quote is under $500, $75 from $500 up to $1,500, and $199 over $1,500. A range quote counts at its midpoint, a visit-fee quote pays $25, and ongoing services count 4 months of the monthly price, capped at $75 (FEE-039).

---

## 5. Getting homeowners (the demand plan)

Target: **at least 6 requests a week by week 3, and 10 by week 6.** That is the pace the real pilot needs. The build only becomes necessary at about 5 a day.

1. **People you know first.** Ask friends, family and neighbors for real jobs they need quotes on. Aim for the first 10 to 20 requests this way. Koerner: first customers come from people you know (pack file 09, sections 11 and 12).
2. **"Who do you recommend" threads.** About 3 replies a week, in neighborhood groups, only where someone is asking: "I run Ask Hava. If you want a few quotes to compare, you can send the job here: <link>. You pick, you hire them directly, it's free for you." Never name or praise a single business.
3. **Repeat askers.** Ask 3 to 5 property managers, vacation-rental hosts or realtors you know to send their repair jobs during the test. They need quotes again and again.
4. **One homeowner tip post a week** in neighborhood groups that allow it, with no link if the rules say so: "I run Ask Hava. Three things to ask before a water-heater swap: ..."
5. **Track the source** of every request with the tracking links (section 3, step 6).

---

## 6. Getting businesses (the supply plan)

Target: **3 or more businesses in each trade you're pushing by week 2,** and 15 to 25 businesses in total by week 6. Most businesses should find Ask Hava and sign up on their own; you visit only a few (section 7).

- **Where:** local business-owner and trades groups first. A guest on Koerner's show found business groups far better than neighborhood groups for reaching businesses, where residents dislike ads (pack file 09, section 11). Neighborhood groups are for homeowners.
- **How often:** 3 to 5 posts a week, each reworded, in different groups, on different days. Never paste the same text twice. That stays well under Facebook's spam limits (pack file 10, section A1).
- **Ask first:** message each group's admin once before posting: what Ask Hava is, that it's free, and whether a business post is allowed or which thread to use.
- **Lead with pay-only-when-picked,** and say "by email" plainly.

**Post templates** (rotate and reword; plain words, no hype):

- **Intro (business groups):** "I run Ask Hava, a small local service here in Havasu. Homeowners send us a job; we email local businesses an anonymous job card, and you quote if you want. You pay only if the homeowner picks you, and it's free during our test. Plumbers, pool techs, handymen and more: sign up here <link>."
- **Real requests (once you have them):** "I run Ask Hava. This week we had 4 pool-cleaning requests and only 1 pool business signed up. If you do weekly pool service in Havasu, the cards are free during our test: <link>." Counts must be true. Never claim how often a card turns into a job.
- **Question post:** "I run Ask Hava. Pool techs: would you quote a green-pool cleanup from three photos, or do you always need to see it first?" Save every answer as a future post.
- **Results (week 4 on):** "I run Ask Hava. First month: <x> job cards sent, <y> businesses quoted, median first quote in <z> hours." True numbers only.

**Seasonal push:** in a winter start, push one or two of handyman, heating and weekly pool cleaning; in a spring or summer start, one or two of AC repair, pool repair and pest control. All trades stay open; the push only decides what the posts talk about.

**Backup channel:** post the same way on Nextdoor if you have an account there, so one Facebook restriction doesn't stop the test (single point of failure, review section "Operations").

---

## 7. The few visits, and the first-week welcome

- **When to visit:** a trade has requests but fewer than 3 businesses; or a business commented with interest but didn't sign up. Visit 2 or 3 shops with one question ("Would you quote a job like this from a description and photos?"), then point them to the sign-up form. In-person events: the Professionals Club of Havasu and the Chamber's building-industry expo (pack file 10, section C), as a guest.
- **Welcome every new business yourself** with a short personal email within a day, and send them any open card in their trade at once. Check in after their first card: "Did the card have what you needed to quote?"
- **When a business stops quoting,** ask once why, and note the answer.

---

## 8. Scorecard and the decision

Fill in the Weekly tab every Sunday (about 15 minutes), including your minutes that week split into recruiting and running jobs.

| Measure | Go to Gate P | Change the plan first | Stop or rethink |
|---|---|---|---|
| Requests started per week (by week 4) | 6 or more | 3 to 5 | under 3 |
| Requests with 1 or more quotes within 24 hours | 70% or more | 50% to 69% | under 50% |
| Median time to first quote (after the card is sent) | 4 hours or less | over 4, up to 12 hours | over 12 hours |
| Picks per request with quotes | 40% or more | 25% to 39% | under 25% |
| Picked businesses saying "worth the fee: yes" | 60% or more | 40% to 59% | under 40% |
| Businesses in the pushed trades | 3 or more each | 2 | 1 or fewer |
| Business sign-ups that came without a visit | 70% or more | 50% to 69% | under 50% |
| Your hours per week | 8 or fewer | 9 to 12 | over 12 |

**Go:** most lines in the first column, none in the last: reply `approved to build` at Gate P after the re-plan pass in section 10.
**Change the plan first:** the test showed what to fix. Claude makes those changes in one pass, then you decide Gate P.
**Stop or rethink:** don't build yet. The numbers say the model needs to change.

The scorecard decides nothing on its own. You decide.

---

## 9. What the test answers for the build

Ask these in the first 5 to 10 business conversations and note the answers (the open interview items in `docs/for-casey-review.md`, section "Pending validation", C1 to C13):

- **C1:** Is the phone or email on your ROC record current? (The build's identity check sends codes there.)
- **C2:** "Sent to 6 businesses": at what number would you stop quoting?
- **C5 and C10:** How fast do you usually see an email? How many minutes does a quote like this take?
- **C6:** Is $25 fair for a small job, and would $35 still be? For ongoing service, is $75 fair for a new client?
- **C13:** Does a 6-month price lock for early joiners matter to you?
- **C11:** Would hearing that the picked quote was lower make you quote again, or quit?
- **Price test:** half of the picked businesses get "worth $25?", the other half "worth $35?" for small jobs (inferred from pack file 03, H6 and 02, section 8).

Also note, from homeowners: how many questions were too many, whether they sent photos, and whether anyone asked who runs Ask Hava.

---

## 10. After the test

1. **One re-plan pass** (Claude, one pull request): apply what the test showed; make the scope cut held from the review (postpone team members, the monthly fee limit, the live license lookup, the full restore replay and the features that launch switched off); and set the fees from the "worth the fee" answers. Until then no new requirements are added (D-455).
2. **Company setup starts** once the test hits its quote line (requests with 1 or more quotes within 24 hours at 70% or more), so the LLC, EIN, Twilio registration and Stripe are ready by Gate C (`docs/PREBUILD.md` section 8).
3. **Gate P:** reply `approved to build` on GitHub issue #24, then resume the build runner.
