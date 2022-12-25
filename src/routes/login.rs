use crate::authentication::{authenticate, AuthError, Credentials};
use crate::domain::{User, UserEmail};
use crate::routes::{see_other, Error};
use crate::startup::ApplicationState;
use askama::Template;
use axum::extract::{Form, State};
use axum::http::header;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use secrecy::Secret;

#[derive(askama::Template)]
#[template(path = "login.html.j2")]
struct LoginTemplate {
    pub user: Option<User>,
}

#[tracing::instrument(name = "Login form")]
pub async fn form() -> Result<Html<String>, Error> {
    let tpl = LoginTemplate { user: None };

    let response = Html(tpl.render()?);

    Ok(response)
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("Authentication failed")]
    Auth(#[source] anyhow::Error),
    #[error("Something went wrong")]
    Unexpected(#[source] anyhow::Error),
}

impl IntoResponse for LoginError {
    fn into_response(self) -> Response {
        let status_code = match self {
            LoginError::Auth(_) => StatusCode::SEE_OTHER,
            LoginError::Unexpected(_) => StatusCode::SEE_OTHER,
        };

        // TODO(vincent): how de we log here ??

        let response = (
            status_code,
            [(header::LOCATION, "/login")],
            status_code.to_string(),
        );

        response.into_response()
    }
}

#[derive(serde::Deserialize)]
pub struct LoginFormData {
    pub email: UserEmail,
    pub password: String,
}

#[tracing::instrument(name = "Login submit", skip(state, form_data), fields())]
pub async fn submit(
    State(state): State<ApplicationState>,
    Form(form_data): Form<LoginFormData>,
) -> Result<impl IntoResponse, LoginError> {
    let pool = &state.pool;

    tracing::Span::current().record("email", &tracing::field::display(&form_data.email));

    let credentials = Credentials {
        email: form_data.email,
        password: Secret::from(form_data.password),
    };

    match authenticate(pool, credentials).await {
        Ok(user_id) => {
            tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

            // TODO(vincent): handle session

            Ok(see_other("/"))
        }

        Err(err) => {
            let err = match err {
                AuthError::InvalidCredentials(_) => LoginError::Auth(err.into()),
                AuthError::Unexpected(_) => LoginError::Unexpected(err.into()),
            };

            Err(err)
        }
    }
}
