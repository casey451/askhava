//! Privacy (`askhava-privacy`, layer 2): the contact vault, the inbound reader and the scrubber (PRIV-001 to PRIV-004, PRIV-024, PRIV-026, SEC-030, SEC-046, SEC-047, AI-002 to AI-004, AI-020).
//!
//! Every homeowner contact field is kept here only, sealed field by field under `VAULT_KEY`, and
//! every other record refers to it by ID (PRIV-001, PRIV-002). Plaintext leaves only through
//! [`api`], audit-logged with its reader and purpose (PRIV-003, PRIV-004). Other crates reach
//! the vault only through [`api`]; the release and realm reads need proof types only `privacy`'s
//! once-per-process mints make (`docs/architecture.md` section 4, rule 4; D-481, D-488). Raw
//! homeowner text is read only by `reader`, and only `scrub` makes `ScrubbedText`, the one text
//! type `ai` accepts (AI-004, D-093).

pub mod api;
mod models;
mod reader;
mod scrub;
mod vault;

#[cfg(test)]
mod tests;
