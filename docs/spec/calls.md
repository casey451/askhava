# Spec: Masked calls

Prefix: `CALL` · Modules: `calls` · Format and checks: [index.md](index.md)

- **CALL-001** Each connected job gets a masked number from a pool. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:110`.
- **CALL-002** Calls between the homeowner's phone and the business's linked phones on that number are connected through the masked line, and neither side sees the other's number. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:189`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:225`.
- **CALL-003** A call from a phone that isn't linked must enter the 4-digit job PIN, and a wrong PIN is refused. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:189`.
- **CALL-004** When a caller has more than one active job on the same number, the call is routed only by PIN. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:110`.
- **CALL-005** After 2 bad PINs from a phone linked to a business with a confirmed, paid pick, the line reads back the homeowner's first name and street for the job it would connect and connects only after the caller confirms; unlinked callers hear no details (D-026). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:110`, `BUILD_BRIEF.md:151`.
- **CALL-006** Calls are never recorded. Source: `BUILD_BRIEF.md:141`.
- **CALL-007** A call counts as a connection only if a person answered and it lasted 45 seconds or more. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:189`.
- **CALL-008** Each finished call logs who called, whether a person or a machine answered, and its length. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:189`.
- **CALL-009** A masked call line closes when the job is cancelled, when the business no-shows or after a complaint, and otherwise after 30 days. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:189`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:205`.
- **CALL-010** When a job is cancelled after connection, the call line is closed, the business is told, and it gets no no-show strike. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:615`.
- **CALL-011** A blocked call from an unlinked staff phone hears the PIN prompt, the "Reply LINK <PIN>" text goes to the business's signed-up main phone naming the last 4 digits of the blocked number, and if nobody has got through 45 minutes later the business is asked to call from its main phone (D-078). Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:216`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:258`.
- **CALL-012** Staff phones are linked by the owner of the business account. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:374`.
- **CALL-013** The masked line works with 711 relay services. Source: `BUILD_BRIEF.md:408`.
- **CALL-014** The identity voice call opens with "This is Ask Hava LLC" and gives a callback number. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:182`, `BUILD_BRIEF.md:449`.
- **CALL-015** The voice code call speaks slowly, repeats the code twice and offers "press 1 to hear it again". Source: `BUILD_BRIEF.md:417`.
- **CALL-016** Businesses never text homeowners through Ask Hava's number. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:169`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:249`.
- **CALL-017** Masked numbers come from a fixed pool sized in settings, the app never buys numbers itself, and a job gets a number no other active job of the same homeowner or business is using when one is free, otherwise sharing one with PIN routing and an alert to Casey (D-077). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:110`, `BUILD_BRIEF.md:61`.
