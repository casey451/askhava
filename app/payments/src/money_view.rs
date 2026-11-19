//! FEE-046 (D-410, D-444, D-491; T210): the owner money view's lines outside the ledger.
//!
//! [`owner_money_view`] puts T067's ledger-only [`MoneyView`] (from `money_view`, unchanged) for
//! one Arizona calendar [`Month`] beside four lines: **collected** (introduction fees charged less
//! refunds, credits and disputed amounts, all from the ledger), **set aside** (the sales-tax
//! set-aside: `Settings::sales_tax_set_aside_basis_points`, 860 = 8.6%, of collected, rounded up
//! to the cent as OPS-039's net does; 0 when collected is not above 0; the setting goes to 0 once
//! the ADOR ruling is filed), **expenses** (Casey's manual list, `payments.expense`) and **left**
//! (collected less set aside less expenses), so the four always add up (D-4728). Stripe's dispute
//! and processing fee lines stay in the ledger view (FEE-050 to FEE-052). [`add_expense`] stores
//! one expense and never writes a ledger row (FEE-028). [`money_view_csv`] is the monthly CSV.

use std::fmt;

use askhava_core::api::arizona;
use jiff::civil::Date;
use jiff::{Span, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;

use crate::models::MoneyView;
use crate::reconcile::money_view;

/// The largest single expense: $1,000,000.00, as the migration's check.
const MAX_EXPENSE_CENTS: i64 = 100_000_000;

/// The monthly CSV's header row (FEE-046): one `expense` row each, then the `month` row.
pub const CSV_HEADER: &str = "line,date,vendor,category,note,collected,set_aside,expenses,left";

/// Why an expense or the money view could not be made. No variant carries a value or a database
/// message (R3, R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExpenseError {
    /// The date is before 2000 or after today in Arizona.
    #[error("the expense date is out of range")]
    Date,
    /// The vendor is empty, over 120 characters or holds a control character.
    #[error("the vendor is empty or too long")]
    Vendor,
    /// The amount is not between 1 cent and $1,000,000.
    #[error("the amount is out of range")]
    Amount,
    /// The category is empty, over 60 characters or holds a control character.
    #[error("the category is empty or too long")]
    Category,
    /// The note is over 500 characters or holds a control character.
    #[error("the note is too long")]
    Note,
    /// The month is outside the years 2000 to 2999.
    #[error("the month is out of range")]
    Month,
    /// The database refused the request.
    #[error("the database refused the request")]
    Database,
}

/// One Arizona calendar month (FEE-046), the money view's and the CSV's period (R2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Month {
    first: Date,
}

impl Month {
    /// The month `month` (1 to 12) of `year` (2000 to 2999).
    ///
    /// # Errors
    /// [`ExpenseError::Month`] outside those ranges.
    pub fn new(year: i16, month: i8) -> Result<Self, ExpenseError> {
        if !(2000..=2999).contains(&year) {
            return Err(ExpenseError::Month);
        }
        let first = Date::new(year, month, 1).map_err(|_| ExpenseError::Month)?;
        Ok(Self { first })
    }

    /// The Arizona calendar month holding `at` (JOB-007).
    #[must_use]
    pub fn of(at: Timestamp) -> Self {
        Self {
            first: at.to_zoned(arizona()).date().first_of_month(),
        }
    }

    /// The month's first day and the next month's first day.
    fn days(self) -> Result<(Date, Date), ExpenseError> {
        let next = self.first.checked_add(Span::new().months(1));
        Ok((self.first, next.map_err(|_| ExpenseError::Month)?))
    }

    /// `[start, end)` of the month in Arizona time, the window `money_view` reads.
    fn window(self) -> Result<(Timestamp, Timestamp), ExpenseError> {
        let start = |d: Date| d.to_zoned(arizona()).map(|z| z.timestamp());
        let (first, next) = self.days()?;
        start(first)
            .and_then(|from| Ok((from, start(next)?)))
            .map_err(|_| ExpenseError::Month)
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.first.year(), self.first.month())
    }
}

/// FEE-046: one checked manual expense Casey enters (R1, R2); only [`NewExpense::new`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExpense {
    spent_on: Date,
    vendor: String,
    amount_cents: i64,
    category: String,
    note: String,
}

impl NewExpense {
    /// FEE-046: checks each field (text trimmed); `note` may be empty.
    ///
    /// # Errors
    /// The [`ExpenseError`] variant of the first field that does not fit.
    pub fn new(
        spent_on: Date,
        vendor: &str,
        amount_cents: i64,
        category: &str,
        note: &str,
    ) -> Result<Self, ExpenseError> {
        if spent_on.year() < 2000 {
            return Err(ExpenseError::Date);
        }
        let vendor = text(vendor, 1, 120).ok_or(ExpenseError::Vendor)?;
        if !(1..=MAX_EXPENSE_CENTS).contains(&amount_cents) {
            return Err(ExpenseError::Amount);
        }
        let category = text(category, 1, 60).ok_or(ExpenseError::Category)?;
        let note = text(note, 0, 500).ok_or(ExpenseError::Note)?;
        Ok(Self {
            spent_on,
            vendor,
            amount_cents,
            category,
            note,
        })
    }
}

/// `raw` trimmed when it has `min` to `max` characters and no control character.
fn text(raw: &str, min: usize, max: usize) -> Option<String> {
    let t = raw.trim();
    let n = t.chars().count();
    (n >= min && n <= max && !t.chars().any(char::is_control)).then(|| t.to_owned())
}

/// FEE-046: one stored expense, as the money view lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expense {
    /// The row ID.
    pub id: i64,
    /// The Arizona date it was spent.
    pub spent_on: Date,
    /// Who was paid.
    pub vendor: String,
    /// Whole cents, above 0.
    pub amount_cents: i64,
    /// Casey's category.
    pub category: String,
    /// Casey's note, possibly empty.
    pub note: String,
}

/// FEE-046: the owner money view of one month: the ledger view beside the four lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerMoneyView {
    /// The month shown.
    pub month: Month,
    /// T067's view, from the ledger only (FEE-030, FEE-051), unchanged.
    pub ledger: MoneyView,
    /// Fees collected: charges less refunds, credits and disputed amounts, in cents.
    pub collected: i64,
    /// The sales-tax set-aside, in cents.
    pub set_aside: i64,
    /// The month's expenses, by date.
    pub expenses: Vec<Expense>,
    /// Their sum, in cents.
    pub expenses_total: i64,
    /// Collected less set aside less expenses, in cents.
    pub left: i64,
}

/// FEE-046, D-444: `basis_points` (at most 10,000) of `collected`, rounded up to the cent; 0 when
/// nothing above 0 was collected.
pub(crate) fn set_aside_cents(collected: i64, basis_points: u16) -> i64 {
    if collected <= 0 {
        return 0;
    }
    let bp = i64::from(basis_points.min(10_000));
    collected / 10_000 * bp + (collected % 10_000 * bp + 9_999) / 10_000
}

/// FEE-046: the four lines from the ledger view and the expenses.
pub(crate) fn assemble(
    month: Month,
    ledger: MoneyView,
    expenses: Vec<Expense>,
    basis_points: u16,
) -> OwnerMoneyView {
    let collected = ledger.charges - ledger.refunds - ledger.credits - ledger.disputes;
    let set_aside = set_aside_cents(collected, basis_points);
    let expenses_total = expenses.iter().map(|e| e.amount_cents).sum::<i64>();
    OwnerMoneyView {
        month,
        ledger,
        collected,
        set_aside,
        expenses,
        expenses_total,
        left: collected - set_aside - expenses_total,
    }
}

/// FEE-046, FEE-028: stores one expense entered at `now` and returns its ID; never a ledger row.
///
/// # Errors
/// [`ExpenseError::Date`] for a date after `now`'s Arizona date; [`ExpenseError::Database`].
pub async fn add_expense(
    pool: &PgPool,
    expense: &NewExpense,
    now: Timestamp,
) -> Result<i64, ExpenseError> {
    if expense.spent_on > now.to_zoned(arizona()).date() {
        return Err(ExpenseError::Date);
    }
    sqlx::query_scalar!(
        "INSERT INTO payments.expense (spent_on, vendor, amount_cents, category, note, entered_at)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        expense.spent_on.to_sqlx() as _,
        expense.vendor,
        expense.amount_cents,
        expense.category,
        expense.note,
        now.to_sqlx() as _,
    )
    .fetch_one(pool)
    .await
    .map_err(|_| ExpenseError::Database)
}

/// FEE-046: the month's expenses, by date then entry order.
async fn expenses_in(pool: &PgPool, month: Month) -> Result<Vec<Expense>, ExpenseError> {
    let (first, next) = month.days()?;
    let rows = sqlx::query!(
        r#"SELECT id, spent_on AS "spent_on: jiff_sqlx::Date", vendor, amount_cents, category, note
             FROM payments.expense WHERE spent_on >= $1 AND spent_on < $2 ORDER BY spent_on, id"#,
        first.to_sqlx() as _,
        next.to_sqlx() as _,
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ExpenseError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| Expense {
            id: r.id,
            spent_on: r.spent_on.to_jiff(),
            vendor: r.vendor,
            amount_cents: r.amount_cents,
            category: r.category,
            note: r.note,
        })
        .collect())
}

/// FEE-046, FEE-030, FEE-051: the owner money view of `month`, the set-aside at `basis_points`
/// (the caller passes `Settings::sales_tax_set_aside_basis_points`, D-3844).
///
/// # Errors
/// [`ExpenseError::Month`] for a month with no Arizona window; [`ExpenseError::Database`].
pub async fn owner_money_view(
    pool: &PgPool,
    month: Month,
    basis_points: u16,
) -> Result<OwnerMoneyView, ExpenseError> {
    let ledger = money_view(pool, month.window()?)
        .await
        .map_err(|_| ExpenseError::Database)?;
    let expenses = expenses_in(pool, month).await?;
    Ok(assemble(month, ledger, expenses, basis_points))
}

/// FEE-046: the monthly CSV ([`CSV_HEADER`]) of [`owner_money_view`].
///
/// # Errors
/// As [`owner_money_view`].
pub async fn money_view_csv(
    pool: &PgPool,
    month: Month,
    basis_points: u16,
) -> Result<String, ExpenseError> {
    Ok(csv_of(&owner_money_view(pool, month, basis_points).await?))
}

/// FEE-046: the CSV text of `view`, CRLF line ends, amounts in dollars.
pub(crate) fn csv_of(view: &OwnerMoneyView) -> String {
    let mut rows = vec![CSV_HEADER.to_owned()];
    for e in &view.expenses {
        let date = e.spent_on.to_string();
        let (vendor, category, note) = (cell(&e.vendor), cell(&e.category), cell(&e.note));
        let amount = dollars(e.amount_cents);
        rows.push(
            [
                "expense", &date, &vendor, &category, &note, "", "", &amount, "",
            ]
            .join(","),
        );
    }
    let month = view.month.to_string();
    let totals = [
        view.collected,
        view.set_aside,
        view.expenses_total,
        view.left,
    ]
    .map(dollars);
    rows.push(format!("month,{month},,,,{}", totals.join(",")));
    rows.join("\r\n") + "\r\n"
}

/// One CSV text cell: a leading `=`, `+`, `-` or `@` is prefixed with `'` so a spreadsheet never
/// runs it as a formula (CSV injection); a comma or quote quotes the cell.
fn cell(text: &str) -> String {
    let safe = if text.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if safe.contains([',', '"']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

/// Cents as dollars with two decimals, a leading `-` when negative.
fn dollars(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let a = cents.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}
