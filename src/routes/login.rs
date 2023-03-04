use crate::authentication::{authenticate, AuthError, Credentials};
use crate::debug_with_error_chain;
use crate::domain::{UserEmail, UserId};
use crate::routes::LOGIN_PAGE;
use crate::routes::{e500, see_other};
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::HttpResponse;
use actix_web::{http, web};
use actix_web_flash_messages::{FlashMessage, IncomingFlashMessages};
use askama::Template;
use secrecy::Secret;
use sqlx::PgPool;
use tracing::{event, Level};

// Login

#[derive(askama::Template)]
#[template(path = "login.html.j2")]
struct LoginTemplate {
    pub page: &'static str,
    pub user_id: Option<UserId>,
    pub flash_messages: IncomingFlashMessages,
}

#[tracing::instrument(
    name = "Login form",
    skip(session, flash_messages),
    fields(
        user_id = tracing::field::Empty,
    )
)]
pub async fn handle_login_form(
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

    let tpl = LoginTemplate {
        page: LOGIN_PAGE,
        user_id,
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
pub enum LoginError {
    #[error("Authentication failed")]
    Auth(#[source] anyhow::Error),
    #[error("Something went wrong")]
    Unexpected(#[source] anyhow::Error),
}

debug_with_error_chain!(LoginError);

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

            event!(Level::DEBUG, "successfully logged in");
            FlashMessage::success("Successfully logged in").send();

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
    FlashMessage::error(err.to_string()).send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, "/login"))
        .finish();

    InternalError::from_response(err, response)
}

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
