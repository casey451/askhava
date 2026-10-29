//! Throwaway spike tests (D-476): the database and the template engine work in `Ci`.

use askama::Template;

/// The CI Postgres service answers through SQLx.
#[tokio::test]
async fn database_answers() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is set in Ci");
    assert_eq!(core_spike::ping(&url).await.expect("database reachable"), 1);
}

/// Askama escapes what it renders.
#[test]
fn template_escapes() {
    let page = core_spike::Hello { name: "<b>" }.render().expect("renders");
    assert_eq!(page, "<p>&#60;b&#62;</p>");
}
