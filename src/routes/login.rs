use crate::domain::{User, UserEmail};
use crate::routes::{see_other, Error};
use crate::startup::ApplicationState;
use askama::Template;
use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse};

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

#[derive(serde::Deserialize)]
pub struct LoginFormData {
    pub email: UserEmail,
    pub password: String,
}

#[tracing::instrument(name = "Login submit", skip(state, form_data), fields())]
pub async fn submit(
    State(state): State<ApplicationState>,
    Form(form_data): Form<LoginFormData>,
) -> Result<impl IntoResponse, Error> {
    let _pool = &state.pool;

    tracing::Span::current().record("email", &tracing::field::display(&form_data.email));

    Ok(see_other("/"))

    // match fetch_user_login_methods(pool, &form_data.email).await {
    //     Ok(methods) => {
    //         if methods.email && methods.password {
    //             // TODO(vincent): need to keep the email in a session
    //             let response = see_other("/login/alternative").into_response();

    //             Ok(response)
    //         } else {
    //             // TODO(vincent): implement sending the login email
    //             let response = see_other("/").into_response();

    //             Ok(response)
    //         }
    //     }
    //     Err(err) => {
    //         error!(err = %err, "unable to check if user exists");
    //         Err(err.into())
    //     }
    // }
}
