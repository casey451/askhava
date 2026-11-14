//! The quote cards on the served quote page (T393, IC-71; PICK-041, PICK-040, PICK-010, LIC-024,
//! LIC-026, LIC-034, LIC-038, LIC-047, BIZ-052; D-2206, D-3080, D-3134, D-3642 to D-3645).
//!
//! [`quote_cards`] builds the card of each shown quote (not withdrawn, not unavailable): one read
//! of the public names (`businesses::api::public_names`), the license facts of the job's category
//! (`licensing::api::card_facts`) turned into lines in the page's language, with a no-license
//! card's handyman tick and insurance answer read here from `businesses::api` (licensing never
//! calls `businesses`), the quote's PICK-040 facts (`quotes::api::stored_facts`) and
//! `quotes::api::shown_card`, which adds the Google and Yelp links, the after-hours line and the
//! "Updated <time>" line. A business `public_names` leaves out shows [`CARD_FALLBACK_NAME`] (D-3642),
//! never a name from anywhere else. A quote with no stored facts, or a job with no stored
//! category, gets no card and keeps the page's number and Pick button (D-3134, D-3643). A pest
//! card leads with the LIC-047 attested line (D-3644). No homeowner detail and no business phone
//! or address is read here (PICK-010).

use askhava_businesses::api::{Insurance, Known, business_names, holding, public_names};
use askhava_jobs::api::job_facts;
use askhava_licensing::api::{
    Agency, CardFacts, CardLanguage, CardLine, CityRecord, Issuer, LicenseFacts, card_facts,
    licensed_lines, no_license_lines,
};
use askhava_quotes::api::{CardSource, shown_card, stored_facts};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::quote_page::Numbered;

/// PICK-041, D-3642: the name a card shows, English then Spanish, for a business with no usable
/// public name.
pub const CARD_FALLBACK_NAME: [&str; 2] =
    ["No business name given", "Este negocio no dio su nombre"];

/// LIC-047, D-3644: the Arizona Department of Agriculture's license search.
const PEST_SEARCH_LINK: &str = "https://searchagriculture.az.gov/mastercontent/licsearch.aspx";

/// LIC-047's words, `{number}` and `{brand}` filled; English in both languages, as LIC-024's.
const PEST_LINE: &str = "This business told us it holds Arizona Department of Agriculture pest \
                         license #{number}. {brand} has not independently verified this. Check it \
                         yourself:";

/// Why the cards could not be built; carries no value (rust-production R5).
#[derive(Debug, thiserror::Error)]
#[error("a quote card read failed")]
pub(crate) struct CardsError;

/// Each carded quote's number with its card lines, in page order.
pub(crate) type Cards = Vec<(usize, Vec<CardLine>)>;

fn read<E>(_: E) -> CardsError {
    CardsError
}

/// PICK-041, PICK-040, BIZ-052, D-3134: the cards of `quotes` of `job` in `language`.
///
/// # Errors
///
/// [`CardsError`] when a read fails.
pub(crate) async fn quote_cards(
    db: &mut PgConnection,
    job: Uuid,
    quotes: &[Numbered],
    language: CardLanguage,
) -> Result<Cards, CardsError> {
    let shown: Vec<&Numbered> = quotes
        .iter()
        .filter(|q| !q.quote.withdrawn && !q.quote.unavailable)
        .collect();
    if shown.is_empty() {
        return Ok(Vec::new());
    }
    let Some(category) = job_facts(db, job).await.map_err(read)?.map(|f| f.category) else {
        return Ok(Vec::new());
    };
    let ids: Vec<Uuid> = shown.iter().map(|q| q.quote.business).collect();
    let names = public_names(db, &ids).await.map_err(read)?;
    let mut cards = Vec::new();
    for q in shown {
        let business = q.quote.business;
        let Some(facts) = stored_facts(db, job, business).await.map_err(read)? else {
            continue;
        };
        let business_name = names.iter().find(|(id, _)| *id == business).map_or_else(
            || CARD_FALLBACK_NAME[usize::from(language == CardLanguage::Spanish)].to_owned(),
            |(_, name)| name.clone(),
        );
        let license_lines = license_lines(db, business, &category, language).await?;
        let source = CardSource {
            business_name,
            license_lines,
            facts,
        };
        if let Some(card) = shown_card(db, (job, business), source)
            .await
            .map_err(read)?
        {
            cards.push((q.number, card.lines(language)));
        }
    }
    Ok(cards)
}

/// LIC-024, LIC-026, LIC-034, LIC-038, LIC-047: `business`'s license lines for a job of
/// `category`; none for a category the catalog does not hold.
async fn license_lines(
    db: &mut PgConnection,
    business: Uuid,
    category: &str,
    language: CardLanguage,
) -> Result<Vec<CardLine>, CardsError> {
    let own = business_names(db, business).await.map_err(read)?;
    let city = own.map(|n| CityRecord {
        business_name: n.business_name,
        trade_name: n.trade_name,
        owner_name: None,
        license_number: n.city_license_number,
    });
    match card_facts(db, business, category, city.as_ref())
        .await
        .map_err(read)?
    {
        None => Ok(Vec::new()),
        Some(CardFacts::Licensed(list)) => licensed(&list, language),
        Some(CardFacts::NoLicense(mut facts)) => {
            let parts = holding(db, business, Known::default())
                .await
                .map_err(read)?;
            let record = askhava_businesses::api::business(db, business)
                .await
                .map_err(read)?;
            facts.handyman = parts.holder.handyman;
            facts.insurance = record.insurance.map(|i| i == Insurance::Yes);
            no_license_lines(&facts, language).map_err(read)
        }
    }
}

/// LIC-026, LIC-034, LIC-037, LIC-047: each license's lines, a pest license led by its attested
/// line with the Department's search link.
fn licensed(list: &[LicenseFacts], language: CardLanguage) -> Result<Vec<CardLine>, CardsError> {
    let mut lines = Vec::new();
    for facts in list {
        if facts.issuer == Issuer::Agency(Agency::PestManagement) {
            lines.push(pest_line(&facts.number));
        }
        lines.extend(licensed_lines(facts, language).map_err(read)?);
    }
    Ok(lines)
}

/// LIC-047: the self-attested pest line for license `number`, linked to the license search.
fn pest_line(number: &str) -> CardLine {
    let text = PEST_LINE
        .replace("{number}", number)
        .replace("{brand}", askhava_core::api::brand().display_name());
    CardLine {
        text,
        link: Some(PEST_SEARCH_LINK.to_owned()),
    }
}
