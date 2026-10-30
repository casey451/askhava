//! Catalog (`askhava-catalog`, layer 2): categories, job types, license rules, fee rules and the
//! category switches (LIC-001 to LIC-005, AI-017).
//!
//! `catalog` loads the reviewed file `docs/catalog/categories.yaml` into its own `catalog` schema and
//! answers which categories are on. Other crates reach it only through [`api`].

pub mod api;
mod loader;
mod models;
mod question_bank;
mod service;

#[cfg(test)]
mod tests;
