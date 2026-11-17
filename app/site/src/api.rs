//! The site's public surface: the router `server::build_router` merges and the banner read every
//! site page makes before it renders (WEB-001, WEB-007, T101), and the home and how-it-works
//! pages with their settings (WEB-012, WEB-021, WEB-026, WEB-028, T102), and the legal and data pages (WEB-014, TERMS-021,
//! TERMS-023, T221).

pub use crate::legal_pages::{
    BUSINESS_DATA_PATH, ES_BUSINESS_DATA_PATH, ES_YOUR_DATA_PATH, LegalDoc, PRIVACY_PATH,
    SMS_TERMS_PATH, TERMS_PATH, YOUR_DATA_PATH, accessibility_statement, legal_routes,
};
pub use crate::pages::{
    ES_HOW_IT_WORKS_PATH, HOME_PATH, HOW_IT_WORKS_PATH, OLD_CATEGORIES_PATH, OLD_EVENTS_PATH,
    SitePages, WhoRuns, home_html, how_it_works_html, page_routes,
};
pub use crate::routes::{banner_shown, site_routes};
