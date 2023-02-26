use crate::domain::UserId;
use crate::error_chain_fmt;
use crate::feed::get_unread_entries;
use crate::feed::FeedEntry;
use crate::routes::{e500, get_user_id_or_redirect, UNREAD_PAGE};
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::web::Data as WebData;
use actix_web::HttpResponse;
use actix_web_flash_messages::IncomingFlashMessages;
use askama::Template;
use sqlx::PgPool;
use std::fmt;

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
#[template(path = "unread.html.j2")]
struct UnreadTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
    pub entries: Vec<FeedEntryForTemplate>,
}

#[derive(thiserror::Error)]
pub enum UnreadError {
    #[error("Something went wrong")]
    Unexpected(#[from] anyhow::Error),
}

impl fmt::Debug for UnreadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Unread",
    skip(pool, session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_unread(
    pool: WebData<PgPool>,
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
) -> Result<HttpResponse, InternalError<UnreadError>> {
    let user_id = get_user_id_or_redirect(&session)?;

    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    // Fetch the unread entries

    let original_feed_entries = get_unread_entries(pool.as_ref(), &user_id)
        .await
        .map_err(UnreadError::Unexpected)
        .map_err(e500)?;

    let feed_entries = original_feed_entries
        .into_iter()
        .map(|feed_entry| FeedEntryForTemplate::new(feed_entry))
        .collect();

    // Render

    let tpl = UnreadTemplate {
        page: UNREAD_PAGE,
        user_id: Some(user_id),
        flash_messages,
        entries: feed_entries,
    };
    let tpl_rendered = tpl
        .render()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(UnreadError::Unexpected)
        .map_err(e500)?;

    let response = HttpResponse::Ok()
        .content_type(http::header::ContentType::html())
        .body(tpl_rendered);

    Ok(response)
}
