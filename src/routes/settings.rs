use crate::domain::UserId;
use crate::routes::{e500, get_user_id_or_redirect};
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http::header::ContentType;
use actix_web::HttpResponse;
use actix_web_flash_messages::IncomingFlashMessages;
use askama::Template;

#[derive(askama::Template)]
#[template(path = "settings.html.j2")]
struct SettingsTemplate {
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
}

#[tracing::instrument(
    name = "Settings",
    skip(session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_settings(
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = get_user_id_or_redirect(&session)?;

    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    //

    let tpl = SettingsTemplate {
        user_id: Some(user_id),
        flash_messages,
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
