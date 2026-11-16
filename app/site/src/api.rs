//! The site's public surface: the router `server::build_router` merges and the banner read every
//! site page makes before it renders (WEB-001, WEB-007, T101).

pub use crate::routes::{banner_shown, site_routes};
