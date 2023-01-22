use crate::helpers::{assert_is_redirect_to, spawn_app};
use crate::helpers::{LoginBody, TestData};
use select::document::Document;
use select::predicate::Class;
use serde::Serialize;
use url::Url;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Serialize)]
struct AddFeedBody {
    pub url: Url,
}

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

    // Setup a mock server that:
    // * responds with a test XML feed on /feed1
    // * responds with a basic HTML page containing a link to a RSS feed

    let mock_server = MockServer::start().await;
    let mock_uri = mock_server.uri();
    let mock_url = Url::parse(&mock_uri).unwrap();

    Mock::given(path("/feed1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            TestData::get("tailscale_rss_feed.xml").unwrap().data,
            "application/xml",
        ))
        .expect(2)
        .mount(&mock_server)
        .await;

    const HTML: &str = r#"
        <link type="application/rss+xml" href="/feed1">
        "#;

    Mock::given(path("/feed2"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(HTML, "text/html"))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create two feeds

    let feed1_url = mock_url.join("/feed1").unwrap();
    let feed2_url = mock_url.join("/feed2").unwrap();

    let urls = vec![feed1_url, feed2_url];
    for url in urls {
        let body = AddFeedBody { url };

        let response = app.post("/feeds/add", &body).await;
        assert_is_redirect_to(&response, "/feeds");
    }

    // Fetch the feeds page and check the content

    let response = app.get_html("/feeds").await;
    assert!(response.contains("Found a feed"));

    let document = Document::from_read(response.as_bytes()).unwrap();
    let feed_cards = document.find(Class("feed-card")).count();

    assert_eq!(2, feed_cards);
}

#[tokio::test]
async fn settings_page_should_redirect_if_not_logged_in() {
    // Setup
    let app = spawn_app().await;

    // Fetch the settings page
    let response = app.get("/settings").await;
    assert_is_redirect_to(&response, "/login");
}
