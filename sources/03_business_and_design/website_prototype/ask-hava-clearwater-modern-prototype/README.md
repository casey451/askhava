# Ask Hava Quotes — Clearwater Modern prototype

A responsive, offline-ready (except Google Fonts), **front-end-only** demonstration of the quote feature integrated into the **Ask Hava** Lake Havasu concierge brand.

## Start here

1. Unzip the downloaded file.
2. Open `index.html` by double-clicking it in your browser. No installation or account is needed.
3. Choose **Get quotes** to walk through a request. On the sample quote screen, choose a quote and enter code **123456**.
4. Use the header links for **For businesses** and **Join free** to try business signup and the portal. Open `app/owner.html` for the owner dashboard.
5. Use **English / Español** to switch between homeowner pages and flows. Business and owner pages are currently English only.

## Quick demo links

- Home: `index.html`
- Homeowner request: `app/request.html`
- Private quote comparison: `app/quotes.html` — use code `123456`
- Homeowner portal: `app/portal.html` — use code `123456`
- Business registration: `app/business-signup.html` — license `000000` (fake)
- Business portal: `app/business-portal.html`
- Owner dashboard: `app/owner.html`
- Design system: `styleguide.html`

## Publish a free preview

**Netlify Drop**

1. Unzip the folder.
2. Visit https://app.netlify.com/drop.
3. Drag the entire unzipped **ask-hava-clearwater-modern-prototype** folder onto the upload area.
4. Share the generated preview URL. The URL is public; don't enter real customer information.

**GitHub Pages**

1. Create a new GitHub repository for the prototype.
2. Upload the contents of the unzipped folder (including `index.html`).
3. In **Settings → Pages**, choose `Deploy from a branch`, branch `main`, and directory `/ (root)`.
4. Open the Pages URL GitHub provides after publishing. Changes are tracked in the repository.

## Important limitations

- No server, authentication, SMS, email, license search, payment, database, ranking algorithm, business matching or real quote-delivery logic.
- **All sample companies, prices, deadlines, metrics and quote cards are fictional.** The Arizona ROC link points to a real search page, but no lookup is performed.
- Phone number `(928) 555-0100` is a *sample number*, not an active texting line.
- The real existing Ask Hava site is not edited. Directory and events links open https://askhava.com; the quote feature is shown as a proposed integration.
- Form entries are kept in the browser view only and are not submitted. Do not type real personal data.
- Texting consent, terms, privacy, legal claims and Spanish translations all require review before public launch.
- Running the preview on a public host does not make it a functioning service. The “Prototype — not a live service” label remains on every page.

## Files and developer handoff

- `assets/css/tokens.css`: colors, typography and core measurements
- `assets/css/styles.css`: layout and component CSS
- `assets/js/sample-data.js`: **the only source of fictional business and job data**
- `assets/js/site.js`: navigation and public-page form demo
- `assets/js/flows.js`: homeowner and business interactive steps
- `assets/img/horizon.svg`: original abstract decorative illustration
- `es/`: Spanish homeowner content and flows
- `DESIGN.md`, `CONTENT.md`, `ACCESSIBILITY.md`, `CHANGELOG.md`: documentation

No build process, framework, cookies, trackers or analytics required. Fonts load from Google when online; system fallbacks work offline.
