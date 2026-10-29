# The copy panel

Every word a homeowner or business reads is checked by a panel of eight writers' methods before it ships (WORD-029, D-474). The AI-smell scan (`docs/ai_smell.yaml`) catches copy that *sounds* machine-made. This panel catches copy that is *weak*: vague, about us instead of them, impossible to picture, or easy for any competitor to copy. The two checks run together.

The panel is a method, not an imitation. Nobody on it is named, quoted or imitated in customer copy (`docs/voice.md`), and the spec's rules always win over the panel: no hype, no pressure, no ranking, the banned words, and the legal and safety wording fixed word for word.

## 1. Who is on the panel, and what each one checks

Each method below is taken from the source listed; the quotes are short and attributed.

| Seat | Writer | The test they bring | Source |
|---|---|---|---|
| 1 | **Harry Dry** (Marketing Examples; YouTube talk "Learn Great Copywriting in 76 Minutes") | Three questions for every claim: "Can you visualize it?", "Can you falsify it?", "Can nobody else say this?" Three no's means rewrite. | Demand Curve lesson "Fundamental rules of good copy"; impactthinking.beehiiv.com summary of the talk |
| 2 | **Joanna Wiebe** (Copyhackers; Copy School; YouTube tutorials) | Voice of the customer: build the message from the words customers actually use, and listen for "unexpected ways of expressing something". | Wistia interview, "Use Your Words"; copyhackers.com/about-us |
| 3 | **Eugene Schwartz** (*Breakthrough Advertising*, 1966) | Stages of awareness: talk to the reader where they are. Someone who knows their problem doesn't need it explained; they need to see the way out. | Copyblogger and Indie Hackers summaries of the book |
| 4 | **David Ogilvy** (*Ogilvy on Advertising*; *The Unpublished David Ogilvy*) | "Concrete figures must be substituted for atmospheric claims." "Superlatives ... have no place in a serious advertisement." "Use short words, short sentences and short paragraphs." "Write the way you talk." Make it "crystal clear what you want the recipient to do." | *The Unpublished David Ogilvy*, via antoinebuteau.com |
| 5 | **Joseph Sugarman** (*The Adweek Copywriting Handbook*) | "The purpose of the first sentence ... is to get you to read the second sentence." Raise the reader's objection yourself, then resolve it. | samuelthomasdavies.com summary of the book |
| 6 | **Eddie Shleyner** (VeryGoodCopy; *Very Good Copy*) | Specificity: concrete nouns, strong verbs, one clear picture instead of jargon; empathy for the reader's actual struggle. | Shortform summary of *Very Good Copy* |
| 7 | **Alex Cattoni** (Copy Posse; YouTube, about 375,000 subscribers) | Trust first: "trust is currency"; copy should "feel personal, sound authentic, and create serious trust", with no manipulative or over-hyped tactics. | copyposse.com/about |
| 8 | **Ann Handley** (*Everybody Writes*) | "Write to one person." "Every sentence must pass reader relevance filter." "Read aloud." | autymate.com summary of the book |

Plain-language research sits behind seats 4 and 8: Nielsen Norman Group found 79% of users scan web pages rather than read word by word, and plain-English guidance built on it targets sentences of 20 words or fewer, active voice, and grade 9 or lower for general readers (Newcastle University design system, "Readability", citing NN/g and Flesch–Kincaid). Ask Hava sets its own bar a step lower: grade 8 for homeowners.

## 2. Who Ask Hava is talking to

- **Homeowners** arrive *problem-aware* (something needs doing) and usually *solution-aware* (they know they could call around, ask a neighbour or use a big lead site). They don't need the problem explained. They need to see, fast, why this is easier and safer than calling around. (Seat 3.)
- **Businesses** arrive *product-unaware* and suspicious: lead sites have burned many of them. They need the fee, what it buys and what it doesn't, in numbers, before anything else. (Seats 3 and 7.)

## 3. The scorecard

The reviewer scores each line or block 0, 1 or 2 on the ten tests, with a one-line reason and, for anything under 2, a rewrite.

| # | Test | 2 means | Seat |
|---|---|---|---|
| 1 | **Picture** | The reader can see it: a person, a job, a number, a moment. | 1, 6 |
| 2 | **Proof** | It could be checked true or false; no superlatives, no "great", "fast" or "easy" without the fact behind it. | 1, 4 |
| 3 | **Only us** | A competitor (a lead site, a single contractor, a directory) couldn't honestly say it. Required for headlines and section titles; other lines may score 1. | 1 |
| 4 | **Their words** | Sounds like the reader talking ("calling around", "get a few quotes", "nobody called back"), not like us describing our process. Phrases come from `docs/copy/voice-of-customer.md` once it has entries. | 2 |
| 5 | **Right stage** | Starts where the reader already is (section 2) and moves them one step. | 3 |
| 6 | **Pulls forward** | The first line makes you read the next; each block ends with one clear next step. | 4, 5 |
| 7 | **Worry answered** | Names a real doubt the reader has at this point (cost, privacy, scams, being sold to, wasted time) and answers it with a fact. | 5, 7 |
| 8 | **One reader** | Written to one person as "you", warm and plain, as it would sound read aloud. | 7, 8 |
| 9 | **Plain** | Short words; sentences of 20 words or fewer; active voice; grade 8 or lower for homeowners, 9 for businesses (`scripts/check_copy.py` measures this). | 4, 8 |
| 10 | **Earns its place** | Cut it and the reader would miss something. | 4, 8 |

**Passing:** no 0 on tests 2, 7 (where a worry exists) or 9; nothing below 1 elsewhere; headlines and section titles also score 2 on tests 1, 3 and 4 or are rewritten. Fixed legal and safety wording (the requirements listed under `exempt_requirements` in `docs/ai_smell.yaml`, WORD-002, WEB-021, WEB-022) is never rewritten; the panel only checks that the copy around it reads well.

## 4. The reviewer persona (the prompt the build uses)

The task template's end routine (step 3) runs this as a reviewer subagent that sees only the finished copy, the audience and the page's job, never the task or the writer's reasoning:

> You are the Ask Hava copy panel: eight writers' methods applied in one read. You review words a Lake Havasu City homeowner or local business owner will read. You get the copy, who reads it (homeowner or business) and what the page or message must get them to do.
>
> Read it aloud as that person would, then score every line or block 0, 1 or 2 on the ten tests in `docs/copy/copy-panel.md` section 3, each with a one-line reason. For every score under 2, write the line again so it passes, using only facts in the copy you were given or in `docs/spec/` (never invent a number, promise or feature) and phrases from `docs/copy/voice-of-customer.md` where one fits.
>
> Hold the rules that beat good copy: no hype, pressure, ranking or banned words (`docs/ai_smell.yaml`, `docs/spec/wording.md`); never rewrite wording the spec fixes for legal or safety reasons; homeowners see "request <n>", businesses "job <n>"; nobody is named or imitated. Say plainly if the copy is good; don't invent problems to have something to say.
>
> End with: PASS or REWRITE, and the full corrected copy if REWRITE.

The writer applies every rewrite or answers why not in the hand-in report. A rewrite that a later spec rule forbids loses to the rule.

## 5. Voice of the customer

`docs/copy/voice-of-customer.md` collects the exact phrases real homeowners and business owners use, starting with the hand-run test: how they describe the job, what they were worried about, why they picked a quote, why they left. Each line is the phrase only, with the date and whether it came from a homeowner or a business: no names, numbers, addresses or anything else that identifies a person (BUILD_BRIEF.md section 2). The panel's test 4 draws on it, and the re-plan pass after the test rewrites the home page from it.
