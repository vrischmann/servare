use crate::helpers::LoginBody;
use crate::helpers::{assert_is_redirect_to, spawn_app};
use serde::Serialize;
use url::Url;

#[derive(Serialize)]
struct AddFeedBody {
    pub url: Url,
}

static FEED1_URL: &str = "https://example.com/feed1";
static FEED2_URL: &str = "https://example.com/feed2";

#[tokio::test]
async fn feeds_should_be_displayed() {
    // Setup, login
    let app = spawn_app().await;

    let login_body = LoginBody {
        email: app.test_user.email.clone(),
        password: app.test_user.password.clone(),
    };
    let login_response = app.post("/login", &login_body).await;
    assert_is_redirect_to(&login_response, "/");

    // Create two feeds
    let urls = vec![FEED1_URL, FEED2_URL];
    for url in urls {
        let body = AddFeedBody {
            url: Url::parse(url).unwrap(),
        };

        let response = app.post("/feeds/add", &body).await;
        assert_is_redirect_to(&response, "/feeds");
    }

    // Fetch the feeds page and check the content
    let response = app.get_html("/feeds").await;

    assert!(response.contains("Feeds"));
}

#[tokio::test]
async fn settings_page_should_redirect_if_not_logged_in() {
    // Setup
    let app = spawn_app().await;

    // Fetch the settings page
    let response = app.get("/settings").await;
    assert_is_redirect_to(&response, "/login");
}
