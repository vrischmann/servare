use crate::domain::UserId;
use crate::error_chain_fmt;
use crate::feed::{fetch_feed, get_all_feeds, insert_feed, Feed, FetchError};
use crate::routes::{e500, get_user_id_or_redirect2, see_other};
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::web;
use actix_web::HttpResponse;
use actix_web_flash_messages::{FlashMessage, IncomingFlashMessages};
use anyhow::Context;
use askama::Template;
use serde::Deserialize;
use sqlx::PgPool;
use std::fmt;
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
    let user_id = get_user_id_or_redirect2(&session)?;

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
    #[error("URL is not a valid feed")]
    URLNotAValidFeed(#[source] FetchError),
    #[error("URL is inaccessible")]
    URLInaccessible(#[source] FetchError),
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

impl fmt::Debug for FeedAddError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Add feed",
    skip(pool, session, form_data),
    fields(
        user_id = tracing::field::Empty,
        url = tracing::field::Empty,
    )
)]
pub async fn handle_feeds_add(
    pool: web::Data<PgPool>,
    http_client: web::Data<reqwest::Client>,
    session: TypedSession,
    form_data: web::Form<FeedAddFormData>,
) -> Result<HttpResponse, InternalError<FeedAddError>> {
    let user_id = get_user_id_or_redirect2(&session)?;

    let feed_url = form_data.0.url;

    let feed = match fetch_feed(&http_client, &feed_url).await {
        Ok(feed) => feed,
        Err(err) => {
            let err = match err {
                FetchError::InvalidURL(_) | FetchError::RSS(_) => {
                    FeedAddError::URLNotAValidFeed(err)
                }
                FetchError::Reqwest(_) => FeedAddError::URLInaccessible(err),
                FetchError::Unexpected(_) => FeedAddError::Unexpected(err.into()),
            };

            return Err(feeds_page_redirect(err));
        }
    };

    tracing::Span::current()
        .record("user_id", &tracing::field::display(&user_id))
        .record("url", &tracing::field::display(&feed_url));

    insert_feed(&pool, &user_id, feed)
        .await
        .map_err(Into::<anyhow::Error>::into)
        .context("unable to save feed")
        .map_err(Into::<FeedAddError>::into)
        .map_err(e500)?;

    Ok(see_other("/feeds"))
}

fn feeds_page_redirect(err: FeedAddError) -> InternalError<FeedAddError> {
    FlashMessage::error(err.to_string()).send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/feeds"))
        .finish();

    InternalError::from_response(err, response)
}
