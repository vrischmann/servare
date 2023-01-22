use crate::configuration::get_configuration;
use crate::domain::{UserEmail, UserId};
use crate::feed::{insert_feed, Feed, FeedId};
use crate::startup::get_connection_pool;
use fake::faker::internet::en::{Password as FakerPassword, SafeEmail as FakerSafeEmail};
use fake::faker::lorem::en::{Paragraph as FakerParagraph, Sentence as FakerSentence};
use fake::Fake;
use secrecy::Secret;
use sqlx::PgPool;
use url::Url;

/// Get a connection pool suitable for tests
///
/// # Panics
///
/// Panics if:
/// * the configuration is invalid somehow.
/// * a connection pool can't be created.
pub async fn get_pool() -> PgPool {
    let config = get_configuration().unwrap();
    get_connection_pool(&config.database).await.unwrap()
}

/// Creates a basic [`reqwest::Client`] suitable for tests.
///
/// # Panics
///
/// Panics if the client can't be created.
pub fn get_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .expect("Failed to build HTTP client")
}

pub async fn fetch(url: &Url) -> bytes::Bytes {
    let client = reqwest::Client::new();

    let response = client.get(url.to_string()).send().await.unwrap();
    let response_body = response.bytes().await.unwrap();

    response_body
}

/// Create a user suitable for testing.
///
/// # Panics
///
/// Panics if any step in the user creation fail.
pub async fn create_user(pool: &PgPool) -> UserId {
    let email = FakerSafeEmail().fake();
    let password = FakerPassword(10..20).fake();

    let user_id =
        crate::authentication::create_user(pool, &UserEmail(email), Secret::new(password))
            .await
            .expect("unable to create user");

    user_id
}

/// Create a test feed for the user [`user_id`] with the site link [`site_link`].
///
/// # Panics
///
/// Panics if any step in the user creation fail.
pub async fn create_feed(pool: &PgPool, user_id: &UserId, url: &Url, site_link: &Url) -> FeedId {
    let title = FakerSentence(4..15).fake();
    let description = FakerParagraph(1..40).fake();

    let feed = Feed {
        id: FeedId::default(),
        url: url.clone(),
        title,
        site_link: site_link.to_string(),
        description,
        site_favicon: None,
        added_at: time::OffsetDateTime::now_utc(),
    };

    insert_feed(pool, user_id, &feed).await.unwrap();

    feed.id
}
