# Spec: Recurring services (the 4-month rule)

Prefix: `REC` · Modules: `catalog`, `payments` · Format and checks: [index.md](index.md)

- **REC-001** Each category records whether its service is recurring. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:35`.
- **REC-002** The fee basis of a recurring service is 4 times its monthly price. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`.
- **REC-003** A $150-a-month pool account has a fee basis of $600 and a fee of $75, and this case is a passing unit test. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`, `BUILD_BRIEF.md:471`.
- **REC-004** A recurring price stated per week, every two weeks, quarter or year is converted to a monthly amount (weekly × 52 ÷ 12, every two weeks × 26 ÷ 12, quarterly ÷ 3, yearly ÷ 12) before the 4-month multiplier (D-010). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`.
- **REC-005** A recurring range takes its midpoint first, then the 4-month multiplier (D-010). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:61`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`.
- **REC-006** No code, template, setting default or copy uses any multiplier other than 4 months for recurring services. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:11`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`.
- **REC-007** There is no early-cancel partial refund for recurring services. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:68`.
- **REC-008** Mutation tests on the fee code turn red when the 4-month rule is broken. Source: `BUILD_BRIEF.md:472`.
- **REC-009** Pricing copy says ongoing services count as four months, capped at $75, with the $150-a-month pool example in the $75 row (D-196). Source: Casey Oct 27, 2026: "Make all the changes you recommend" on the business-side fix plan (`docs/decisions.md:277`), `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:143`.
- **REC-010** A quote in a recurring category for a job type not in its one-time job types must state a period, one with no period is bounced back to the business to state it, and a per-visit price with a stated visit schedule is converted to a monthly amount (weekly visits at 52 / 12 a month) before the 4-month multiplier (D-037, D-159). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:61`.
- **REC-011** A weekly cleaning at $160 a visit has a fee basis of $2,773.33 and a fee of $75 under the recurring cap, and this case (worked example W33) is a passing unit test (D-196). Source: Casey Oct 27, 2026: "Make all the changes you recommend" on the business-side fix plan (`docs/decisions.md:277`), `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:60`, `BUILD_BRIEF.md:471`.
