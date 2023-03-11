use crate::domain::UserId;
use crate::feed::{feed_with_url_exists, find_feed, insert_feed};
use crate::feed::{
    get_all_feeds, get_feed, get_feed_entries, get_feed_entry, get_feed_favicon,
    mark_feed_entry_as_read,
};
use crate::feed::{Feed, FeedId, FindError, FoundFeed, ParseError, ParsedFeed};
use crate::feed::{FeedEntry, FeedEntryId};
use crate::job::{add_fetch_favicon_job, add_refresh_feed_job};
use crate::routes::FEEDS_PAGE;
use crate::routes::{e500, error_redirect, get_user_id_or_redirect, see_other};
use crate::sessions::TypedSession;
use crate::telemetry::spawn_blocking_with_tracing;
use crate::{debug_with_error_chain, fetch_bytes};
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::web::{Data as WebData, Form as WebForm, Path as WebPath};
use actix_web::HttpResponse;
use actix_web_flash_messages::{FlashMessage, IncomingFlashMessages};
use anyhow::Context;
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

impl FeedForTemplate {
    fn new(feed: Feed) -> Self {
        Self {
            site_link: feed.site_link_as_url(),
            has_favicon: feed.site_favicon.is_some(),
            original: feed,
        }
    }
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
    pub url: String,
}

#[derive(thiserror::Error)]
pub enum FeedAddError {
    #[error("Did not find a valid feed")]
    NoFeed(#[source] FindError),
    #[error("URL is not a valid RSS feed")]
    URLNotAValidRSSFeed(#[from] ParseError),
    #[error("URL is inaccessible")]
    URLInaccessible(#[source] reqwest::Error),
    #[error("URL is invalid")]
    URLInvalid(#[source] url::ParseError),
    #[error("Feed already exists")]
    FeedAlreadyExists,
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

debug_with_error_chain!(FeedAddError);

fn guess_url(url: String) -> Result<Url, url::ParseError> {
    if url.starts_with("https://") || url.starts_with("http://") {
        return Url::parse(&url);
    }

    if url.starts_with("localhost") || url.starts_with("127.0.0.1") {
        Url::parse(&["http://", &url].concat())
    } else {
        Url::parse(&["https://", &url].concat())
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

    // The URL might not have a scheme, try to guess it

    let original_url = guess_url(form_data.0.url)
        .map_err(FeedAddError::URLInvalid)
        .map_err(feeds_page_redirect)?;

    //

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

            ParsedFeed::parse(&url, &response_bytes[..])
                .map_err(FeedAddError::URLNotAValidRSSFeed)
                .map_err(feeds_page_redirect)?
        }
        FoundFeed::Raw(raw_feed) => {
            event!(Level::INFO, "original URL was a RSS feed");

            ParsedFeed::from_raw_feed(&original_url, raw_feed)
        }
    };

    event!(Level::INFO,
        title = %feed.title,
        site_link = %feed.site_link,
        "Fetched feed",
    );

    // 3) Check if the feed already exists

    let feed_exists = feed_with_url_exists(pool.as_ref(), &user_id, &feed.url)
        .await
        .map_err(FeedAddError::Unexpected)
        .map_err(feeds_page_redirect)?;
    if feed_exists {
        return Err(feeds_page_redirect(FeedAddError::FeedAlreadyExists));
    }

    // 4) Insert the feed

    let feed_id = insert_feed(&pool, &user_id, &feed)
        .await
        .map_err(Into::<anyhow::Error>::into)
        .context("unable to save feed")
        .map_err(Into::<FeedAddError>::into)
        .map_err(feeds_page_redirect)?;

    // 5) Add needed background jobs
    //
    // Note we don't fail if these return an error, it's only a backgroun job

    if let Err(err) = add_fetch_favicon_job(pool.as_ref(), feed_id, &feed.site_link).await {
        warn!(%err, "unable to add fetch favicon job");
    }
    if let Err(err) = add_refresh_feed_job(pool.as_ref(), &user_id, feed_id, feed.url).await {
        warn!(%err, "unable to add refresh feed job");
    }

    FlashMessage::success("Found a feed").send();

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

debug_with_error_chain!(FeedRefreshError);

/// This is the /feeds/refresh handler.
///
/// Adds a refresh feed job for every feed.
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
        add_refresh_feed_job(pool.as_ref(), &user_id, feed.id, feed.url)
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

    FlashMessage::success("Refresh started").send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/feeds"))
        .finish();

    Ok(response)
}

/// This is the /feeds/:feed_id/favicon handler.
///
/// It serves the feed's favicon data.
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

// TODO(vincent): this is duplicated code, refactor it

struct FeedEntryForTemplate {
    original: FeedEntry,
    created_at: String,
    author: String,
}

impl FeedEntryForTemplate {
    fn new(original: FeedEntry) -> Self {
        // TODO(vincent): this is ugly, can we replace the unwrap() ?
        let created_at = original
            .created_at
            .replace_nanosecond(0_000_000)
            .unwrap()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()); // TODO(vincent): can this really fail ?

        let author = original.authors.first().cloned().unwrap_or_default();

        Self {
            original,
            created_at,
            author,
        }
    }
}

#[derive(askama::Template)]
#[template(path = "feed_entries.html.j2")]
struct FeedEntriesTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
    pub feed: FeedForTemplate,
    pub entries: Vec<FeedEntryForTemplate>,
}

#[derive(thiserror::Error)]
pub enum FeedEntriesError {
    #[error("Feed not found")]
    NotFound,
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

debug_with_error_chain!(FeedEntriesError);

#[tracing::instrument(
    name = "Feed entries",
    skip(pool, session, flash_messages, feed_id),
    fields(
        user_id = tracing::field::Empty,
        feed_id = tracing::field::Empty,
    )
)]
pub async fn handle_feed_entries(
    pool: WebData<PgPool>,
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
    feed_id: WebPath<FeedId>,
) -> Result<HttpResponse, InternalError<FeedEntriesError>> {
    let user_id = get_user_id_or_redirect(&session)?;
    let feed_id = feed_id.into_inner();

    tracing::Span::current()
        .record("user_id", &tracing::field::display(&user_id))
        .record("feed_id", &tracing::field::display(&feed_id));

    // NOTE(vincent): do we need a transaction here since we don't write anything ?
    let mut tx = {
        let tx_begin_span = tracing::span!(Level::TRACE, "tx_begin");
        let _guard = tx_begin_span.enter();

        pool.begin()
            .await
            .map_err(Into::<anyhow::Error>::into)
            .map_err(FeedEntriesError::Unexpected)
            .map_err(e500)?
    };

    // 1) Get the feed data

    let feed = get_feed(&mut tx, &user_id, &feed_id)
        .await
        .map_err(FeedEntriesError::Unexpected)
        .map_err(feeds_page_redirect)?;

    let feed = feed
        .ok_or(FeedEntriesError::NotFound)
        .map_err(feeds_page_redirect)?;

    // 2) Get the feed entries

    let raw_entries = get_feed_entries(&mut tx, &user_id, &feed_id)
        .await
        .map_err(FeedEntriesError::Unexpected)
        .map_err(feeds_page_redirect)?;

    let entries = raw_entries
        .into_iter()
        .map(FeedEntryForTemplate::new)
        .collect();

    // Render

    let tpl = FeedEntriesTemplate {
        page: FEEDS_PAGE,
        user_id: Some(user_id),
        flash_messages,
        feed: FeedForTemplate::new(feed),
        entries,
    };
    let tpl_rendered = tpl
        .render()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(FeedEntriesError::Unexpected)
        .map_err(e500)?;

    let response = HttpResponse::Ok()
        .content_type(http::header::ContentType::html())
        .body(tpl_rendered);

    Ok(response)
}

//
// Feed entry: /feeds/:feed_id/entries/:entry_id
//

#[derive(askama::Template)]
#[template(path = "feed_entry.html.j2")]
struct FeedEntryTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
    pub feed: FeedForTemplate,
    pub entry: FeedEntryForTemplate,
}

#[derive(thiserror::Error)]
pub enum FeedEntryError {
    #[error("Feed not found")]
    FeedNotFound,
    #[error("Entry not found")]
    EntryNotFound,
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

debug_with_error_chain!(FeedEntryError);

#[tracing::instrument(
    name = "Feed entry",
    skip(pool, session, flash_messages, route_params),
    fields(
        user_id = tracing::field::Empty,
        feed_id = tracing::field::Empty,
        entry_id = tracing::field::Empty,
    )
)]
pub async fn handle_feed_entry(
    pool: WebData<PgPool>,
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
    route_params: WebPath<(FeedId, FeedEntryId)>,
) -> Result<HttpResponse, InternalError<FeedEntryError>> {
    let user_id = get_user_id_or_redirect(&session)?;
    let feed_id = route_params.0;
    let entry_id = route_params.1;

    tracing::Span::current()
        .record("user_id", &tracing::field::display(&user_id))
        .record("feed_id", &tracing::field::display(&feed_id))
        .record("entry_id", &tracing::field::display(&entry_id));

    let mut tx = {
        let tx_begin_span = tracing::span!(Level::TRACE, "tx_begin");
        let _guard = tx_begin_span.enter();

        pool.begin()
            .await
            .map_err(Into::<anyhow::Error>::into)
            .map_err(FeedEntryError::Unexpected)
            .map_err(e500)?
    };

    // 1) Get the feed data

    let feed = get_feed(&mut tx, &user_id, &feed_id)
        .await
        .map_err(FeedEntryError::Unexpected)
        .map_err(feeds_page_redirect)?;

    let feed = feed
        .ok_or(FeedEntryError::FeedNotFound)
        .map_err(feeds_page_redirect)?;

    // 1) Get the feed entry

    let entry = get_feed_entry(&mut tx, &user_id, &feed_id, &entry_id)
        .await
        .map_err(FeedEntryError::Unexpected)
        .map_err(|err| feed_page_redirect(err, feed_id))?;

    let entry = entry
        .ok_or(FeedEntryError::EntryNotFound)
        .map_err(|err| feed_page_redirect(err, feed_id))?;

    // 2) Set its read date

    mark_feed_entry_as_read(&mut tx, &user_id, &feed_id, &entry_id)
        .await
        .map_err(FeedEntryError::Unexpected)
        .map_err(|err| feed_page_redirect(err, feed_id))?;

    tx.commit()
        .await
        .map_err(Into::<anyhow::Error>::into)
        .map_err(FeedEntryError::Unexpected)
        .map_err(|err| feed_page_redirect(err, feed_id))?;

    // Render

    let tpl = FeedEntryTemplate {
        page: FEEDS_PAGE,
        user_id: Some(user_id),
        flash_messages,
        feed: FeedForTemplate::new(feed),
        entry: FeedEntryForTemplate::new(entry),
    };
    let tpl_rendered = tpl
        .render()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(FeedEntryError::Unexpected)
        .map_err(e500)?;

    let response = HttpResponse::Ok()
        .content_type(http::header::ContentType::html())
        .body(tpl_rendered);

    Ok(response)
}

fn feeds_page_redirect<E: fmt::Display>(err: E) -> InternalError<E> {
    error_redirect(err, "/feeds")
}

fn feed_page_redirect<E: fmt::Display>(err: E, feed_id: FeedId) -> InternalError<E> {
    let location = format!("/feeds/{}/entries", feed_id);
    error_redirect(err, &location)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess_uri_should_work_with_and_without_a_scheme() {
        let url1 = guess_url("http://127.0.0.1/foo".to_string()).unwrap();
        let url2 = guess_url("127.0.0.1/foo".to_string()).unwrap();
        assert_eq!(url1, url2);

        let url1 = guess_url("http://localhost/foo".to_string()).unwrap();
        let url2 = guess_url("localhost/foo".to_string()).unwrap();
        assert_eq!(url1, url2);

        let url1 = guess_url("https://example.com/foo".to_string()).unwrap();
        let url2 = guess_url("example.com/foo".to_string()).unwrap();
        assert_eq!(url1, url2);
    }
}
