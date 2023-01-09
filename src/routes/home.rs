use crate::domain::UserId;
use crate::routes::e500;
use crate::routes::HOME_PAGE;
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http::header::ContentType;
use actix_web::HttpResponse;
use actix_web_flash_messages::IncomingFlashMessages;
use askama::Template;

#[derive(askama::Template)]
#[template(path = "home.html.j2")]
struct HomeTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
}

#[tracing::instrument(
    name = "Home",
    skip(session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_home(
    session: TypedSession,
    flash_messages: IncomingFlashMessages,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = session
        .get_user_id()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(e500)?;
    if let Some(ref user_id) = user_id {
        tracing::Span::current().record("user_id", &tracing::field::display(user_id));
    }

    //

    let tpl = HomeTemplate {
        page: HOME_PAGE,
        user_id,
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
