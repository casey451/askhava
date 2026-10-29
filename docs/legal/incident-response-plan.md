Draft prepared with AI research; not reviewed by a lawyer.
Version 0.2 (draft, D-492 notice method) · October 29, 2026 · Internal document

# Ask Hava Incident Response Plan

What the Ask Hava owner does when personal information may have been exposed, lost or changed without permission. It goes with the Ask Hava Written Information Security Program.

## 1. Who decides

The Ask Hava owner decides every step and keeps the incident log. There is no backup person.

## 2. Spot it

Signs include: an alert from the app (OPS-004, OPS-029), a vendor's breach notice, a report from a homeowner or business, a secret found in the wrong place, or unexpected owner-portal activity.

## 3. Contain it (first hour)

1. Start by pausing texting and charging and preserving the logs (OPS-021).
2. Rotate any exposed key or password, and sign out every session that may be affected.
3. Close any masked call lines involved.
4. Don't delete anything that shows what happened.

## 4. Work out what happened (within 3 days)

- Which data, whose, how many people, from when to when, and whether it was encrypted and the key was safe.
- Write it in the incident log with dates and times.

## 5. Tell people

**Homeowners and businesses affected.** Ask Hava tells them whenever their information was exposed in a way that could harm them, even when no law requires it. The notice says, in plain words: what happened and when; what information was involved; what Ask Hava is doing; what they can do; and how to reach Ask Hava (help@askhava.com).

**Arizona breach law** (A.R.S. 18-551 and 18-552). It covers a name together with a "specified data element" (such as a Social Security, driver license, passport or taxpayer ID number, a financial account or card number with its access code, health or biometric data), or a username or email together with its password. Ask Hava doesn't collect those for homeowners, so a leak of the contact vault is probably not a notifiable breach under this law. Ask Hava notifies anyway, within 45 days, with the law's contents: the approximate date, a description of the information, and the contacts for the three national credit bureaus and the FTC. If a covered breach ever requires notice to more than 1,000 people, also notify the three national credit bureaus, the Arizona Attorney General and the director of the Arizona Department of Homeland Security (OPS-020).

- **How.** For a breach the Arizona law covers, notice goes by letter, email or a live phone call (never a recorded message), never by text alone. A text may follow to say "check your email", with no details.
- **Substitute notice.** Only where the law allows it (the cost of notice would be over $50,000, more than 100,000 people are affected, or Ask Hava has no contact details for them): a letter to the Arizona Attorney General and a notice kept on askhava.com for at least 45 days.
- **Law-enforcement pause.** If a law enforcement agency says notice would hamper its investigation, wait until it says notice may go; the 45 days count from then.
- **Attorney General.** Use the Attorney General's online data-breach notice form (or its PDF form) and keep a copy in the incident log.
- **Processors.** Each vendor that holds Ask Hava's data must tell Ask Hava of a breach as soon as practicable; this is part of each vendor's checked terms (security program section 5).

**FTC (if the Safeguards Rule applies).** If the unencrypted information of 500 or more people was taken, file the FTC notice within 30 days of discovering it (16 C.F.R. 314.4(j)).

**Others.** Tell any vendor involved; report a crime to law enforcement when it helps; tell Stripe and Twilio if their accounts are affected.

## 6. Recover

Restore from a clean backup if needed (OPS-013 to OPS-019), switch texting and charging back on only when the cause is fixed, and watch closely for 30 days.

## 7. Learn from it

Within 2 weeks, write what went wrong, what changed, and update the security program, the threat model and any affected test.

## 8. Practice

Walk through this plan once a year with a made-up incident, at the security program's yearly review.
