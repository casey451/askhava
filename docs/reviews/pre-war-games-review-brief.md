This prompt tells Claude to continue building Ask Hava without interruption, but insert a mandatory competitive review between the completed build and the war games. The goal is to identify features Runhome does better, determine whether Ask Hava needs them, and address important gaps before the full-scale simulations begin.

# CLAUDE / FABLE — ASK HAVA PRE-WAR-GAMES COMPETITIVE REVIEW

## IMPORTANT: DO NOT INTERRUPT THE CURRENT BUILD

You are currently building Ask Hava. Continue executing the existing approved build plan.

I want you to add a new mandatory milestone to the project roadmap:

**PRE-WAR-GAMES COMPETITIVE GAP ANALYSIS & FEATURE READINESS REVIEW**

This milestone must occur:

1. AFTER the currently approved build is completed and its initial functionality is verified.
2. BEFORE the comprehensive AI war games, adversarial testing, simulated customers, simulated contractors, payment simulations, and large-scale stress testing begin.

Do not implement additional features now, rewrite the existing architecture, or derail current development.

At this milestone, determine whether anything important is missing that should be designed, built, or corrected before the war games.

The goal is to avoid thoroughly testing a product that is technically complete but competitively inferior or missing important customer-facing capabilities.

---

## 1. COMPETITOR REFERENCE: RUNHOME

We have been examining Runhome (runhome.com), an AI-assisted home repair concierge.

Runhome appears to offer several capabilities that may make it more appealing to homeowners than Ask Hava:

- Discovering contractors without requiring the homeowner to search for them.
- Providing preliminary AI-generated repair estimates.
- Contacting contractors and obtaining quotes.
- Helping homeowners avoid the work of contacting multiple businesses.
- Negotiating or coordinating aspects of the repair.
- Helping arrange appointments.
- Supporting homeowners throughout the repair process.
- Offering a more hands-off customer experience.
- Supporting urgent home repair situations.
- Offering a broader home-management relationship rather than a single transaction.

These are competitive capabilities to investigate, not instructions to copy Runhome or assumptions about its internal implementation.

Review Runhome's publicly available website, FAQ, terms, privacy policies, and other legitimate sources where useful. Distinguish verified claims from marketing assertions and speculation.

Do not attempt unauthorized access to Runhome's proprietary technology, private data, or systems.

## 2. PRESERVE ASK HAVA'S BUSINESS MODEL

Ask Hava is designed as a transparent, AI-driven contractor quotation and introduction marketplace.

The core model:

- Homeowners submit job requests through AI-assisted SMS or web chat.
- AI collects relevant details, photos, budget, timing, location, and job specifications.
- All qualified, opted-in businesses within the appropriate service category receive an anonymous opportunity to quote.
- Contractors submit quotes through secure links.
- Homeowners see the submitted quotes without paid ranking or manipulation.
- Homeowners select their preferred contractor.
- Selected contractors confirm and pay the applicable introduction fee.
- Only after confirmation and payment are the permitted homeowner details released.
- Phone communications use masked calling.
- Homeowners do not pay for the basic quote service.

Current fee structure:

- Under $500: $25
- $500–$1,500: $75
- Above $1,500: $199
- Ongoing services use four months of estimated value, capped at the $75 fee tier.

Preserve the integrity of this model unless your analysis identifies a substantial reason to recommend changing it.

**Do not automatically convert Ask Hava into a general contractor, construction manager, or repair company.**

## 3. REQUIRED COMPETITIVE GAP INVESTIGATION

Evaluate the following areas in detail.

### A. Contractor discovery without prior signup

One of our biggest potential weaknesses is launching in markets where no contractors have registered.

Investigate how Ask Hava could:

- Discover legitimate contractors when a homeowner submits a job.
- Identify relevant trades and service areas.
- Verify qualifications and licensing.
- Contact businesses that have never registered.
- Invite them to submit a quote for an actual homeowner request.
- Explain the introduction fee transparently before participation.
- Convert responding businesses into opted-in participants.

Evaluate permitted business directories, licensing databases, APIs, public business websites, and other legitimate discovery methods.

Address federal and state compliance, including CAN-SPAM, TCPA, state solicitation restrictions, consent, opt-outs, and communications-provider policies.

Distinguish live telephone calls, B2B email, automated texting, and AI-generated voice calls.

Do not assume cold AI calls or unsolicited automated texts are permitted. Flag legal questions requiring professional review.

Determine whether contractor discovery is essential before the national launch or should be developed even earlier.

### B. Preliminary AI repair estimates

Runhome offers homeowners preliminary price information before final contractor quotes.

Determine whether Ask Hava should provide:

- Localized estimated repair ranges.
- Expected labor and material cost ranges.
- Explanations of common price differences.
- Warnings about factors that could change the final cost.
- Guidance on whether submitted contractor prices appear unusual.

Assess data quality, hallucination risk, liability, operating cost, and whether the functionality improves customer trust.

Any estimate must be clearly identified as preliminary, not a contractor's binding offer.

### C. Post-selection repair coordination — Hava Assist

Investigate an optional second-stage feature.

After selecting a contractor, the homeowner could choose:

**"I'll take it from here"**

or

**"Hava, help me handle the rest."**

Evaluate whether Hava could assist with:

- Relaying appointment preferences.
- Contractor communication.
- Appointment confirmation and reminders.
- Arrival updates.
- Tracking job status.
- Notifying homeowners about delays.
- Follow-up communications.
- Customer satisfaction surveys.
- Confirming whether the job was completed.
- Identifying when a homeowner needs additional help.

Evaluate which tasks are realistically automatable through Rust backend services, AI agents, messaging infrastructure, and existing integrations.

**Critical legal requirement:** Arizona law can classify certain construction-management activities, including managing schedules or supervising construction projects, as regulated contractor activity.

Differentiate administrative assistance from construction management.

Research the legal boundaries, identify unresolved questions, and recommend appropriate legal review before any feature with potential licensing implications is implemented.

Do not assume that describing Hava as an AI assistant eliminates these obligations.

### D. Emergency and urgent repair requests

Ask Hava's existing quotation windows may work well for routine jobs but poorly for emergencies.

Evaluate:

- Same-day contractor availability.
- Rapid invitations to available qualified businesses.
- First acceptable quote workflows.
- Homeowner control over contractor selection.
- Emergency service categories.
- Escalations when no contractor responds.
- Realistic response-time expectations.
- Safety warnings and emergency-service referral rules.

Determine whether urgent requests need a separate workflow or whether emergencies should be excluded from the initial service.

Never imply guaranteed emergency coverage that the system cannot provide.

### E. Homeowner convenience and decision fatigue

Evaluate whether showing all quotes is always enough.

Could Ask Hava improve the experience by providing:

- Standardized quote comparisons.
- Clear explanations of scope differences.
- Availability comparisons.
- Licensing verification indicators.
- Questions the homeowner should ask.
- Simple side-by-side comparisons.
- Notifications when additional quotes arrive.
- An option to choose immediately without waiting for all contractors.

Maintain neutrality. Never introduce undisclosed paid rankings or manipulate which quote wins.

### F. Repeat customer engagement

Runhome's ongoing assistance may create repeat relationships with homeowners.

Investigate whether Ask Hava should offer:

- Home repair history.
- Previous contractor records.
- Maintenance reminders.
- Seasonal service reminders.
- Rebooking previous contractors.
- Recurring maintenance quote requests.
- A homeowner dashboard for current and previous jobs.

Determine which features provide meaningful retention rather than unnecessary complexity.

### G. Contractor economics and fee resistance

Analyze whether our introduction fee model creates friction.

Pay particular attention to:

- The $199 fee on jobs just above $1,500.
- Whether contractors understand exactly what they're purchasing.
- Homeowner cancellations.
- Invalid or duplicate requests.
- Unreachable homeowners.
- Jobs that do not proceed after introduction.
- Fee disputes and credits.
- Contractor retention and willingness to quote again.

Recommend protections that preserve profitability without making contractors reluctant to participate.

### H. Conversion and operational effectiveness

Ask Hava must optimize for successful introductions, not just completed AI conversations.

Assess whether the existing build sufficiently measures:

- Homeowner requests received.
- Eligible contractors discovered.
- Contractor invitations delivered.
- Invitations accepted.
- Quotes submitted.
- Time to first quote.
- Homeowner selections.
- Contractors who confirm and pay.
- Introductions completed.
- Repairs completed, where known.
- Failed requests and reasons.
- Revenue and variable cost per request.
- Refunds, credits, and disputes.
- Repeat customers and repeat contractors.

Recommend the smallest necessary instrumentation to understand whether the business is actually working.

---

## 4. LOOK BEYOND RUNHOME

Do not limit the analysis to the features listed above.

At the review milestone, critically examine the completed Ask Hava system from these perspectives:

1. A homeowner trying to get a repair completed with minimal effort.
2. A contractor deciding whether Ask Hava is worth paying for.
3. A competitor trying to attract those same customers.
4. A founder operating the system without a large support team.
5. A national marketplace that must expand into cities without established contractor networks.
6. A security and compliance reviewer examining privacy, liability, payments, and contractor communications.

Identify important missing functionality even if Runhome does not currently offer it.

Do not confuse more features with a better product. Prioritize improvements that materially increase conversion, trust, reliability, retention, or profitability.

## 5. REQUIRED OUTPUT

At the new milestone, create a detailed document titled:

**ASK HAVA — PRE-WAR-GAMES COMPETITIVE & FEATURE READINESS REVIEW**

Include a comparison matrix:

| Capability | Ask Hava current state | Runhome evidence | Competitive gap | Recommendation | Priority |
|---|---|---|---|---|---|
| Contractor discovery | Verify from code | Research | Analyze | Build / Improve / Defer / Reject | P0/P1/P2 |
| AI price estimates | Verify from code | Research | Analyze | Build / Improve / Defer / Reject | P0/P1/P2 |
| Repair coordination | Verify from code | Research | Analyze | Build / Improve / Defer / Reject | P0/P1/P2 |
| Emergency handling | Verify from code | Research | Analyze | Build / Improve / Defer / Reject | P0/P1/P2 |

Expand the matrix to cover every meaningful issue.

For each proposed feature, document:

- Problem being solved.
- Expected business benefit.
- Evidence supporting the recommendation.
- Existing functionality that can be reused.
- Proposed technical approach.
- Dependencies.
- Estimated development effort and ongoing cost.
- Security, privacy, legal, and operational risks.
- Acceptance criteria.
- How to test its effectiveness.
- Whether it should be developed before the war games, after the war games, or after launch.

Use three priorities:

**P0 — Required before war games:** Critical missing capabilities or defects that would invalidate testing of the intended launch product.

**P1 — Important but deferrable:** Meaningful competitive improvements that can be completed after initial testing without invalidating it.

**P2 — Future enhancements:** Interesting functionality that does not justify launch delay.

Separate legally blocked features from ordinary product priorities. Do not authorize implementation of regulated features simply because they rank P0.

## 6. UPDATE THE BUILD PLAN

Add this milestone to the existing authoritative project roadmap and tracking system.

Required sequence:

**Current approved build → Build verification → Competitive gap and feature readiness review → Approved necessary enhancements → Verification of enhancements → War games**

The review is mandatory; new feature implementation is not automatically mandatory.

Do not remove, reorder, or silently modify existing approved milestones.

Keep the current build scope intact and identify this review as a future gate.

After the review, produce a recommended feature implementation plan and decision log. Do not automatically begin major new features without my approval.

If approved features are added, ensure their implementation, tests, documentation, and security controls are complete before the war games.

The subsequent war games must be updated to test the expanded functionality, including missing contractor networks, failed outreach, emergency demand, quote abandonment, cancellations, contractor fee disputes, and attempted system abuse.

## 7. SUCCESS CRITERIA

The objective is not to copy Runhome.

The objective is to ensure Ask Hava does not launch with obvious competitive weaknesses that could have been discovered and corrected beforehand.

At the end of the review, answer:

1. What does Runhome offer that Ask Hava still lacks?
2. Which differences would actually cause customers or contractors to prefer Runhome?
3. Which of those weaknesses must be fixed before launch?
4. Can Ask Hava offer a similarly convenient experience without becoming a licensed construction manager?
5. Can Ask Hava fulfill requests in cities where no contractors have signed up?
6. Can the system scale nationally without requiring substantial human labor for every job?
7. Which proposed features improve measurable business outcomes enough to justify their cost?
8. Is the product ready for the war games, or is important functionality still missing?

**IMMEDIATE ACTION REQUEST:**

For now, only update the roadmap, register the milestone and its requirements, and confirm exactly where it will occur.

Continue the existing build as planned.

Do not begin this competitive review or start building its proposed features until the scheduled milestone is reached.

The goal is to make sure we finish building the right product before investing heavily in proving that it works.

This also gives Claude an important constraint: don't add features just because Runhome has them. Require evidence that each addition improves the business before expanding scope.
