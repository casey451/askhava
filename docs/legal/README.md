Draft prepared with AI research; not reviewed by a lawyer.
Version 0.1 (draft) · October 28, 2026

# Ask Hava legal document drafts

Plain-English drafts for the Ask Hava owner's review (TERMS-015, TERMS-028, D-389). They were written from the spec in `docs/spec/` and the AI legal-research memos (outside this repository, in `/home/claude/hhq/outputs/legal/`). No lawyer has reviewed them. None is in effect or published; publishing waits for the owner's go (WEB-013, WEB-017).

| Document | For | Spec it follows |
|---|---|---|
| [homeowner-terms.md](homeowner-terms.md) | Homeowners and tenants | TERMS-001 to TERMS-004, WORD-002, WEB-026, PICK-016 |
| [business-terms.md](business-terms.md) | Businesses | TERMS-005 to TERMS-007, TERMS-019, TERMS-022, TERMS-024, TERMS-026, TERMS-027, FEE-001 to FEE-042 |
| [privacy-policy.md](privacy-policy.md) | Everyone | TERMS-008 to TERMS-010, TERMS-020, TERMS-025, AI-018, PRIV-007 to PRIV-014, CON-035 |
| [sms-terms.md](sms-terms.md) | Homeowners who text | CON-002 to CON-035, TERMS-009 |
| [business-email-notices.md](business-email-notices.md) | Businesses | CON-019, BIZ-036 to BIZ-048, CARD-022, PICK-037, SEC-025, SEC-028 |
| [accessibility-statement.md](accessibility-statement.md) | Everyone | A11Y-001, A11Y-020, A11Y-024 |
| [information-security-program.md](information-security-program.md) | Internal | SEC and PRIV rules; Memo 2 V.B |
| [incident-response-plan.md](incident-response-plan.md) | Internal | OPS-020, OPS-021; Memo 2 III.C and V.B |

**Check.** `python3 scripts/check_legal_docs.py` fails unless every document here starts with the draft note and a version line, every key phrase listed in the script appears both in its spec requirement and, word for word, in its document, and no document uses the banned wording listed in the script (the WORD-020 to WORD-023 lists, ranking and vetting claims, the category that is never advertised (WEB-010), a personal name or the brand shortened (WORD-021), and, in business documents, license or data-access framing of the fee, TERMS-027). The homeowner terms' arbitration section must stay marked OPTIONAL and OFF.

**Before publishing,** the owner fills in the bracketed items (the mailing address, the effective date, the business data retention days) and decides the open items in `docs/for-casey-review.md` (arbitration, among others). Spanish versions are written after these are settled and go to a native speaker (TERMS-018).
