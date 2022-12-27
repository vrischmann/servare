use crate::domain::UserId;
use crate::routes::e500;
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http::header::ContentType;
use actix_web::HttpResponse;
use askama::Template;

#[derive(askama::Template)]
#[template(path = "home.html.j2")]
struct HomeTemplate {
    pub user_id: Option<UserId>,
}

#[tracing::instrument(name = "Home", skip(session))]
pub async fn home(session: TypedSession) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = session
        .get_user_id()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(e500)?;

    let tpl = HomeTemplate { user_id };
    let tpl_rendered = tpl
        .render()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(e500)?;

    let response = HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(tpl_rendered);

    Ok(response)
}
