//! The owner's step-up page and the owner door's own paths (SEC-043, SEC-048; T303, D-2128,
//! D-3878).
//!
//! [`owner_step_up_page`] renders `templates/owner_step_up.html` for the signed-in owner: a form
//! that takes a fresh 6-digit code from the authenticator app (the step-up `OwnerAction`s need one
//! within the last 5 minutes, SEC-043), then the "Sign out" and "Sign out everywhere" buttons. The
//! page never links to the owner sign-in page, not even after a wrong code: the door probe reads a
//! 4xx that links there as the owner door's refusal (`app/server/tests/door_matrix.rs`, D-3878).
//! [`owner_signed_out`] is that refusal: the one answer every owner route gives to no session, a
//! business or homeowner session, or an ended owner session. `server::owner_signin` serves the
//! routes [`OWNER_STEP_UP_ROUTE`], [`OWNER_SIGN_OUT_ROUTE`] and [`OWNER_SIGN_OUT_ALL_ROUTE`].

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::owner_auth::OWNER_SIGNIN_ROUTE;

/// The step-up form's path: `GET` shows it, `POST` checks the code (SEC-043).
pub const OWNER_STEP_UP_ROUTE: &str = "/owner/step-up";
/// The "Sign out" button's `POST`: ends this browser's owner session (SEC-043).
pub const OWNER_SIGN_OUT_ROUTE: &str = "/owner/sign-out";
/// The "Sign out everywhere" button's `POST`: ends every owner session (SEC-043).
pub const OWNER_SIGN_OUT_ALL_ROUTE: &str = "/owner/sign-out-everywhere";

/// Which state of the step-up page to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepUpScreen {
    /// The form, before any code was sent.
    Ask,
    /// The code was wrong, old or already used: the form again with its error summary.
    Failed,
    /// The code was right: the 5-minute window starts now.
    Done,
}

#[derive(Template)]
#[template(path = "owner_step_up.html")]
struct StepUpView<'a> {
    csrf: &'a str,
    failed: bool,
    done: bool,
    step_up_route: &'a str,
    sign_out_route: &'a str,
    sign_out_all_route: &'a str,
}

/// The owner's step-up page for `screen`, its three forms carrying `csrf` (SEC-042, SEC-043).
///
/// # Errors
///
/// The template's render error.
pub fn owner_step_up_page(csrf: &str, screen: StepUpScreen) -> Result<String, askama::Error> {
    StepUpView {
        csrf,
        failed: screen == StepUpScreen::Failed,
        done: screen == StepUpScreen::Done,
        step_up_route: OWNER_STEP_UP_ROUTE,
        sign_out_route: OWNER_SIGN_OUT_ROUTE,
        sign_out_all_route: OWNER_SIGN_OUT_ALL_ROUTE,
    }
    .render()
}

/// SEC-005, SEC-006, SEC-019: the owner door's refusal, a 401 with one link to the owner sign-in
/// page and nothing else.
#[must_use]
pub fn owner_signed_out() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Sign in</title></head><body><main>\
         <h1>Sign in as the owner</h1><p><a href=\"{OWNER_SIGNIN_ROUTE}\">Sign in</a> to see \
         this page.</p></main></body></html>"
    );
    (StatusCode::UNAUTHORIZED, Html(page)).into_response()
}

#[cfg(test)]
mod tests {
    //! The step-up page's three screens (SEC-043, D-3878).

    use super::*;

    #[test]
    fn no_screen_links_to_the_sign_in_page_and_each_carries_the_token() {
        for screen in [StepUpScreen::Ask, StepUpScreen::Failed, StepUpScreen::Done] {
            let page = owner_step_up_page("tok123", screen).expect("page");
            assert!(!page.contains(OWNER_SIGNIN_ROUTE), "{screen:?}");
            assert_eq!(page.matches("value=\"tok123\"").count(), 3, "{screen:?}");
        }
        let failed = owner_step_up_page("t", StepUpScreen::Failed).expect("page");
        assert!(failed.contains("error-summary") && failed.contains("aria-invalid"));
        let done = owner_step_up_page("t", StepUpScreen::Done).expect("page");
        assert!(done.contains("step-up-done") && !done.contains("error-summary"));
    }

    #[tokio::test]
    async fn the_refusal_links_only_to_the_owner_sign_in() {
        let refused = owner_signed_out();
        assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(refused.into_body(), 1 << 20)
            .await
            .expect("body");
        let page = String::from_utf8(body.to_vec()).expect("utf-8");
        assert_eq!(page.matches("href=").count(), 1, "one link: {page}");
        let link = format!("href=\"{OWNER_SIGNIN_ROUTE}\"");
        assert!(page.contains(&link), "{page}");
    }
}
