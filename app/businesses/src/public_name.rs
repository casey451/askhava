//! The public business name shown to the homeowner, read once for many businesses (T316; PICK-041,
//! BIZ-022, TXT-031, TXT-027; D-336, D-1310, D-1687, D-2206, D-2943).
//!
//! [`public_names`] reads `businesses.business_name`, the row T165's `save_business_name` writes
//! at sign-up: the trade name (DBA) when the business gave one, else its business name. The chosen
//! name is screened again with [`screen_public_name`] (a row may predate a stricter screen); a
//! name that fails, and a known business with no stored name, are left out and raise one Normal
//! (Low) [`PUBLIC_NAME_ALERT`] about the business ID only, never the name or the reason. An
//! unknown ID is left out with no alert. Read only: no table, column or writer, and nothing
//! through `licensing`. Its callers are the LIST reply (T317) and the who's-calling text (T318);
//! each shows its own fallback for a business left out.

use askhava_ops::api::{Severity, raise_alert_once};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::about_us::screen_public_name;
use crate::models::BusinessesError;

/// The alert kind raised for a business whose public name is missing or fails the screen.
pub const PUBLIC_NAME_ALERT: &str = "businesses.public_name_unusable";

/// The alert's subject kind; its ID is the business ID.
const SUBJECT: &str = "businesses.business";

/// PICK-041, BIZ-022, D-336, D-2943: the name to show from a stored row, the trade name when
/// given, else the business name, or none when that name fails the phone, link and channel screen.
fn shown<'a>(business_name: &'a str, trade_name: Option<&'a str>) -> Option<&'a str> {
    let name = trade_name.unwrap_or(business_name);
    screen_public_name(name).ok().map(|()| name)
}

/// PICK-041, BIZ-022, TXT-031, TXT-027, D-336, D-2206, D-2943: each known business in `ids` with
/// its public name ([`shown`] of its stored names), in the order of `ids` (a repeated ID once). A
/// known business with no stored name, or whose name fails the screen, is left out and raises one
/// open [`PUBLIC_NAME_ALERT`] with its ID only (`raise_alert_once`, so repeated reads add none);
/// an unknown ID is left out silently.
///
/// # Errors
/// [`BusinessesError::Database`] when the read or an alert fails.
pub async fn public_names(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, BusinessesError> {
    let rows = sqlx::query!(
        r#"SELECT b.id AS "id!", n.business_name AS "business_name?", n.trade_name
           FROM unnest($1::uuid[]) WITH ORDINALITY AS wanted (id, at)
           JOIN businesses.business b ON b.id = wanted.id
           LEFT JOIN businesses.business_name n ON n.business_id = b.id
           ORDER BY wanted.at"#,
        ids,
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| BusinessesError::Database)?;
    let mut names: Vec<(Uuid, String)> = Vec::with_capacity(rows.len());
    let mut seen: Vec<Uuid> = Vec::with_capacity(rows.len());
    for row in rows {
        if seen.contains(&row.id) {
            continue;
        }
        seen.push(row.id);
        let name = row
            .business_name
            .as_deref()
            .and_then(|b| shown(b, row.trade_name.as_deref()));
        match name {
            Some(n) => names.push((row.id, n.to_owned())),
            None => {
                raise_alert_once(conn, PUBLIC_NAME_ALERT, Severity::Normal, (SUBJECT, row.id))
                    .await
                    .map_err(|_| BusinessesError::Database)?;
            }
        }
    }
    Ok(names)
}
