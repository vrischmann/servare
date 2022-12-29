use crate::domain::{Feed, FeedId, UserId};
use crate::routes::{e500, get_user_id_or_redirect, see_other};
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http::header::ContentType;
use actix_web::web;
use actix_web::HttpResponse;
use actix_web_flash_messages::IncomingFlashMessages;
use anyhow::Context;
use askama::Template;
use serde::Deserialize;
use sqlx::PgPool;
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
        .content_type(ContentType::html())
        .body(tpl_rendered);

    Ok(response)
}

#[derive(Deserialize)]
pub struct FeedAddFormData {
    pub url: Url,
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
    session: TypedSession,
    form_data: web::Form<FeedAddFormData>,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = get_user_id_or_redirect(&session)?;

    let feed_url = form_data.0.url;

    tracing::Span::current()
        .record("user_id", &tracing::field::display(&user_id))
        .record("url", &tracing::field::display(&feed_url));

    insert_feed(&pool, &user_id, feed_url).await.map_err(e500)?;

    Ok(see_other("/feeds"))
}

/// Create a new feed in the database for this `user_id` with the URL `url`.
#[tracing::instrument(name = "Insert feed", skip(pool))]
async fn insert_feed(pool: &PgPool, user_id: &UserId, url: Url) -> Result<FeedId, anyhow::Error> {
    let id = FeedId::default();

    sqlx::query!(
        r#"
        INSERT INTO feeds(id, user_id, url, created_at)
        VALUES ($1, $2, $3, $4)
        "#,
        &id.0,
        &user_id.0,
        url.to_string(),
        time::OffsetDateTime::now_utc(),
    )
    .execute(pool)
    .await
    .map_err(Into::<anyhow::Error>::into)?;

    Ok(id)
}

#[tracing::instrument(name = "Get all feeds", skip(pool))]
async fn get_all_feeds(pool: &PgPool, user_id: &UserId) -> Result<Vec<Feed>, anyhow::Error> {
    let records = sqlx::query!(
        r#"
        SELECT
            f.id, f.url, f.title, f.site_link, f.description,
            f.created_at, f.last_checked_at
        FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1
        "#,
        &user_id.0,
    )
    .fetch_all(pool)
    .await
    .map_err(Into::<anyhow::Error>::into)?;

    let mut feeds = Vec::new();
    for record in records {
        let url = Url::parse(&record.url)
            .map_err(Into::<anyhow::Error>::into)
            .context("invalid feed URL")?;

        feeds.push(Feed {
            id: FeedId(record.id),
            url,
            title: record.title,
            site_link: record.site_link,
            description: record.description,
            created_at: record.created_at,
            last_checked_at: record.last_checked_at,
        });
    }

    Ok(feeds)
}
