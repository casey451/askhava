# Spec: Quiet hours

Prefix: `QH` · Modules: `messaging` · Format and checks: [index.md](index.md)

- **QH-001** A non-urgent text is sent only when the local time is between 8:00 a.m. and 8:00 p.m. in both Arizona time and the recipient's area-code time zone (D-003). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:184`, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:121`, `BUILD_BRIEF.md:446`.
- **QH-002** Arizona time is computed as America/Phoenix, which keeps the same UTC offset all year. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:184`.
- **QH-003** The recipient's time zone is derived from the phone number's area code, and a number whose area code maps to no known zone may be texted only during hours that fall inside 8 a.m.–8 p.m. in every US time zone (D-025). Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:121`, `BUILD_BRIEF.md:537`.
- **QH-004** The only texts allowed outside the window are urgent safety texts, STOP/HELP/START replies, pick codes, and direct replies within 15 minutes of the person's own inbound text (D-003). Source: `BUILD_BRIEF.md:446`, `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:264`.
- **QH-005** Quiet hours apply to every recipient, homeowners and businesses alike, including job cards and confirm requests. Source: `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:184`.
- **QH-006** A text held by quiet hours is sent at the start of the next allowed window. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:212`.
- **QH-007** A held text is dropped if its job closed before the send time. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:212`.
- **QH-008** Retries of failed texts follow the same quiet-hours rules as first sends. Source: `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:200`.
- **QH-009** Quiet-hours decisions are covered by unit and property tests across both time zones and the daylight-saving changes of the recipient's zone. Source: `BUILD_BRIEF.md:471`.
