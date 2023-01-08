use crate::helpers::{fetch, parse_url, testdata};
use servare::feed::{find_feed, Feed, FoundFeed};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn find_feed_should_work() {
    let mock_server = MockServer::start().await;
    let mock_uri = mock_server.uri();
    let mock_url = parse_url(mock_uri);

    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(testdata("tailscale_rss_feed.xml"), "application/xml"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let data = fetch(&mock_url).await;
    let found_feed = find_feed(&mock_url, &data[..]).unwrap();

    let feed = match found_feed {
        FoundFeed::Raw(raw_feed) => Feed::from_raw_feed(&mock_url, raw_feed),
        FoundFeed::Url(_) => panic!("expected a FoundFeed::Raw"),
    };

    assert_eq!("Blog on Tailscale", feed.title);
    assert_eq!("https://tailscale.com/blog/", feed.site_link);
    assert_eq!("Recent content in Blog on Tailscale", feed.description);
}
