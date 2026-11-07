//! The business's notice-address and channel page (TD-BIZ-13; BIZ-036, BIZ-040, SEC-024; T196,
//! D-1848).
//!
//! [`notice_settings_page`] lists the verified notice addresses and those still waiting for their
//! code, shows the BIZ-040 banner line for each returned address, offers only email while business
//! texting is off (text and both only while it is on) and shows the change form to the owner
//! alone. [`change_channel`] stores a choice only for the signed-in owner (SEC-024). The addresses
//! are read from the vault by the composition and handed in; this module reads no contact. The
//! route [`NOTICE_SETTINGS_ROUTE`] is served with the business portal (D-1728, D-1848); address
//! changes go through `businesses::api::request_notice_change` (SEC-038's fresh code).

use askama::Template;
use askhava_businesses::api::{ChannelError, NoticeChannel, returned_line, set_notice_channel};
use jiff::Timestamp;
use sqlx::PgConnection;

use crate::business_auth::BusinessPrincipal;

/// The page's path in the business portal.
pub const NOTICE_SETTINGS_ROUTE: &str = "/b/notices";

/// One notice address as the composition read it from the vault, with its BIZ-040 state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeAddress {
    /// The address shown to the signed-in person of this business.
    pub shown: String,
    /// Verified and not returned (`businesses::api::may_get_notices`).
    pub verified: bool,
    /// Returned by a bounce or a complaint (BIZ-040).
    pub returned: bool,
}

/// What the page shows besides the addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFacts {
    /// The stored channel preference (BIZ-036).
    pub channel: NoticeChannel,
    /// The business-texting switch (TXT-035).
    pub texting_on: bool,
}

/// One channel choice on the page.
struct Choice {
    code: &'static str,
    label: &'static str,
    checked: bool,
}

#[derive(Template)]
#[template(path = "notice_settings.html")]
struct NoticeSettingsView<'a> {
    verified: Vec<&'a str>,
    waiting: Vec<&'a str>,
    banners: Vec<String>,
    choices: Vec<Choice>,
    owner: bool,
    texting_on: bool,
    csrf: &'a str,
}

/// BIZ-036, TXT-035: the channels offered: email alone while business texting is off.
#[must_use]
pub fn channel_choices(texting_on: bool) -> Vec<NoticeChannel> {
    if texting_on {
        vec![
            NoticeChannel::Email,
            NoticeChannel::Text,
            NoticeChannel::Both,
        ]
    } else {
        vec![NoticeChannel::Email]
    }
}

fn choice(channel: NoticeChannel, current: NoticeChannel) -> Choice {
    let (code, label) = match channel {
        NoticeChannel::Email => ("email", "Email"),
        NoticeChannel::Text => ("text", "Text"),
        NoticeChannel::Both => ("both", "Email and text"),
    };
    let checked = channel == current;
    Choice {
        code,
        label,
        checked,
    }
}

/// TD-BIZ-13 (BIZ-036, BIZ-040, SEC-024): the page for `principal`, with the CSRF token `csrf`.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn notice_settings_page(
    principal: &BusinessPrincipal,
    addresses: &[NoticeAddress],
    facts: ChannelFacts,
    csrf: &str,
) -> Result<String, askama::Error> {
    let current = if facts.texting_on {
        facts.channel
    } else {
        NoticeChannel::Email
    };
    let pick = |ok: bool| {
        addresses
            .iter()
            .filter(move |a| a.verified == ok && !a.returned)
    };
    let view = NoticeSettingsView {
        verified: pick(true).map(|a| a.shown.as_str()).collect(),
        waiting: pick(false).map(|a| a.shown.as_str()).collect(),
        banners: addresses
            .iter()
            .filter(|a| a.returned)
            .map(|a| returned_line(&a.shown))
            .collect(),
        choices: channel_choices(facts.texting_on)
            .into_iter()
            .map(|c| choice(c, current))
            .collect(),
        owner: principal.owner,
        texting_on: facts.texting_on,
        csrf,
    };
    view.render()
}

/// Why a channel change was not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChannelChangeError {
    /// Only the owner changes the preference (BIZ-036, SEC-024).
    #[error("only the owner changes the notice channel")]
    NotOwner,
    /// Text or both while business texting is off (TXT-035).
    #[error("text is offered only while business texting is on")]
    TextingOff,
    /// A database call failed.
    #[error("notice channel database call failed")]
    Database,
}

/// BIZ-036, SEC-024: stores `channel` for the signed-in owner's business; any other person is
/// refused.
///
/// # Errors
/// [`ChannelChangeError`].
pub async fn change_channel(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    facts: ChannelFacts,
    now: Timestamp,
) -> Result<(), ChannelChangeError> {
    if !principal.owner {
        return Err(ChannelChangeError::NotOwner);
    }
    let set = (facts.channel, facts.texting_on);
    match set_notice_channel(db, principal.business, set, now).await {
        Ok(()) => Ok(()),
        Err(ChannelError::TextingOff) => Err(ChannelChangeError::TextingOff),
        Err(ChannelError::Database) => Err(ChannelChangeError::Database),
    }
}
