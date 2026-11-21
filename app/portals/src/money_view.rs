//! The owner money view page, its expense entry and its monthly CSV (FEE-046, FEE-028; T434,
//! D-410, D-444, D-492, D-3445, D-3744, D-3890).
//!
//! [`money_view_page`] renders `templates/money_view.html` from T210's
//! `payments::api::owner_money_view` only: the ledger lines (T067's `MoneyView`) beside the four
//! lines collected, set aside (sales tax), expenses and left, and the month's expenses. It computes
//! no figure itself; it only words cents as dollars. [`post_expense`] parses the entry form once
//! ([`parse_expense`]) and stores it with `payments::api::add_expense`, which never writes a ledger
//! row (FEE-028); a bad field re-renders the page with that field's error and the values kept.
//! [`money_view_csv_file`] is `payments::api::money_view_csv` as it stands. `portals` reads no
//! `Settings`, so the serving route (T305, D-2128) passes the set-aside basis points
//! (`Settings::sales_tax_set_aside_basis_points`) and the CSRF token, and checks that token on
//! the `POST`. Every route is a fixed path under `/owner/`, the month a `?month=YYYY-MM` query
//! or form value read by [`month_start`] (D-3890).

use askama::Template;
use askhava_core::api::arizona;
use askhava_payments::api::{
    ExpenseError, Month, NewExpense, OwnerMoneyView, add_expense, money_view_csv, owner_money_view,
};
use jiff::Timestamp;
use jiff::civil::Date;
use serde::Deserialize;
use sqlx::PgPool;

use crate::owner_portal::OwnerToolError;

/// The money view page's path (FEE-046); `server` serves it (T305).
pub const OWNER_MONEY_ROUTE: &str = "/owner/money";
/// The monthly CSV download's path (FEE-046), the month as a query value.
pub const OWNER_MONEY_CSV_ROUTE: &str = "/owner/money/csv";
/// The expense entry `POST`'s path (FEE-046).
pub const OWNER_EXPENSE_ROUTE: &str = "/owner/money/expense";

/// The `?month=YYYY-MM` query of the page and the CSV (FEE-046, D-3890).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MonthQuery {
    /// The month asked for; the current Arizona month when absent.
    pub month: Option<String>,
}

/// The expense entry form's body (FEE-046); the CSRF field is read by `server`'s `CsrfForm`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExpenseForm {
    /// The date spent, `YYYY-MM-DD`.
    pub spent_on: String,
    /// Who was paid.
    pub vendor: String,
    /// The amount in dollars, such as `12.50`, `$1,200` or `40`.
    pub amount: String,
    /// The expense's category, such as `Software`.
    pub category: String,
    /// An optional note.
    #[serde(default)]
    pub note: String,
}

/// What the expense `POST` did (FEE-046).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpenseAnswer {
    /// Stored; redirect to this path (the page of the expense's month).
    Added(String),
    /// Nothing stored: the page again, with the field's error and the values kept.
    Refused(String),
}

#[derive(Template)]
#[template(path = "money_view.html")]
struct MoneyPage<'a> {
    month: String,
    month_value: String,
    view: &'a OwnerMoneyView,
    csrf: &'a str,
    entered: &'a ExpenseForm,
    problem: Option<(&'static str, &'static str)>,
}

impl MoneyPage<'_> {
    fn dollars(&self, cents: &i64) -> String {
        let sign = if *cents < 0 { "-" } else { "" };
        format!("{sign}${}.{:02}", cents.abs() / 100, cents.abs() % 100)
    }

    fn error_for(&self, field: &str) -> Option<&'static str> {
        self.problem
            .filter(|(f, _)| *f == field)
            .map(|(_, why)| why)
    }
}

fn db<E>(_: E) -> OwnerToolError {
    OwnerToolError::Database
}

/// FEE-046, JOB-007: the first day of the month `raw` names (`YYYY-MM`), or of `now`'s Arizona
/// month when `raw` is absent or blank; `None` for anything else.
#[must_use]
pub fn month_start(raw: Option<&str>, now: Timestamp) -> Option<Date> {
    match raw.map(str::trim).filter(|r| !r.is_empty()) {
        None => Some(now.to_zoned(arizona()).date().first_of_month()),
        Some(r) => Date::strptime("%Y-%m-%d", format!("{r}-01")).ok(),
    }
}

/// FEE-046: `raw` dollars (an optional `$`, commas and up to two decimals) as cents.
fn cents(raw: &str) -> Option<i64> {
    let clean: String = raw.trim().trim_start_matches('$').replace(',', "");
    let (whole, part) = clean.split_once('.').unwrap_or((&clean, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || part.len() > 2 || !digits(whole) || !digits(part) {
        return None;
    }
    let part = format!("{part:0<2}").parse::<i64>().ok()?;
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(part)
}

/// FEE-046 (R1): the form parsed once into a checked [`NewExpense`].
///
/// # Errors
///
/// The [`ExpenseError`] of the first field that does not fit.
pub fn parse_expense(form: &ExpenseForm) -> Result<NewExpense, ExpenseError> {
    let date = Date::strptime("%Y-%m-%d", form.spent_on.trim()).map_err(|_| ExpenseError::Date)?;
    let amount = cents(&form.amount).ok_or(ExpenseError::Amount)?;
    NewExpense::new(date, &form.vendor, amount, &form.category, &form.note)
}

/// The field an error belongs to and the line shown beside it (FEE-046).
fn problem(e: ExpenseError) -> (&'static str, &'static str) {
    match e {
        ExpenseError::Date => (
            "spent_on",
            "Enter a date between 2000 and today, like 2026-11-04.",
        ),
        ExpenseError::Vendor => ("vendor", "Enter who you paid, up to 120 characters."),
        ExpenseError::Amount => (
            "amount",
            "Enter an amount from $0.01 to $1,000,000, like 12.50.",
        ),
        ExpenseError::Category => ("category", "Enter a category, up to 60 characters."),
        ExpenseError::Note => ("note", "Shorten the note to 500 characters or fewer."),
        ExpenseError::Month | ExpenseError::Database => ("", ""),
    }
}

/// Renders the page of `first`'s month with the form `entered` and its `problem`.
async fn render(
    pool: &PgPool,
    (first, basis_points): (Date, u16),
    (csrf, entered): (&str, &ExpenseForm),
    problem: Option<(&'static str, &'static str)>,
) -> Result<String, OwnerToolError> {
    let month = Month::new(first.year(), first.month()).map_err(|_| OwnerToolError::Refused)?;
    let view = owner_money_view(pool, month, basis_points)
        .await
        .map_err(db)?;
    MoneyPage {
        month: first.strftime("%B %Y").to_string(),
        month_value: month.to_string(),
        view: &view,
        csrf,
        entered,
        problem,
    }
    .render()
    .map_err(db)
}

/// FEE-046: the money view page of the month starting on `first`, the set-aside at
/// `basis_points`, with an empty expense form carrying `csrf`.
///
/// # Errors
///
/// [`OwnerToolError::Refused`] for a month outside 2000 to 2999; [`OwnerToolError::Database`].
pub async fn money_view_page(
    pool: &PgPool,
    first: Date,
    basis_points: u16,
    csrf: &str,
) -> Result<String, OwnerToolError> {
    render(
        pool,
        (first, basis_points),
        (csrf, &ExpenseForm::default()),
        None,
    )
    .await
}

/// FEE-046, FEE-028: stores the expense `form` names at `now` (never a ledger row) and answers
/// the redirect to its month's page, or the page again with the field's error.
///
/// # Errors
///
/// [`OwnerToolError::Database`] when the store or a re-render fails.
pub async fn post_expense(
    pool: &PgPool,
    form: &ExpenseForm,
    (basis_points, csrf): (u16, &str),
    now: Timestamp,
) -> Result<ExpenseAnswer, OwnerToolError> {
    let added = match parse_expense(form) {
        Ok(expense) => add_expense(pool, &expense, now).await.map(|_| expense),
        Err(e) => Err(e),
    };
    let today = now.to_zoned(arizona()).date().first_of_month();
    match added {
        Ok(_) => {
            let first = Date::strptime("%Y-%m-%d", form.spent_on.trim()).map_err(db)?;
            let month = first.strftime("%Y-%m");
            Ok(ExpenseAnswer::Added(format!(
                "{OWNER_MONEY_ROUTE}?month={month}"
            )))
        }
        Err(ExpenseError::Database | ExpenseError::Month) => Err(OwnerToolError::Database),
        Err(e) => {
            let page = render(pool, (today, basis_points), (csrf, form), Some(problem(e))).await?;
            Ok(ExpenseAnswer::Refused(page))
        }
    }
}

/// FEE-046: the monthly CSV of the month starting on `first`, as `payments` makes it.
///
/// # Errors
///
/// [`OwnerToolError::Refused`] for a month outside 2000 to 2999; [`OwnerToolError::Database`].
pub async fn money_view_csv_file(
    pool: &PgPool,
    first: Date,
    basis_points: u16,
) -> Result<String, OwnerToolError> {
    let month = Month::new(first.year(), first.month()).map_err(|_| OwnerToolError::Refused)?;
    money_view_csv(pool, month, basis_points).await.map_err(db)
}

#[cfg(test)]
mod tests {
    //! The form's parsing (FEE-046).

    use super::*;

    #[test]
    fn amounts_parse_to_cents_and_bad_ones_do_not() {
        assert_eq!(cents("12.5"), Some(1250));
        assert_eq!(cents("$1,200"), Some(120_000));
        assert_eq!(cents(" 0.07 "), Some(7));
        for bad in ["", "abc", "1.234", "-5", ".5", "1.2.3"] {
            assert_eq!(cents(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_month_is_a_query_value_or_this_arizona_month() {
        let now: Timestamp = "2026-12-01T05:00:00Z".parse().expect("now");
        let nov = Date::new(2026, 11, 1).expect("date");
        assert_eq!(month_start(None, now), Some(nov));
        assert_eq!(month_start(Some("2026-11"), now), Some(nov));
        assert_eq!(month_start(Some("2026-13"), now), None);
    }
}
