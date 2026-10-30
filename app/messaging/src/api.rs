//! The public face of `askhava-messaging`: everything other crates may call (D-481).
//!
//! Render a text from its template ID, pick the language it may use, and count its segments.

pub use crate::templates::{
    Audience, Encoding, Language, Registry, Segments, TemplateError, Text, count_segments,
};

/// Renders template `id` for a person whose wanted language is `wanted`, filling each `{name}` from
/// `values`. Spanish is used only while `spanish_conversations_on` is true (TXT-039, the owner-portal
/// setting) or the Spanish template is marked `spanish_stays_on`; a Spanish text is marked a draft
/// (TXT-003). The language is the person's own, so an owner phone in English and a team phone in
/// Spanish are rendered separately (BIZ-018, TXT-002).
///
/// # Errors
///
/// [`TemplateError::Unknown`] for an ID not in the registry, [`TemplateError::MissingValue`] when a
/// placeholder has no value, [`TemplateError::Invalid`] if the compiled-in files are broken.
pub fn render(
    id: &str,
    wanted: Language,
    spanish_conversations_on: bool,
    values: &[(&str, &str)],
) -> Result<Text, TemplateError> {
    Registry::embedded()?.render(id, wanted, spanish_conversations_on, values)
}

/// Every template ID in the compiled-in registry, sorted (TXT-001).
///
/// # Errors
///
/// [`TemplateError::Invalid`] if the compiled-in files are broken.
pub fn template_ids() -> Result<Vec<String>, TemplateError> {
    Ok(Registry::embedded()?.ids())
}
