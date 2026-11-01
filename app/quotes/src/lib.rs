//! Quotes (`askhava-quotes`, layer 5): who receives each job card, the cards and the quotes
//! (`docs/architecture.md` section 3).
//!
//! T044 added the matching: a card goes to every active, verified business serving one of the
//! job's Arizona offer towns whose licenses cover every candidate job type as the license map says
//! and which ticked one of them, with no cap (CARD-001 to CARD-003, LIC-035), in `models`, `rules`
//! and `service`.
//!
//! T045 added the anonymous card and the final plain-code gate (CARD-005 to CARD-009, PRIV-006)
//! in `cards` and `gate`.
//!
//! T046 added quote intake: the 24-hour window, the checked quote and its bounces, the stored
//! record (PICK-001, PICK-002, PICK-007) in `intake`, and the every-window-ended check that hands
//! the counts to `jobs` (TXT-042, JOB-025) in `handlers`.
//!
//! T186 added the quote card in `quote_card`: the business's name, license lines, the quote facts
//! in order, its own words beside any checked summary, the after-hours line, the same for the
//! homeowner and the business's preview (PICK-003 to PICK-005, PICK-040, PICK-041, PICK-044).
//!
//! T047 added the homeowner quote texts in `notify`: the first-quote text, the 3-hour update gap, the
//! LIST text and the cards-out text (TXT-016, TXT-017, TXT-031 to TXT-033, WORD-011).
//!
//! Other crates reach it only through [`api`].

pub mod api;
mod cards;
mod gate;
mod handlers;
mod intake;
mod models;
mod notify;
mod quote_card;
mod rules;
mod service;

#[cfg(test)]
mod tests;
