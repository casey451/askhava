//! The brand as data (D-1932; WORD-001, WORD-008, WORD-021, TXT-006, FEE-049).
//!
//! The brand may change before launch, so no customer-visible code names it: everything reads
//! [`brand()`]. The one source is `app/core/data/brand.json`, compiled in with `include_str!` and
//! read by Python through `scripts/brand.py` with the same fields, tokens and checks.
//! [`Brand::embedded`] parses and checks that file; each binary checks and installs it at start-up
//! ([`install`], through `askhava_server::start_up`), and a test may install a fake one parsed
//! with [`Brand::parse`]. [`Brand::fill`] replaces the brand tokens in a text template.
//! `tests/docs/test_brand_literals.py` fails on a new brand literal under `app/`.

use std::sync::OnceLock;

use serde::Deserialize;
use thiserror::Error;

/// The brand file, compiled in (D-1932).
const EMBEDDED: &str = include_str!("../data/brand.json");

/// The fixed part of a card-statement descriptor after the prefix: `<prefix>* JOB ` (FEE-049).
const STATEMENT_JOINER: &str = "* JOB ";

/// The digits a job number may take on a card statement (FEE-049).
const JOB_NUMBER_DIGITS: usize = 7;

/// The card statement's descriptor limit (FEE-049; `payments`' `DESCRIPTOR_MAX`).
const STATEMENT_MAX: usize = 22;

/// The longest short name a text may open with (WORD-008, D-1932).
const SHORT_NAME_MAX: usize = 20;

/// The installed brand, set once per process by [`install`].
static INSTALLED: OnceLock<Brand> = OnceLock::new();

/// The embedded brand, parsed once when nothing was installed.
static FALLBACK: OnceLock<Brand> = OnceLock::new();

/// What can go wrong with the brand file, its installation or a template's tokens.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BrandError {
    /// The file is not valid JSON of the brand's shape (an unknown or missing field included).
    #[error("the brand file does not parse: {0}")]
    Parse(String),
    /// A field breaks its rule; the field is named, never its value.
    #[error("the brand field {0} is not valid")]
    Field(&'static str),
    /// [`install`] was called a second time in this process.
    #[error("a brand is already installed in this process")]
    AlreadyInstalled,
    /// A template holds a `{brand...}` token that is not one of the five.
    #[error("unknown brand token {{{0}}}")]
    UnknownToken(String),
}

/// The brand's Spanish names (WORD-001, D-1932); today the same words as the English ones. Only a
/// checked [`Brand`] holds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandEs {
    display_name: String,
    business_name: String,
    short_name: String,
    legal_name: String,
}

/// The brand: names, domain, sender, card-statement prefix and the words never used alone
/// (D-1932). Built only by [`Brand::parse`] or [`Brand::embedded`], so every value passed its
/// check: it has no `Default` and no public `Deserialize` (the file is read into [`RawBrand`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brand {
    display_name: String,
    business_name: String,
    short_name: String,
    legal_name: String,
    spoken_legal_name: String,
    domain: String,
    email_from_name: String,
    email_from_address: String,
    statement_prefix: String,
    forbidden_alone: Vec<String>,
    es: BrandEs,
}

/// The brand file as read, before its check; private, so no unchecked [`Brand`] can be built.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBrand {
    display_name: String,
    business_name: String,
    short_name: String,
    legal_name: String,
    spoken_legal_name: String,
    domain: String,
    email_from_name: String,
    email_from_address: String,
    statement_prefix: String,
    forbidden_alone: Vec<String>,
    es: RawEs,
}

/// The `es` object as read (see [`RawBrand`]).
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEs {
    display_name: String,
    business_name: String,
    short_name: String,
    legal_name: String,
}

impl RawBrand {
    /// Moves the read fields into a [`Brand`]; private, called by [`Brand::parse`] before the check
    /// and by [`brand()`] for the empty fallback.
    fn into_brand(self) -> Brand {
        Brand {
            display_name: self.display_name,
            business_name: self.business_name,
            short_name: self.short_name,
            legal_name: self.legal_name,
            spoken_legal_name: self.spoken_legal_name,
            domain: self.domain,
            email_from_name: self.email_from_name,
            email_from_address: self.email_from_address,
            statement_prefix: self.statement_prefix,
            forbidden_alone: self.forbidden_alone,
            es: BrandEs {
                display_name: self.es.display_name,
                business_name: self.es.business_name,
                short_name: self.es.short_name,
                legal_name: self.es.legal_name,
            },
        }
    }
}

impl Brand {
    /// Parses and checks the compiled-in `app/core/data/brand.json` (D-1932).
    ///
    /// # Errors
    ///
    /// [`BrandError::Parse`] or [`BrandError::Field`] naming the bad field.
    pub fn embedded() -> Result<Self, BrandError> {
        Self::parse(EMBEDDED)
    }

    /// Parses and checks a brand file's text: every field non-empty, the short name, legal name,
    /// domain, sender and statement prefix rules (WORD-001, WORD-008, TXT-006, FEE-049).
    ///
    /// # Errors
    ///
    /// [`BrandError::Parse`] for bad JSON or an unknown field, [`BrandError::Field`] naming the
    /// first field that breaks its rule.
    pub fn parse(json: &str) -> Result<Self, BrandError> {
        let raw: RawBrand =
            serde_json::from_str(json).map_err(|err| BrandError::Parse(err.to_string()))?;
        let brand = raw.into_brand();
        brand.check()?;
        Ok(brand)
    }

    fn check(&self) -> Result<(), BrandError> {
        self.check_filled()?;
        check_short_name(&self.short_name, "short_name")?;
        check_short_name(&self.es.short_name, "es.short_name")?;
        if !self.legal_name.contains(&self.display_name) {
            return Err(BrandError::Field("legal_name"));
        }
        if !self.es.legal_name.contains(&self.es.display_name) {
            return Err(BrandError::Field("es.legal_name"));
        }
        if !is_host_name(&self.domain) {
            return Err(BrandError::Field("domain"));
        }
        if !sender_on_domain(&self.email_from_address, &self.domain) {
            return Err(BrandError::Field("email_from_address"));
        }
        if !statement_prefix_fits(&self.statement_prefix) {
            return Err(BrandError::Field("statement_prefix"));
        }
        Ok(())
    }

    fn check_filled(&self) -> Result<(), BrandError> {
        let fields = [
            ("display_name", &self.display_name),
            ("business_name", &self.business_name),
            ("short_name", &self.short_name),
            ("legal_name", &self.legal_name),
            ("spoken_legal_name", &self.spoken_legal_name),
            ("domain", &self.domain),
            ("email_from_name", &self.email_from_name),
            ("email_from_address", &self.email_from_address),
            ("statement_prefix", &self.statement_prefix),
            ("es.display_name", &self.es.display_name),
            ("es.business_name", &self.es.business_name),
            ("es.short_name", &self.es.short_name),
            ("es.legal_name", &self.es.legal_name),
        ];
        if let Some((name, _)) = fields.iter().find(|(_, value)| value.trim().is_empty()) {
            return Err(BrandError::Field(name));
        }
        if self.forbidden_alone.is_empty()
            || self.forbidden_alone.iter().any(|w| w.trim().is_empty())
        {
            return Err(BrandError::Field("forbidden_alone"));
        }
        Ok(())
    }

    /// Replaces `{brand}`, `{brand_business}`, `{brand_short}`, `{brand_legal}` and `{domain}` in
    /// `text`, with the Spanish names when `spanish`; every other `{name}` placeholder (`{job}`)
    /// is left alone (WORD-001, WORD-008, TXT-006, D-1932). A token is `{`, then no brace, then
    /// `}`, the leftmost first, as in `scripts/brand.py`: `{{brand}}` fills to `{<name>}` and a
    /// stray `{` stays as it is.
    ///
    /// # Errors
    ///
    /// [`BrandError::UnknownToken`] for a `{brand...}` token that is not one of those.
    pub fn fill(&self, text: &str, spanish: bool) -> Result<String, BrandError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let inner = &rest[start + 1..];
            let Some(end) = inner
                .find(['{', '}'])
                .filter(|&end| inner[end..].starts_with('}'))
            else {
                out.push('{');
                rest = inner;
                continue;
            };
            let name = &inner[..end];
            match self.token(name, spanish) {
                Some(value) => out.push_str(value),
                None if name.starts_with("brand") => {
                    return Err(BrandError::UnknownToken(name.to_owned()));
                }
                None => out.push_str(&rest[start..=start + end + 1]),
            }
            rest = &inner[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn token(&self, name: &str, spanish: bool) -> Option<&str> {
        let value = match (name, spanish) {
            ("brand", false) => &self.display_name,
            ("brand", true) => &self.es.display_name,
            ("brand_business", false) => &self.business_name,
            ("brand_business", true) => &self.es.business_name,
            ("brand_short", false) => &self.short_name,
            ("brand_short", true) => &self.es.short_name,
            ("brand_legal", false) => &self.legal_name,
            ("brand_legal", true) => &self.es.legal_name,
            ("domain", _) => &self.domain,
            _ => return None,
        };
        Some(value)
    }

    /// The name on every page and document (WORD-001).
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The business side's name, "<display name> for Business" today (WORD-001).
    #[must_use]
    pub fn business_name(&self) -> &str {
        &self.business_name
    }

    /// The name every text opens with, as `<short_name>:` (WORD-008, D-064).
    #[must_use]
    pub fn short_name(&self) -> &str {
        &self.short_name
    }

    /// The legal entity's name (WORD-001, FEE-015).
    #[must_use]
    pub fn legal_name(&self) -> &str {
        &self.legal_name
    }

    /// The legal name as the voice line says it (D-1932).
    #[must_use]
    pub fn spoken_legal_name(&self) -> &str {
        &self.spoken_legal_name
    }

    /// The public domain every link points at (TXT-006, SEC-025).
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// The email sender's display name (D-1932).
    #[must_use]
    pub fn email_from_name(&self) -> &str {
        &self.email_from_name
    }

    /// The email sender's address, on the domain or a subdomain of it (SEC-025).
    #[must_use]
    pub fn email_from_address(&self) -> &str {
        &self.email_from_address
    }

    /// The card-statement descriptor prefix the Stripe account carries (FEE-049).
    #[must_use]
    pub fn statement_prefix(&self) -> &str {
        &self.statement_prefix
    }

    /// The words never used alone, outside the display name (WORD-021).
    #[must_use]
    pub fn forbidden_alone(&self) -> &[String] {
        &self.forbidden_alone
    }

    /// The Spanish names (WORD-001).
    #[must_use]
    pub fn es(&self) -> &BrandEs {
        &self.es
    }
}

impl BrandEs {
    /// The Spanish display name (WORD-001).
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The Spanish business-side name (WORD-001).
    #[must_use]
    pub fn business_name(&self) -> &str {
        &self.business_name
    }

    /// The name every Spanish text opens with (WORD-008).
    #[must_use]
    pub fn short_name(&self) -> &str {
        &self.short_name
    }

    /// The Spanish legal name (WORD-001).
    #[must_use]
    pub fn legal_name(&self) -> &str {
        &self.legal_name
    }
}

/// WORD-008: a text opens `<short_name>:`, so the short name holds no `:` and stays short.
fn check_short_name(value: &str, field: &'static str) -> Result<(), BrandError> {
    if value.contains(':') || value.chars().count() > SHORT_NAME_MAX {
        return Err(BrandError::Field(field));
    }
    Ok(())
}

/// TXT-006: a lower-case host name of two or more labels, each letters, digits and inner hyphens.
fn is_host_name(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    domain.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// SEC-025: the sender is `local@domain` or `local@<sub>.domain`.
fn sender_on_domain(address: &str, domain: &str) -> bool {
    let Some((local, host)) = address.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !local.chars().any(|c| c.is_whitespace() || c == '@')
        && is_host_name(host)
        && (host == domain || host.ends_with(&format!(".{domain}")))
}

/// FEE-049: 5 to 10 upper-case letters or digits, at least one letter, and `<prefix>* JOB ` plus
/// a 7-digit job number within the statement's 22 characters.
fn statement_prefix_fits(prefix: &str) -> bool {
    (5..=10).contains(&prefix.len())
        && prefix
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && prefix.chars().any(|c| c.is_ascii_uppercase())
        && prefix.len() + STATEMENT_JOINER.len() + JOB_NUMBER_DIGITS <= STATEMENT_MAX
}

/// Sets the process's brand once: the binaries' start-up installs the checked embedded brand
/// before anything renders, and a test may install a fake one (D-1932).
///
/// # Errors
///
/// [`BrandError::AlreadyInstalled`] on a second call.
pub fn install(brand: Brand) -> Result<(), BrandError> {
    INSTALLED
        .set(brand)
        .map_err(|_| BrandError::AlreadyInstalled)
}

/// The process's brand: the installed one, or else the embedded one (WORD-001, D-1932). If the
/// embedded file failed its check (the start-up refuses to run then, and `src/tests/brand.rs`
/// fails the build), this is an empty brand rather than a panic.
#[must_use]
pub fn brand() -> &'static Brand {
    INSTALLED.get().unwrap_or_else(|| {
        FALLBACK
            .get_or_init(|| Brand::embedded().unwrap_or_else(|_| RawBrand::default().into_brand()))
    })
}
