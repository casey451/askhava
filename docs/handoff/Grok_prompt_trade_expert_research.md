# Research task: Build the "trade expert" knowledge base for a home-services quote service

You're researching for **Ask Hava**, a quote-request service in **Lake Havasu City, Arizona**. Homeowners text, chat or send photos describing a problem. Our assistant asks a few smart questions, then sends a short, anonymous job card to local businesses in that trade. The businesses quote in their own words.

**The goal:** our assistant should think like an experienced, licensed pro in every trade. A good plumber hears "water under my kitchen sink" and already knows what to ask. Look at a photo and they know what they're seeing. They know what they'd need to give a price without driving out. Your research becomes that expertise.

Ask Hava is **not a contractor**. It never diagnoses, prices, recommends a business or gives repair advice. The assistant's expertise is only for asking the right questions, recognizing emergencies, picking the right trade and writing a clear job card. Keep that line in every answer you write.

---

## Part 1: Every reason someone might reach out

List every kind of request a Lake Havasu homeowner, renter, landlord, vacation-home owner or boat/RV owner might send, including messy, vague and off-topic ones. Group them by trade. Include the local specifics:

- **The basics:**
  - extreme summer heat (115°F+)
  - hard water and scale
  - pools and spas
  - evaporative (swamp) coolers
  - rooftop package AC units
  - monsoon storms and dust
  - scorpions
  - desert landscaping and irrigation
  - snowbird and vacation homes left empty for months
  - short-term rentals
  - manufactured homes
- **Water and vehicles:**
  - boats, docks and boat lifts
  - RVs and RV pads
  - golf carts
  - lake-related work
- **HOAs and gated communities.**
- **Things that are emergencies** (gas smell, carbon monoxide, sparking or burning smell, flooding, no AC in extreme heat for elderly or medical households, sewage backup). Also the things that sound like emergencies but usually aren't, such as a chirping smoke detector.
- **Things we shouldn't take or should redirect:**
  - work that needs a permit and an engineer
  - commercial jobs
  - utility-side problems (the meter, the main line on the street side)
  - HOA-owned common areas
  - insurance claims
  - jobs needing a licensed specialist that a handyman can't legally do in Arizona

## Part 2: For each trade, the expert intake

Cover at least these trades, and add any you find missing:

- **Systems:** heating & cooling (repair, replacement, maintenance, swamp coolers, mini-splits), plumbing, water heaters, water softeners and reverse osmosis, electrical, solar, generators and EV chargers
- **Pools and outdoors:** pool & spa repair, weekly pool cleaning, landscaping and irrigation, tree trimming, pavers and concrete
- **Interior and exterior work:** handyman, roofing, painting, drywall, flooring, cabinets and countertops, windows and doors, garage doors, appliance repair
- **Pests:** pest control (general, scorpions, termites, pre-emergent weed control)
- **Cleaning:** house cleaning, carpet cleaning, window cleaning, gutter and solar-panel cleaning, junk removal
- **Moving:** moving help
- **Vehicles and water:** boat and dock services, RV services, golf cart repair
- **Home safety and inspection:** home inspection, locksmith, security and alarm systems, septic pumping

For **each trade**, give:

1. **Plain-language triggers:** the words people actually use ("AC blowing warm," "toilet keeps running," "breaker keeps tripping"), including common misspellings and Spanish phrases. Map each to the right job type.
2. **The 3–6 questions a pro asks first,** in the order that most changes the price. Prefer multiple-choice answers a person can tap, include branching ("ask only if…"), and use plain words (6th–8th grade reading level).
3. **What a pro needs to quote without a visit,** and which jobs almost always need a site visit no matter what.
4. **Photos:** what to ask the homeowner to photograph ("the label on the side of the water heater," "the whole outdoor unit, plus the data plate"). Then say what a pro looks for in each photo: model and serial plates, age clues, corrosion, water stains, burn marks, breaker brands known for problems, pool equipment types, roof material. Also list photo red flags that mean "stop and give safety instructions."
5. **Safety and emergency signals** in that trade, and the exact immediate safety step (for example, "leave the house and call 911 and the gas company from outside"). Only cite official sources for these.
6. **Arizona license routing:**
   - Which Arizona Registrar of Contractors (ROC) license classes (R- and CR- classes, with A.A.C. R4-9-102/103/104 citations) or other agency licenses can legally do each job type.
   - What an unlicensed handyman can legally do under Arizona's handyman exemption (A.R.S. 32-1121(A)(14)): under $1,000, no permit, and not part of a larger job.
7. **Seasonal and local notes** for Lake Havasu, such as AC demand in summer, pool green-ups, monsoon roof leaks and snowbird season.
8. **A sample finished job card:** 2–4 short lines a busy business owner can read in 10 seconds and quote from. No name, phone, exact address or other personal details, only the general area (for example, "Lake Havasu City, north side").

## Part 3: The photo-reading playbook

Explain how a vision AI should look at a homeowner's photo for each trade:

- what it can reliably identify (equipment type, brand from a label, visible damage)
- what it can't reliably judge (hidden causes, whether something is up to code)
- how to describe what it sees in neutral words that aren't a diagnosis ("visible rust at the base of the tank" rather than "your tank is failing")
- privacy steps before a photo goes to any business: blur faces, house numbers, mail, license plates and documents
- what to do with blurry, irrelevant or inappropriate photos

## Part 4: Quality checks

Give 10 realistic, messy test messages per trade, each with the correct trade, job type, emergency yes/no and the ideal first question. These become our test set. Include vague ones ("something's wrong with my AC"), mixed ones ("AC is out and there's water on the floor"), wrong-trade ones and Spanish ones.

---

## Output format

- **Structured data:** one YAML file per trade, using these fields: trade, job_types, triggers_en, triggers_es, questions (id, text, options, ask_if, changes_price: yes/no), site_visit_usually_needed, photos_to_request, photo_cues, photo_red_flags, emergency_signals and safety_step, license_classes with citations, handyman_ok, local_notes, sample_card, test_messages.
- **Summary:** a short narrative per trade.
- **Sources:** cite them (ROC rules, Arizona statutes, manufacturer guides, industry associations, utility and fire-department safety pages). Label anything that's your judgment rather than sourced as **[judgment]**.
- **Wording rules:** never use "vetted," "pre-screened," "leads," "exclusive," "best" or "recommended." Never write anything that sounds like Ask Hava diagnosing, guaranteeing, pricing or supervising work.
