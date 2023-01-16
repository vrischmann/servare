use crate::domain::UserId;
use crate::feed::{find_feed, get_all_feeds, get_feed_favicon, insert_feed};
use crate::feed::{Feed, FeedId, FindError, FoundFeed, ParseError};
use crate::job::{add_fetch_favicon_job, add_refresh_feed_job};
use crate::routes::FEEDS_PAGE;
use crate::routes::{e500, get_user_id_or_redirect, see_other};
use crate::sessions::TypedSession;
use crate::telemetry::spawn_blocking_with_tracing;
use crate::{error_chain_fmt, fetch_bytes};
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::web::{Data as WebData, Form as WebForm, Path as WebPath};
use actix_web::HttpResponse;
use actix_web_flash_messages::{FlashMessage, IncomingFlashMessages};
use anyhow::{anyhow, Context};
use askama::Template;
use serde::Deserialize;
use sqlx::PgPool;
use std::fmt;
use tracing::{event, warn, Level};
use url::Url;

#[derive(askama::Template)]
#[template(path = "feeds.html.j2")]
struct FeedsTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
    pub feeds: Vec<FeedForTemplate>,
}

struct FeedForTemplate {
    original: Feed,
    site_link: Option<Url>,
    has_favicon: bool,
}

#[tracing::instrument(
    name = "Feeds",
    skip(pool, session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_feeds(
    pool: WebData<PgPool>,
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = get_user_id_or_redirect(&session)?;

    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    //

    // TODO(vincent): can we handle this better ?
    let original_feeds = get_all_feeds(pool.as_ref(), &user_id).await.map_err(e500)?;

    let feeds = original_feeds
        .into_iter()
        .map(|feed| FeedForTemplate {
            site_link: feed.site_link_as_url(),
            has_favicon: feed.site_favicon.is_some(),
            original: feed,
        })
        .collect();

    //

    let tpl = FeedsTemplate {
        page: FEEDS_PAGE,
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
    pool: WebData<PgPool>,
    http_client: WebData<reqwest::Client>,
    session: TypedSession,
    form_data: WebForm<FeedAddFormData>,
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

    insert_feed(&pool, &user_id, &feed)
        .await
        .map_err(Into::<anyhow::Error>::into)
        .context("unable to save feed")
        .map_err(Into::<FeedAddError>::into)
        .map_err(feeds_page_redirect)?;

    // 4) Add needed background jobs
    //
    // Note we don't fail if this returns an error, it's only a backgroun job
    if let Err(err) = add_fetch_favicon_job(pool.as_ref(), feed.id, &feed.site_link).await {
        warn!(%err, "unable to add fetch favicon job");
    }

    Ok(see_other("/feeds"))
}

#[derive(askama::Template)]
#[template(path = "feeds_add.html.j2")]
struct FeedsAddTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
}

#[tracing::instrument(
    name = "Feeds add form",
    skip(session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_feeds_add_form(
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = get_user_id_or_redirect(&session)?;

    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    let tpl = FeedsAddTemplate {
        page: FEEDS_PAGE,
        user_id: Some(user_id),
        flash_messages,
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

#[derive(thiserror::Error)]
pub enum FeedRefreshError {
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

impl fmt::Debug for FeedRefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Feeds refresh",
    skip(pool, session),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_feeds_refresh(
    pool: WebData<PgPool>,
    session: TypedSession,
) -> Result<HttpResponse, InternalError<FeedRefreshError>> {
    let user_id = get_user_id_or_redirect(&session)?;

    // Iterate over all feeds and add a refresh job for it

    let mut tx = pool
        .begin()
        .await
        .map_err(Into::<anyhow::Error>::into)
        .map_err(FeedRefreshError::Unexpected)
        .map_err(feeds_page_redirect)?;

    let feeds = get_all_feeds(&mut tx, &user_id)
        .await
        .map_err(FeedRefreshError::Unexpected)
        .map_err(feeds_page_redirect)?;

    for feed in feeds {
        add_refresh_feed_job(pool.as_ref(), feed.id, feed.url)
            .await
            .map_err(FeedRefreshError::Unexpected)
            .map_err(feeds_page_redirect)?;
    }

    tx.commit()
        .await
        .map_err(Into::<anyhow::Error>::into)
        .map_err(FeedRefreshError::Unexpected)
        .map_err(feeds_page_redirect)?;

    // Done, redirect to the feed list

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/feeds"))
        .finish();

    Ok(response)
}

#[tracing::instrument(
    name = "Feed favicon",
    skip(pool, session, feed_id),
    fields(
        user_id = tracing::field::Empty,
        feed_id = tracing::field::Empty,
    )
)]
pub async fn handle_feed_favicon(
    pool: WebData<PgPool>,
    session: TypedSession,
    feed_id: WebPath<FeedId>,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = get_user_id_or_redirect(&session)?;
    let feed_id = feed_id.into_inner();

    tracing::Span::current()
        .record("user_id", &tracing::field::display(&user_id))
        .record("feed_id", &tracing::field::display(&feed_id));

    let favicon = get_feed_favicon(&pool, &user_id, &feed_id)
        .await
        .map_err(e500)?;

    if let Some(favicon) = favicon {
        let response = HttpResponse::Ok()
            .content_type("image/x-icon")
            .body(favicon);

        Ok(response)
    } else {
        Ok(HttpResponse::NotFound().into())
    }
}

fn feeds_page_redirect<E>(err: E) -> InternalError<E>
where
    E: fmt::Display,
{
    FlashMessage::error(err.to_string()).send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/feeds"))
        .finish();

    InternalError::from_response(err, response)
}
