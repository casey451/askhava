//! T410: every signed-in business page shows the "Get help" link, and no other page does
//! (BIZ-047, TD-BIZ-23, D-3155, D-3890). The link lives only in `business_layout.html`'s fill
//! of `base.html`'s empty `get_help` block, so the check reads every template under
//! `app/portals/templates/` and `app/site/templates/`: exactly [`BUSINESS_PAGES`] extend the
//! business layout, no other template fills the block or links the form, and the business help
//! page links the form once in its body. Rendered pages then confirm it: a business page holds
//! the link; the quote page, the owner sign-in page and a site page do not.

use std::fs;
use std::path::{Path, PathBuf};

use askama::Template;

use crate::api::GET_HELP_ROUTE;
use crate::help::{GetHelpScreen, get_help_page};
use crate::owner_auth::owner_signin_page;
use crate::quote_page::PageView;

/// Every template served behind the business session (BIZ-047, D-3155, D-3744, D-3890). A new
/// signed-in business page extends `business_layout.html` and joins this list.
const BUSINESS_PAGES: [&str; 19] = [
    "address_change.html",
    "business_actions.html",
    "business_billing.html",
    "business_complaints.html",
    "business_home.html",
    "business_leave.html",
    "business_step_up.html",
    "business_team.html",
    "confirm_pay.html",
    "emergency_test_email.html",
    "get_help.html",
    "job_release.html",
    "job_types.html",
    "not_a_fit.html",
    "notice_log.html",
    "notice_settings.html",
    "phone_approval.html",
    "text_consent.html",
    "why_charged.html",
];

/// The layout that fills the block, the shared base that leaves it empty, and the business help
/// page that links the form in its body (T200, D-4293).
const LAYOUT: &str = "business_layout.html";
const BASE: &str = "base.html";
const BUSINESS_HELP: &str = "business_help.html";

/// The site's 404 page, a public site page rendered here through `askama.toml`'s second folder,
/// since `portals` may not call `site` (D-3890).
#[derive(Template)]
#[template(path = "not_found.html")]
struct SitePage {
    banner_shown: bool,
}

/// Every `.html` template in both folders, with its file name.
fn templates() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dirs = [root.join("templates"), root.join("../site/templates")];
    let mut out = Vec::new();
    for dir in dirs {
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
            .expect("template folder")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "html"))
            .collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            out.push((name, fs::read_to_string(&path).expect("read")));
        }
    }
    out
}

/// The ways a template can link the Get help form or fill the header block.
fn links_get_help(body: &str) -> usize {
    let marks = [
        format!("href=\"{GET_HELP_ROUTE}\""),
        "href=\"{{ crate::api::GET_HELP_ROUTE }}\"".to_owned(),
        "data-get-help".to_owned(),
        "block get_help".to_owned(),
    ];
    marks.iter().map(|m| body.matches(m.as_str()).count()).sum()
}

/// TD-BIZ-23 (BIZ-047, D-3155): exactly the listed business pages extend the business layout.
#[test]
fn td_biz_23_only_business_pages_extend_the_layout() {
    let extends = format!("{{% extends \"{LAYOUT}\" %}}");
    let mut found: Vec<String> = templates()
        .into_iter()
        .filter(|(_, body)| body.contains(&extends))
        .map(|(name, _)| name)
        .collect();
    found.sort();
    assert_eq!(found, BUSINESS_PAGES.map(str::to_owned).to_vec());
}

/// TD-BIZ-23 (BIZ-047, D-3155, D-4293): `base.html`'s block is empty, only the layout fills it,
/// and the business help page is the one other template that links the form, once.
#[test]
fn td_biz_23_no_other_template_links_get_help() {
    for (name, body) in templates() {
        let count = links_get_help(&body);
        match name.as_str() {
            BASE => assert!(
                body.contains("{% block get_help %}{% endblock %}"),
                "{name}"
            ),
            LAYOUT => assert!(count > 0, "{name}"),
            BUSINESS_HELP => assert_eq!(body.matches("href=\"/b/help\"").count(), 1, "{name}"),
            _ => assert_eq!(count, 0, "{name} links Get help"),
        }
    }
    assert_eq!(GET_HELP_ROUTE, "/b/help");
}

/// TD-BIZ-23 (BIZ-047): a rendered business page holds the link; the homeowner's quote page,
/// the owner sign-in page and a public site page do not.
#[test]
fn td_biz_23_rendered_pages() {
    let link = format!("<a href=\"{GET_HELP_ROUTE}\" data-get-help>Get help</a>");
    let business = get_help_page("fakecsrf", GetHelpScreen::Ask).expect("business page");
    assert!(business.contains(&link));
    let quote = PageView {
        request: 1042,
        still_open: false,
        live: true,
        pick_open: true,
        csrf: "fakecsrf".to_owned(),
        quotes: Vec::new(),
    }
    .render()
    .expect("quote page");
    let owner = owner_signin_page("fakecsrf", false, false).expect("owner page");
    let site = SitePage {
        banner_shown: false,
    }
    .render()
    .expect("site page");
    for (page, html) in [("quote", quote), ("owner", owner), ("site", site)] {
        assert!(!html.contains("data-get-help"), "{page}");
        assert!(
            !html.contains(&format!("href=\"{GET_HELP_ROUTE}\"")),
            "{page}"
        );
    }
}
