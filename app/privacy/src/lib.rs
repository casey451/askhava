//! Privacy (`askhava-privacy`, layer 2): the contact vault (PRIV-001 to PRIV-004, PRIV-024, SEC-030, SEC-046, SEC-047).
//!
//! Every homeowner contact field is kept here only, sealed field by field under `VAULT_KEY`, and
//! every other record refers to it by ID (PRIV-001, PRIV-002). Plaintext leaves only through
//! [`api`], audit-logged with its reader and purpose (PRIV-003, PRIV-004). Other crates reach
//! the vault only through [`api`]; the release and realm reads need proof types only `privacy`'s
//! once-per-process mints make (`docs/architecture.md` section 4, rule 4; D-481, D-488).

pub mod api;
mod models;
mod vault;
