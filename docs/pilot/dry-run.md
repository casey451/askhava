# Dry run: rehearse the hand-run test with fake businesses and fake homeowners (D-469)

**For:** Casey. **When:** after setup (`hand-run-test.md` section 4), before the first real post. **Time:** about 90 minutes, once. **Cost:** $0. Stripe test mode moves no real money.

The dry run proves every step works before a real person is involved: the forms, the sheet, the job-card emails, quotes coming back, the pick, a payment link, a refusal, a job nobody covers, a homeowner who doesn't pick, and the scorecard. Nothing here is real, and nothing is posted.

You need two inboxes: the **test inbox** you made for Ask Hava, and **your personal Gmail**. Your personal Gmail plays every fake business and fake homeowner, using "+" addresses. Gmail delivers `you+anything@gmail.com` to `you@gmail.com`, so one inbox can be six different people. Write down which "+" address is which person.

| Plays | Address (your personal Gmail with a "+" tag) |
|---|---|
| Test Pool Co A | `you+poola@gmail.com` |
| Test Pool Co B | `you+poolb@gmail.com` |
| Test Handyman A | `you+handya@gmail.com` |
| Test Handyman B | `you+handyb@gmail.com` |
| Test Homeowner 1 | `you+home1@gmail.com` |
| Test Homeowner 2 | `you+home2@gmail.com` |

---

## Part 1. The tracker (10 minutes)

1. Upload `docs/pilot/hand-run-tracker.xlsx` to Google Drive and open it with Google Sheets.
2. Open **Scorecard**. It's already filled from fake data: you should see Go, Change and Stop lines in green, amber and red. Change one number on **Requests** (for example, a request's source class from Seeded to Organic) and watch the Organic line change. That proves the formulas came through the import.
3. **Clear the fake data** (Read me tab): on Businesses, Requests, Quotes, Picks, Nudges and Posts, select the blue columns from row 2 down and press Delete. On Weekly, clear the two blue minutes columns.
4. On **Settings**, for the dry run only: set the start date to today's Monday, set the pushed trades to `pool_cleaning` and `handyman`, and set **First paid week to 1**, so the payment step runs today. You'll set it back in Part 5.

## Part 2. Forms and Stripe (25 minutes)

1. Build the two Google Forms exactly as `hand-run-test.md` section 4 lists them, and link each to this sheet (Form > Responses > Link to Sheets > Select existing spreadsheet). Each adds a "Form Responses" tab. You copy each new response into Businesses or Requests by hand; that's when you give it an ID, a source class and "routable".
2. In Stripe, stay in **test mode** (the "Test mode" switch at the top). Make three payment links: $25, $75 and $199, each named "Ask Hava introduction fee". Test mode works before the account is activated.
3. Write the three test links on the Rules tab, marked TEST. You'll make the real ones once the live account is ready (playbook section 9).

## Part 3. Six scenarios (45 minutes)

Run them in order. After each one, check the line in the "Check" column. Start a timer for each, and put the minutes in the request's "Your minutes" column.

| # | Scenario | What you do | Check |
|---|---|---|---|
| 1 | **Business sign-ups** | Submit the business form four times, as Pool Co A, Pool Co B, Handyman A and Handyman B, each from its "+" address. Pool Co B enters a made-up ROC class; the others choose "I don't hold a contractor license". Answer "Where did you hear" differently each time. Copy all four into Businesses (B01 to B04). | Four rows; "Signed up on their own" is Y; "Active" shows FALSE until they quote. |
| 2 | **A pool job with two quotes, a pick and a payment** | As Homeowner 1, submit the request form: weekly pool cleaning, "How did you hear": Facebook. Copy it in as R001, source Organic, routable Y. Send her the card (template C), then email the card to Pool Co A and B in BCC (template D) and note "Card sent". As each pool company, reply with a price (for example $120 a month, so $480 for 4 months). Log both quotes, then email her both quotes (template E). As Homeowner 1, pick Pool Co B. Log the pick. Send Pool Co B template K with the $25 test link. As Pool Co B, pay with Stripe's test card `4242 4242 4242 4242`, any future date, any CVC. Mark Paid = Y. Only then send her details (template F without the "worth it" question). | Quotes: both valid (Y). Picks: fee shows $25, "Paid week" TRUE, "First payment from this business" 1. Stripe test dashboard shows the payment. The details went out only after payment. |
| 3 | **A handyman job: one too big, one late** | As Homeowner 2, request a handyman job. Card both handymen. As Handyman A, quote $1,200. As Handyman B, quote $300, but enter its received time as 26 hours after "Card sent". | The $1,200 quote isn't passed on, and you send Handyman A the reason (playbook section 5, step 5). The $300 quote shows "Valid in 24h" = N. The request shows 0 valid quotes. |
| 4 | **Nobody covers it** | As Homeowner 1, request a plumbing repair. | You reply with template A. Routable = N, no-pick reason "Not covered". It appears on the scorecard's request counts, but not in the pushed lines. |
| 5 | **No pick** | Run another pool job to quotes, then don't pick. When the 24 hours would be up, send template J. Close it with a no-pick reason. | Picked = FALSE; the no-pick reason is filled in; "Picks, among requests with 2+ quotes" drops. |
| 6 | **A refusal, then the backup** | Run a third pool job to a pick of Pool Co A. Send template K. As Pool Co A, don't pay. Mark Paid = Refused, offer the pick to her next choice, and that business pays with the test card. Then refund one test payment in Stripe and mark Refunded = Y. | "Refusals at the real fee" shows 1. The refund shows in Stripe test mode. |

Then add one nudge on Nudges (for example "Reminded Pool Co A to quote"), one row on Posts, and your minutes on Weekly. Check that the **Scorecard** moved for each.

## Part 4. What to look for

- Did any template need changing to make sense? Fix it in your copy and tell Claude, so the playbook gets the same fix.
- Did any step take much longer than you expected? A real request should take about 30 minutes or less once you're practiced.
- Did every email land in the right inbox, and not in Spam? If one went to Spam, mark it "Not spam" and add the test inbox to your contacts. Real businesses will have the same problem, which is why the playbook's welcome email asks them to check.
- Did anything ever ask you to paste a homeowner's details into an AI tool? It shouldn't. If you want AI help wording a card, remove the name and address first.

## Part 5. Reset for the real test (5 minutes)

1. Clear every row you made in the dry run (the blue columns, as in Part 1), and the Form Responses tabs.
2. On **Settings**: set the real start date (the Monday of week 1), your real pushed trades, and **First paid week back to 4**.
3. Fill in the Rules tab's date and any changes you decided during the dry run.
4. Delete the TEST payment links from the Rules tab. Make the real ones only once the live Stripe account is ready.
5. Delete the dry-run emails from both inboxes.

You're ready for the first real post.
