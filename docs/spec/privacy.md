# Spec: Privacy, the contact vault and retention

Prefix: `PRIV` · Modules: `privacy` · Format and checks: [index.md](index.md)

- **PRIV-001** Homeowner names, phone numbers, emails, addresses, on-site contacts and original photos are stored only in the contact vault, and every other record refers to them by ID. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:81`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:85`.
- **PRIV-002** Vault fields are encrypted field by field with AES-GCM under a key kept separate from the database credentials. Source: `BUILD_BRIEF.md:348`.
- **PRIV-003** Every vault read is audit-logged. Source: `BUILD_BRIEF.md:349`.
- **PRIV-004** Only the messaging and release code can read the vault. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:85`.
- **PRIV-005** No homeowner contact detail leaves the vault except to the business on a confirmed, paid pick (or a recorded free win). Source: `BUILD_BRIEF.md:151`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`.
- **PRIV-006** Job cards, logs, dashboards and AI prompts never contain vault fields. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:81`.
- **PRIV-007** Homeowner contact details are purged 180 days after the job closes, unless a dispute or complaint is open (D-013). Source: `BUILD_BRIEF.md:354`.
- **PRIV-008** Job text is kept with contact details removed, for 3 years (D-013). Source: `BUILD_BRIEF.md:355`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:437`.
- **PRIV-009** Photos are deleted 30 days after the job closes (D-013). Source: `BUILD_BRIEF.md:358`.
- **PRIV-010** Ledger and payment records, and the audit log, are kept 7 years. Source: `BUILD_BRIEF.md:357`, `BUILD_BRIEF.md:359`.
- **PRIV-011** "Delete my data" removes the vault entry and photos and replaces names with "deleted", while ledger and tax records stay without personal details. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:443`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:385`.
- **PRIV-012** The app never collects SSNs, driver's licenses or tax ID images, and uses EIN or ROC number instead. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:223`, `BUILD_BRIEF.md:361`.
- **PRIV-013** Photos are optional, with size and type limits checked from the file bytes rather than the name. Source: `BUILD_BRIEF.md:338`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:222`.
- **PRIV-014** Every photo is re-encoded with EXIF and GPS data stripped, stored privately, and served only through short-lived signed links. Source: `BUILD_BRIEF.md:339`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:222`.
- **PRIV-015** The photo upload says "avoid photos of people". Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:222`.
- **PRIV-016** Logs carry request IDs only and never phone numbers. Source: `BUILD_BRIEF.md:373`.
- **PRIV-017** Sentry runs with send_default_pii=False plus a scrubber. Source: `BUILD_BRIEF.md:372`.
- **PRIV-018** Backups are encrypted. Source: `BUILD_BRIEF.md:349`.
- **PRIV-019** Code, tests, fixtures, logs and screenshots use fake data only, and the PII scanner fails the build on real-looking personal data. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:263`, `BUILD_BRIEF.md:298`.
- **PRIV-020** The homeowner's phone number is never shared, and the homeowner's first name and address go only to the business they pick. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:191`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`.
- **PRIV-021** A homeowner can review and change their data (properties, on-site contacts, language, texts on or off) in the homeowner portal. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:385`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:218`.
- **PRIV-022** Privacy-module code keeps at least 95% branch coverage. Source: `BUILD_BRIEF.md:481`.
