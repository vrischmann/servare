use crate::domain::UserId;
use crate::error_chain_fmt;
use crate::feed::{find_feed, get_all_feeds, insert_feed, Feed, FindError, FoundFeed, ParseError};
use crate::routes::{e500, get_user_id_or_redirect, see_other};
use crate::sessions::TypedSession;
use crate::telemetry::spawn_blocking_with_tracing;
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::web;
use actix_web::HttpResponse;
use actix_web_flash_messages::{FlashMessage, IncomingFlashMessages};
use anyhow::Context;
use askama::Template;
use bytes::Bytes;
use serde::Deserialize;
use sqlx::PgPool;
use std::fmt;
use tracing::{event, Level};
use url::Url;

#[derive(askama::Template)]
#[template(path = "feeds.html.j2")]
struct FeedsTemplate {
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
    pub feeds: Vec<Feed>,
}

#[tracing::instrument(
    name = "Feeds",
    skip(pool, session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_feeds(
    pool: web::Data<PgPool>,
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = get_user_id_or_redirect(&session)?;

    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    //

    // TODO(vincent): can we handle this better ?
    let feeds = get_all_feeds(&pool, &user_id).await.map_err(e500)?;

    let tpl = FeedsTemplate {
        user_id: Some(user_id),
        flash_messages,
        feeds,
    };
    let tpl_rendered = tpl
        .render()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(e500)?;

    let response = HttpResponse::Ok()
        .content_type(http::header::ContentType::html())
        .body(tpl_rendered);

    Ok(response)
}

#[derive(Deserialize)]
pub struct FeedAddFormData {
    pub url: Url,
}

#[derive(thiserror::Error)]
pub enum FeedAddError {
    #[error("Did not find a valid feed")]
    NoFeed(#[source] FindError),
    #[error("URL is not a valid RSS feed")]
    URLNotAValidRSSFeed(#[from] ParseError),
    #[error("URL is inaccessible")]
    URLInaccessible(#[source] reqwest::Error),
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

impl fmt::Debug for FeedAddError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// This is the handler for /feeds/add.
/// Its job is to:
/// * find a feed for a given URL
/// * if one is found, fetch its information
/// * store it in the database
///
/// Thus the URL can either be a RSS or Atom feed or a website
/// containing a link to such a feed.
///
/// # Errors
///
/// This function will return an error if .
#[tracing::instrument(
    name = "Add feed",
    skip(pool, http_client, session, form_data),
    fields(
        user_id = tracing::field::Empty,
        url = tracing::field::Empty,
        feed_url = tracing::field::Empty,
    )
)]
pub async fn handle_feeds_add(
    pool: web::Data<PgPool>,
    http_client: web::Data<reqwest::Client>,
    session: TypedSession,
    form_data: web::Form<FeedAddFormData>,
) -> Result<HttpResponse, InternalError<FeedAddError>> {
    let user_id = get_user_id_or_redirect(&session)?;

    let original_url = form_data.0.url;

    tracing::Span::current()
        .record("user_id", &tracing::field::display(&user_id))
        .record("url", &tracing::field::display(&original_url));

    // 1) Fetch the data at the URL
    // We don't know yet if it's a website or a straight-up feed.

    let response_bytes = fetch_bytes(&http_client, &original_url)
        .await
        .map_err(FeedAddError::URLInaccessible)
        .map_err(feeds_page_redirect)?;

    // 1) Find the feed
    //
    // Note we spawn a blocking task to avoid taking too much time parsing the data

    // TODO(vincent): how can we avoid a clone here ?
    let find_feed_url = original_url.clone();

    let found_feed_result =
        spawn_blocking_with_tracing(move || find_feed(&find_feed_url, &response_bytes[..]))
            .await
            .context("Failed to spawn blocking task")
            .map_err(Into::<anyhow::Error>::into)
            .map_err(FeedAddError::Unexpected)
            .map_err(feeds_page_redirect)?;
    let found_feed = found_feed_result
        .map_err(FeedAddError::NoFeed)
        .map_err(feeds_page_redirect)?;

    // 2) Process the result

    let feed = match found_feed {
        FoundFeed::Url(url) => {
            event!(Level::INFO,
                url = %url,
                "original URL was a HTML document containing a RSS feed URL",
            );

            let response_bytes = fetch_bytes(&http_client, &url)
                .await
                .map_err(FeedAddError::URLInaccessible)
                .map_err(feeds_page_redirect)?;

            Feed::parse(&url, &response_bytes[..])
                .map_err(FeedAddError::URLNotAValidRSSFeed)
                .map_err(feeds_page_redirect)?
        }
        FoundFeed::Raw(raw_feed) => {
            event!(Level::INFO, "original URL was a RSS feed");

            Feed::from_raw_feed(&original_url, raw_feed)
        }
    };

    event!(Level::INFO,
        title = %feed.title,
        site_link = %feed.site_link,
        "Fetched feed",
    );

    // 3) Insert the feed

    insert_feed(&pool, &user_id, feed)
        .await
        .map_err(Into::<anyhow::Error>::into)
        .context("unable to save feed")
        .map_err(Into::<FeedAddError>::into)
        .map_err(feeds_page_redirect)?;

    Ok(see_other("/feeds"))
}

fn feeds_page_redirect(err: FeedAddError) -> InternalError<FeedAddError> {
    FlashMessage::error(err.to_string()).send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/feeds"))
        .finish();

    InternalError::from_response(err, response)
}

/// Fetches the content of a URL directly as a bytes buffer.
///
/// # Errors
///
/// This function will return an error if the fetch fails.
async fn fetch_bytes(client: &reqwest::Client, url: &Url) -> Result<Bytes, reqwest::Error> {
    let response = client.get(url.to_string()).send().await?;
    let response_bytes = response.bytes().await?;

    Ok(response_bytes)
}
