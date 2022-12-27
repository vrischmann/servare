use crate::authentication::{authenticate, AuthError, Credentials};
use crate::domain::{UserEmail, UserId};
use crate::error_chain_fmt;
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::http::header::ContentType;
use actix_web::http::{header, StatusCode};
use actix_web::web;
use actix_web::HttpResponse;
use askama::Template;
use secrecy::Secret;
use sqlx::PgPool;
use std::fmt;
use tracing::{event, Level};

pub fn e500<T>(err: T) -> actix_web::error::InternalError<T>
where
    T: fmt::Debug + fmt::Display + 'static,
{
    actix_web::error::InternalError::new(err, StatusCode::INTERNAL_SERVER_ERROR)
}

// pub fn e400<T>(err: T) -> actix_web::error::InternalError<T>
// where
//     T: fmt::Debug + fmt::Display + 'static,
// {
//     actix_web::error::InternalError::new(err, StatusCode::BAD_REQUEST)
// }

pub fn see_other(location: &str) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header((header::LOCATION, location))
        .finish()
}

pub async fn handle_status() -> HttpResponse {
    HttpResponse::Ok().finish()
}

// Home

#[derive(askama::Template)]
#[template(path = "home.html.j2")]
struct HomeTemplate {
    pub user_id: Option<UserId>,
}

#[tracing::instrument(name = "Home", skip(session))]
pub async fn handle_home(
    session: TypedSession,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
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

// Login

#[derive(askama::Template)]
#[template(path = "login.html.j2")]
struct LoginTemplate {
    pub user_id: Option<UserId>,
}

#[tracing::instrument(name = "Login form", skip(session))]
pub async fn handle_login_form(
    session: TypedSession,
) -> Result<HttpResponse, InternalError<anyhow::Error>> {
    let user_id = session
        .get_user_id()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(e500)?;

    let tpl = LoginTemplate { user_id };
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
pub enum LoginError {
    #[error("Authentication failed")]
    Auth(#[source] anyhow::Error),
    #[error("Something went wrong")]
    Unexpected(#[source] anyhow::Error),
}

impl fmt::Debug for LoginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[derive(serde::Deserialize)]
pub struct LoginFormData {
    pub email: UserEmail,
    pub password: String,
}

#[tracing::instrument(
    name = "Login submit",
    skip(pool, session, form_data),
    fields(
        username = tracing::field::Empty,
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_login_submit(
    pool: web::Data<PgPool>,
    session: TypedSession,
    form_data: web::Form<LoginFormData>,
) -> Result<HttpResponse, InternalError<LoginError>> {
    let pool = &pool;

    tracing::Span::current().record("email", &tracing::field::display(&form_data.email));

    let credentials = Credentials {
        email: form_data.0.email,
        password: Secret::from(form_data.0.password),
    };

    match authenticate(pool, credentials).await {
        Ok(user_id) => {
            tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

            event!(Level::DEBUG, "authentication succeeded");

            session.renew();
            session
                .insert_user_id(user_id)
                .map_err(|err| login_redirect(LoginError::Unexpected(err.into())))?;

            Ok(see_other("/"))
        }

        Err(err) => {
            event!(Level::WARN, "authentication failed");

            let err = match err {
                AuthError::InvalidCredentials(_) => LoginError::Auth(err.into()),
                AuthError::Unexpected(_) => LoginError::Unexpected(err.into()),
            };

            Err(login_redirect(err))
        }
    }
}

fn login_redirect(err: LoginError) -> InternalError<LoginError> {
    // FlashMessage::error(err.to_string()).send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/login"))
        .finish();

    InternalError::from_response(err, response)
}

// Logout

#[tracing::instrument(name = "Do logout", skip(session))]
pub async fn handle_logout(session: TypedSession) -> Result<HttpResponse, actix_web::Error> {
    let user_id = session.get_user_id().map_err(e500)?;
    match user_id {
        Some(_) => {
            session.logout();
            // FlashMessage::info("You have successfully logged out").send();
            Ok(see_other("/"))
        }
        None => Ok(see_other("/")),
    }
}
