use crate::helpers::{parse_url, testdata};
use servare::feed::fetch_feed;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fetch_feed_should_work() {
    let mock_server = MockServer::start().await;
    let mock_uri = mock_server.uri();

    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(testdata("tailscale_rss_feed.xml"), "application/xml"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    let feed = fetch_feed(&client, &parse_url(mock_uri)).await.unwrap();
    assert_eq!("Blog on Tailscale", feed.title);
    assert_eq!("https://tailscale.com/blog/", feed.site_link);
    assert_eq!("Recent content in Blog on Tailscale", feed.description);
}
