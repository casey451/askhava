//! The business's own names and its Google and Yelp links, kept for its quote cards (BIZ-022,
//! BIZ-052, PICK-041, LIC-011, SEC-041; D-336, D-439, D-2943, D-2954).
//!
//! [`save_business_name`] screens the business name (required), the trade name or DBA and the
//! city license number a no-license business entered, then writes or replaces the one
//! `businesses.business_name` row with an audit row naming the field only (PRIV-028). A name with
//! a phone number, a link or a direction to another channel is refused with its reason and
//! nothing is stored ([`screen_public_name`]); the city license number goes through the same
//! screen (D-3037). [`check_links`] accepts only a Google Business Profile link shape
//! ([`GOOGLE_SHAPES`]) and a Yelp business page ([`YELP_SHAPES`]) with no phone number and no
//! link inside it, and only when it opens through the SEC-041 guard ([`GuardedOpener`] over
//! `adapters::api::fetch_untrusted`); only a
//! [`ReviewLinks`] it built is saved ([`save_review_links`]). The quote card reads
//! [`review_links`] (PICK-041). Editing the links in the portal is T204's; leaving is T228's.

use std::time::Duration;

use askhava_adapters::api::{FetchError, FetchPolicy, fetch_untrusted};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::about_us::{AboutUsRefusal, screen_public_name};
use crate::models::BusinessesError;

/// The longest business name or trade name (D-2943).
pub const NAME_MAX: usize = 200;
/// The longest city license number (D-2954).
pub const CITY_LICENSE_MAX: usize = 64;
/// The longest review link (BIZ-052).
pub const LINK_MAX: usize = 500;
/// SEC-041: Google's hosts, subdomains included, the guarded fetch of a Google link may visit,
/// redirects included (BIZ-052).
pub const GOOGLE_HOSTS: [&str; 5] = ["google.com", "g.page", "goo.gl", "g.co", "share.google"];
/// SEC-041: Yelp's hosts, subdomains included, the guarded fetch of a Yelp link may visit.
pub const YELP_HOSTS: [&str; 1] = ["yelp.com"];
/// BIZ-052, D-3037: the Business Profile link shapes Google gives out, as an exact host and a
/// start of the path; a prefix ending in `/` needs more after it. Search, Sites, Docs and
/// `/url?q=` links on Google's domains are refused.
pub const GOOGLE_SHAPES: [(&str, &str); 8] = [
    ("g.page", "/"),
    ("maps.app.goo.gl", "/"),
    ("g.co", "/kgs/"),
    ("share.google", "/"),
    ("www.google.com", "/maps/"),
    ("maps.google.com", "/maps/"),
    ("maps.google.com", "/?cid="),
    ("business.google.com", ""),
];
/// BIZ-052, D-3037: a Yelp business page, `/biz/<page>` on `www.yelp.com` or `yelp.com`.
pub const YELP_SHAPES: [(&str, &str); 2] = [("www.yelp.com", "/biz/"), ("yelp.com", "/biz/")];
/// D-3037: the characters a written phone number may hold between its digits.
const PHONE_SEPARATORS: [char; 6] = [' ', '-', '.', '(', ')', '+'];

/// Why a name or city license number was refused (BIZ-022); `reason` is shown beside the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRefusal {
    /// The business name is blank.
    Missing,
    /// A name is over [`NAME_MAX`] characters.
    TooLong,
    /// A name holds a phone number, a link or a direction to another channel (PICK-041, D-336).
    Screened(AboutUsRefusal),
    /// The city license number is over [`CITY_LICENSE_MAX`] characters.
    CityLicense,
    /// The city license number holds a phone number, a link or a direction to another channel
    /// (D-3037).
    CityLicenseScreened(AboutUsRefusal),
}

impl NameRefusal {
    /// The reason shown with the name again (BIZ-022).
    #[must_use]
    pub fn reason(self) -> String {
        match self {
            Self::Missing => "Enter your business name.".to_owned(),
            Self::TooLong => "Keep each name to 200 characters or fewer.".to_owned(),
            Self::Screened(r) | Self::CityLicenseScreened(r) => r.reason(),
            Self::CityLicense => {
                "Keep the city license number to 64 characters or fewer.".to_owned()
            }
        }
    }
}

/// Google Business Profile or Yelp (BIZ-052).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewSite {
    /// A Google Business Profile link.
    Google,
    /// A Yelp page link.
    Yelp,
}

impl ReviewSite {
    /// The hosts its guarded fetch may visit (SEC-041).
    #[must_use]
    pub const fn hosts(self) -> &'static [&'static str] {
        match self {
            Self::Google => &GOOGLE_HOSTS,
            Self::Yelp => &YELP_HOSTS,
        }
    }

    /// The link shapes a business may paste (BIZ-052, D-3037).
    #[must_use]
    pub const fn shapes(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Google => &GOOGLE_SHAPES,
            Self::Yelp => &YELP_SHAPES,
        }
    }

    /// SEC-041: the guarded fetch's rules for this site: its hosts, 2 MB, 10 seconds.
    #[must_use]
    pub fn policy(self) -> FetchPolicy {
        FetchPolicy::new(self.hosts(), 2 * 1024 * 1024, Duration::from_secs(10))
    }
}

/// Why a review link was refused (BIZ-052); `reason` is shown beside the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefusal {
    /// Not an `https` link of the site's own shape, holds a phone number or a link, or over
    /// [`LINK_MAX`] characters.
    WrongSite(ReviewSite),
    /// The page did not open.
    DoesNotOpen(ReviewSite),
}

impl LinkRefusal {
    /// The reason shown with the link again (BIZ-052).
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::WrongSite(ReviewSite::Google) => {
                "Paste the link to your Google Business Profile, the one its Share button gives you. It should start with https:// and go to g.page, maps.app.goo.gl, share.google, g.co/kgs or google.com/maps, with no phone number in it."
            }
            Self::WrongSite(ReviewSite::Yelp) => {
                "Paste your Yelp business page link. It should start with https://www.yelp.com/biz/ and have no phone number in it."
            }
            Self::DoesNotOpen(_) => {
                "That page didn't open for us. Check the link, or leave it blank."
            }
        }
    }
}

/// What a name or link save can fail on. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinksError {
    /// A name was refused; nothing was stored.
    #[error("business name refused")]
    Name(NameRefusal),
    /// A link was refused; nothing was stored.
    #[error("review link refused")]
    Link(LinkRefusal),
    /// `UnknownBusiness`, `Database` or `Audit`.
    #[error(transparent)]
    Businesses(#[from] BusinessesError),
}

/// The names stored for one business (D-2943, D-2954).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessNames {
    /// The business name it signed up under.
    pub business_name: String,
    /// Its trade name or DBA, if any.
    pub trade_name: Option<String>,
    /// The city license number a no-license business entered, if any (LIC-011).
    pub city_license_number: Option<String>,
}

/// `text` trimmed, or none when blank.
fn given(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

/// BIZ-022, PICK-041, D-336, D-2943, D-2954, D-3037: checks the names and the city license
/// number (length, then the same phone, link and channel screen); the trimmed values on success.
///
/// # Errors
/// The first [`NameRefusal`].
pub fn screen_names<'a>(
    name: &'a str,
    trade_name: Option<&'a str>,
    city_license: Option<&'a str>,
) -> Result<(&'a str, Option<&'a str>, Option<&'a str>), NameRefusal> {
    let name = given(Some(name)).ok_or(NameRefusal::Missing)?;
    let trade = given(trade_name);
    for n in [Some(name), trade].into_iter().flatten() {
        if n.chars().count() > NAME_MAX {
            return Err(NameRefusal::TooLong);
        }
        screen_public_name(n).map_err(NameRefusal::Screened)?;
    }
    let city = given(city_license);
    if let Some(c) = city {
        if c.chars().count() > CITY_LICENSE_MAX {
            return Err(NameRefusal::CityLicense);
        }
        screen_public_name(c).map_err(NameRefusal::CityLicenseScreened)?;
    }
    Ok((name, trade, city))
}

async fn log_field(
    tx: &mut PgConnection,
    business: Uuid,
    field: &'static str,
) -> Result<(), BusinessesError> {
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action: "businesses.field_changed",
        subject_kind: "businesses.business",
        subject_id: business,
        change: Change::Field(field),
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Audit)
}

/// BIZ-022, PICK-041, D-2943, D-2954: screens again ([`screen_names`]), then writes or replaces
/// `business`'s name row with one audit row naming the field only (PRIV-028).
///
/// # Errors
/// [`LinksError::Name`] (nothing stored), or `UnknownBusiness`, `Database`, `Audit`.
pub async fn save_business_name(
    conn: &mut PgConnection,
    business: Uuid,
    (name, trade_name): (&str, Option<&str>),
    city_license: Option<&str>,
) -> Result<(), LinksError> {
    let (name, trade, city) =
        screen_names(name, trade_name, city_license).map_err(LinksError::Name)?;
    let mut tx = sqlx::Connection::begin(&mut *conn).await.map_err(db)?;
    let saved = sqlx::query!(
        "INSERT INTO businesses.business_name \
           (business_id, business_name, trade_name, city_license_number, changed_at) \
         SELECT id, $2, $3, $4, now() FROM businesses.business WHERE id = $1 \
         ON CONFLICT (business_id) DO UPDATE SET business_name = EXCLUDED.business_name, \
           trade_name = EXCLUDED.trade_name, \
           city_license_number = EXCLUDED.city_license_number, changed_at = EXCLUDED.changed_at",
        business,
        name,
        trade,
        city,
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if saved.rows_affected() == 0 {
        return Err(BusinessesError::UnknownBusiness.into());
    }
    log_field(&mut tx, business, "business_name").await?;
    tx.commit().await.map_err(db)?;
    Ok(())
}

/// D-2943, D-2954: `business`'s stored names, or none before sign-up saved them.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn business_names(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<BusinessNames>, BusinessesError> {
    sqlx::query_as!(
        BusinessNames,
        "SELECT business_name, trade_name, city_license_number \
         FROM businesses.business_name WHERE business_id = $1",
        business,
    )
    .fetch_optional(conn)
    .await
    .map_err(db)
}

fn db(_: sqlx::Error) -> BusinessesError {
    BusinessesError::Database
}

/// SEC-041: opens a review link. The production one is [`GuardedOpener`]; tests bring a fake.
pub trait LinkOpener {
    /// Whether `url` opens under `site`'s rules.
    fn opens(&self, site: ReviewSite, url: &str) -> impl Future<Output = bool> + Send;
}

/// SEC-041: opens a link only through `adapters::api::fetch_untrusted` under
/// [`ReviewSite::policy`]: the site's hosts on https and port 443, public addresses only, at most
/// three re-checked redirects, a 2 MB cap (a page over it still opened) and a 10-second timeout.
#[derive(Debug, Clone, Copy, Default)]
pub struct GuardedOpener;

impl LinkOpener for GuardedOpener {
    async fn opens(&self, site: ReviewSite, url: &str) -> bool {
        opened(&fetch_untrusted(&site.policy(), url).await)
    }
}

/// SEC-041, D-3034: a guarded fetch's result counts as open on a normal answer or a body over the
/// cap (the page answered); every other failure is "didn't open".
pub(crate) const fn opened(fetched: &Result<Vec<u8>, FetchError>) -> bool {
    matches!(fetched, Ok(_) | Err(FetchError::TooLarge))
}

/// BIZ-052, D-3037: whether `url` is an `https` link, with no user name, password, port, backslash
/// or whitespace, of one of `site`'s [`ReviewSite::shapes`], with no phone number and no link
/// inside it, and at most [`LINK_MAX`] characters.
#[must_use]
pub fn on_site(site: ReviewSite, url: &str) -> bool {
    let Some(after) = url.strip_prefix("https://") else {
        return false;
    };
    let cut = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (host, rest) = after.split_at(cut);
    let host = host.to_ascii_lowercase();
    let clean = !host.contains(['@', ':', '\\']) && !url.chars().any(char::is_whitespace);
    let shaped = site.shapes().iter().any(|(h, prefix)| {
        host == *h
            && rest.starts_with(prefix)
            && (!prefix.ends_with('/') || rest.len() > prefix.len())
    });
    let inner = decoded(rest).to_ascii_lowercase();
    let nested = inner.contains("://") || inner.contains("http:") || inner.contains("https:");
    clean && shaped && !nested && !holds_phone(&inner) && url.chars().count() <= LINK_MAX
}

/// `text` with `%XX` escapes decoded and `+` read as a space, as a browser reads a query.
fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// D-3037: whether `text` holds a written phone number: 7 to 15 digits joined only by
/// [`PHONE_SEPARATORS`], not glued to a letter or digit on either side (so map IDs, short codes
/// and `!3d34.48` data are not numbers), and not one decimal number such as a map coordinate.
fn holds_phone(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let (mut j, mut end, mut digits) = (i, i, 0);
        while j < chars.len() && (chars[j].is_ascii_digit() || PHONE_SEPARATORS.contains(&chars[j]))
        {
            if chars[j].is_ascii_digit() {
                digits += 1;
                end = j + 1;
            }
            j += 1;
        }
        let glued = (i > 0 && chars[i - 1].is_alphanumeric())
            || chars.get(end).is_some_and(|c| c.is_alphanumeric());
        if !glued && (7..=15).contains(&digits) && !decimal(&chars[i..end]) {
            return true;
        }
        i = j;
    }
    false
}

/// A decimal number such as `114.3225123`: digits with one dot and at most three digits before it.
fn decimal(token: &[char]) -> bool {
    let dots = token.iter().filter(|c| **c == '.').count();
    let plain = token.iter().all(|c| c.is_ascii_digit() || *c == '.');
    let whole = token.iter().take_while(|c| **c != '.').count();
    dots == 1 && plain && whole <= 3
}

/// The Google and Yelp links of one business, each checked on its site and open (BIZ-052). Only
/// [`check_links`] and [`review_links`] build one, so only checked links are saved (R2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewLinks {
    google: Option<String>,
    yelp: Option<String>,
}

impl ReviewLinks {
    /// The Google Business Profile link, if given.
    #[must_use]
    pub fn google(&self) -> Option<&str> {
        self.google.as_deref()
    }

    /// The Yelp page link, if given.
    #[must_use]
    pub fn yelp(&self) -> Option<&str> {
        self.yelp.as_deref()
    }
}

async fn checked(
    opener: &impl LinkOpener,
    site: ReviewSite,
    url: Option<&str>,
) -> Result<Option<String>, LinkRefusal> {
    let Some(url) = given(url) else {
        return Ok(None);
    };
    if !on_site(site, url) {
        return Err(LinkRefusal::WrongSite(site));
    }
    if !opener.opens(site, url).await {
        return Err(LinkRefusal::DoesNotOpen(site));
    }
    Ok(Some(url.to_owned()))
}

/// BIZ-052, SEC-041, D-439: checks the optional Google and Yelp links; a blank one is none.
///
/// # Errors
/// The first [`LinkRefusal`], Google before Yelp.
pub async fn check_links(
    opener: &impl LinkOpener,
    google: Option<&str>,
    yelp: Option<&str>,
) -> Result<ReviewLinks, LinkRefusal> {
    let google = checked(opener, ReviewSite::Google, google).await?;
    let yelp = checked(opener, ReviewSite::Yelp, yelp).await?;
    Ok(ReviewLinks { google, yelp })
}

/// BIZ-052, D-439: writes or replaces `business`'s checked links with one audit row naming the
/// field only (PRIV-028).
///
/// # Errors
/// `UnknownBusiness`, `Database` or `Audit`.
pub async fn save_review_links(
    conn: &mut PgConnection,
    business: Uuid,
    links: &ReviewLinks,
) -> Result<(), BusinessesError> {
    let mut tx = sqlx::Connection::begin(&mut *conn).await.map_err(db)?;
    let saved = sqlx::query!(
        "INSERT INTO businesses.review_link (business_id, google_url, yelp_url, changed_at) \
         SELECT id, $2, $3, now() FROM businesses.business WHERE id = $1 \
         ON CONFLICT (business_id) DO UPDATE SET google_url = EXCLUDED.google_url, \
           yelp_url = EXCLUDED.yelp_url, changed_at = EXCLUDED.changed_at",
        business,
        links.google,
        links.yelp,
    )
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if saved.rows_affected() == 0 {
        return Err(BusinessesError::UnknownBusiness);
    }
    log_field(&mut tx, business, "review_links").await?;
    tx.commit().await.map_err(db)
}

/// BIZ-052, PICK-041: `business`'s saved links; none saved reads as no links.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn review_links(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<ReviewLinks, BusinessesError> {
    let row = sqlx::query!(
        "SELECT google_url, yelp_url FROM businesses.review_link WHERE business_id = $1",
        business,
    )
    .fetch_optional(conn)
    .await
    .map_err(db)?;
    Ok(row.map_or_else(ReviewLinks::default, |r| ReviewLinks {
        google: r.google_url,
        yelp: r.yelp_url,
    }))
}
