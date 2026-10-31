//! AI (`askhava-ai`, layer 3): the intake reading through the AI adapter, strict schemas, no tools,
//! versioned instructions, the AI-021 output screen, the AI-019 output checks and writer fallbacks
//! and the AI-008 zero-retention gate (AI-001, AI-005, AI-006, AI-008, AI-010, AI-013 to AI-015,
//! AI-019, AI-021 to AI-025).
//!
//! `ai` accepts only `ScrubbedText` (AI-004), sends it through `adapters`' AI client to the Claude
//! Messages API only (AI-001), and keeps only schema-checked fields (AI-005). Other crates reach it
//! only through [`api`]; nothing it returns is executed (AI-006).

pub mod api;
mod models;
mod output_checks;
mod schemas;
mod service;
mod switch;
mod writer;

#[cfg(test)]
mod tests;
