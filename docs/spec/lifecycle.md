# Spec: Job lifecycle

Prefix: `JOB` · Modules: `jobs` · Format and checks: [index.md](index.md)

- **JOB-001** Every job has exactly one status: Intake, Open, No quotes yet, Quotes ready, Awaiting confirm, Payment pending, Connected, Done, Closed, Closed early, Not covered or Cancelled. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:155`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:158`.
- **JOB-002** A status moves only along the defined arrows, and any other move is refused and logged as a bug. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:155`.
- **JOB-003** Every waiting status has a timeout. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:111`, `BUILD_BRIEF.md:136`.
- **JOB-004** Each status change takes a row lock and applies only if the job is still in the expected status. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:689`.
- **JOB-005** Every status change writes an audit row. Source: `BUILD_BRIEF.md:150`.
- **JOB-006** Timers live in the database, survive a restart, and check the current status first, doing nothing if the job has moved on. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:179`.
- **JOB-007** "Business hours" for timers are 8:00 a.m.–8:00 p.m. Arizona time, every day of the week (D-004). Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:179`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:66`.
- **JOB-008** Intake moves to Open only after consent, intake questions, the safety check, the area check and the duplicate check. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:170`.
- **JOB-009** An Open job moves to Quotes ready when its first quote arrives (D-011). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:82`.
- **JOB-010** An Open job with no quote by 6 p.m. the next day moves to No quotes yet, and the "no quotes yet" text is sent at 6 p.m. (D-030). Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:171`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:210`.
- **JOB-011** A No quotes yet job moves to Quotes ready when a quote arrives and to Closed after 5 days with none. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:172`.
- **JOB-012** A Quotes ready job with no pick for 3 days moves to Closed. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:173`.
- **JOB-013** Awaiting confirm returns to Quotes ready when the business does not confirm within 2 business hours. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:174`.
- **JOB-014** Payment pending returns to Quotes ready on a declined card, or when no working card is added within 1 business hour. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:175`.
- **JOB-015** A Connected job moves to Done or Closed on the homeowner's DONE, NO or PROBLEM reply at day 3 or day 14, and returns to Quotes ready if the business never called within 72 hours. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:176`.
- **JOB-016** A day-3 PROBLEM reply reporting a final bill well above the quote is flagged to Casey. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:62`.
- **JOB-017** A review request is sent only after Done. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:177`.
- **JOB-018** The homeowner can cancel a job from any status before Done. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:155`.
- **JOB-019** A job closes on "CLOSE" plus its number (for example "CLOSE 1042"), the close button on the quote page, or a longer cancel-intent message in English or Spanish (for example "please cancel my request", "never mind", "no longer need", "ya no"). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:159`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:112`.
- **JOB-020** The word "cancel" alone never closes a job. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:159`, `BUILD_BRIEF.md:444`.
- **JOB-021** A cancel-intent message from a homeowner with more than one open job gets "Close which?" with each job numbered plus an "all" option, and closes nothing until answered. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:112`.
- **JOB-022** A cancel-intent message from a homeowner with one open job closes that job and says so. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:215`.
- **JOB-023** A message that merely mentions "cancel" without cancel intent gets one reply explaining how to close: "To close request <number>, reply CLOSE <number>." Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:199`.
- **JOB-024** A job number that was texted to a homeowner is never reused for anyone else. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:481`.
