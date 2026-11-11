# Owner actions

**Status:** open (D-2740). Only Casey or counsel can close these; a Claude session can prepare each one. P0 = before real customer data, live charges or any production privilege (Gate C). P1 = establish now, evidence before any certification work. Record each closure as an `operating` row in `evidence-register.md` named `OA-<n>` (pointer or opaque label only, never the secret or account detail). Gate C (T145) does not open until P0 items 1 to 14 each have that row (D-3080, checked by `scripts/gate_c_ready.py`, T395).

## P0 — Casey

1. Approve or amend `isms-scope.md`, the risk method in its section 5, and the SoA exclusions in `soa.md` (ISMS-01, 03, 04).
2. Decide the solo-operator question: accept it in writing (OPS-002, R-11) or name a deputy with restricted break-glass access, and test whichever you choose (OPS-ISMS-03, ISMS-05).
3. Set recovery targets (RTO, RPO) so T139's restore drill can be judged against them (OPS-ISMS-03, A-01).
4. Turn on and record a phishing-resistant or app second factor with offline recovery on GitHub, the host, Stripe, Twilio, storage, email, monitoring, AI and the domain registrar (TECH-02, O-1).
5. List which accounts and tokens each robot holds on GitHub, the host, Stripe and Twilio, and decide when N-01 trims the runner job (TECH-01, R-37).
6. Confirm with Stripe or the acquirer which PCI self-assessment applies to the Confirm page before declaring any (TECH-10).
7. Collect each provider's security report or page and data-processing terms, and fill the "last review" and "agreement" columns of the inventory, keeping the documents in a private store (OPS-ISMS-02).
8. Engage an outside security tester for the test site before launch; AI audits and the ZAP scan do not replace it (TECH-15).
9. Set the device baseline: screen lock, updates, disk encryption, password manager, hardware keys and their offline recovery (OPS-ISMS-05).
10. Record the AI provider's current terms or the zero-retention agreement before AI runs on live data (AI-008, TECH-13).

## P0 — counsel

11. Determine which privacy, breach-notice, messaging and financial-safeguards laws apply in each state Ask Hava will serve; the WISP's section 4a is an AI reading, not a legal finding (OPS-ISMS-01, ISMS-02).
12. Review the incident plan's notice steps, deadlines and templates before anyone relies on them (OPS-ISMS-04).
13. Review texting consent and any outbound message to people who are not yet customers (TECH-11).
14. Set response deadlines and identity checks for data-access and deletion requests (TECH-19).

## P1 — Casey

15. Approve the objectives and targets in `security-objectives.md` and check the first monthly scorecard (ISMS-06).
16. Sign or reject each residual risk in `docs/threat-model.md` section 6, starting with the business sign-in exceptions R2 and R18 (TECH-18, OPS-ISMS-07).
17. Run one incident tabletop with a made-up compromise and record the follow-ups (OPS-ISMS-06, F-05).
18. Hold and record the first management review (ISMS-09).
19. Name an independent internal auditor for after launch (ISMS-08).
20. Write the access rule for anyone new: training, confidentiality agreement, least access, and a tested removal (OPS-ISMS-08, ISMS-10).
21. Approve the TECH-22 exclusion (no custom detection system for now).
