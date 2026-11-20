//! Recovery (`askhava-recovery`, layer 8): the nightly encrypted export and the backup checks
//! (`docs/architecture.md` sections 3 and 10; OPS-013, OPS-051, PRIV-018, PRIV-024, PRIV-029).
//!
//! T109 built the nightly export (`backup`): `pg_dump -Fc` as the read-only `askhava_backup`
//! role, encrypted to an offline `age` recipient, stored under `exports/`, each result one
//! `recovery.backup_check` row and one `ops` audit entry.
//! T110 built the replay after a restore (`replay`): texts and charges rebuilt from Twilio's and
//! Stripe's records with derived IDs, so nothing replayed sends or charges twice (OPS-015 to
//! OPS-017). Other crates reach it only through
//! [`api`].

pub mod api;
mod backup;
mod models;
mod replay;
#[cfg(test)]
mod tests;
