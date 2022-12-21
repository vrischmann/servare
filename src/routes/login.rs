use crate::domain::User;
use crate::routes::{see_other, Error};
use askama::Template;
use axum::extract::Form;
use axum::response::{Html, IntoResponse};

#[derive(askama::Template)]
#[template(path = "login.html.j2")]
struct LoginTemplate {
    pub user: Option<User>,
}

pub async fn form() -> Result<Html<String>, Error> {
    let tpl = LoginTemplate { user: None };

    let response = Html(tpl.render()?);

    Ok(response)
}

#[derive(serde::Deserialize)]
pub struct LoginFormData {
    pub email: String,
}

pub async fn submit(Form(login_form_data): Form<LoginFormData>) -> impl IntoResponse {
    // TODO(vincent): implement this !
    //
    // 1) lookup the account with the email
    //
    // 2) if the account exists, determine if it has alternative login methods (password, webauthn)
    // 3) if it has any, redirect to a page with buttons to choose the login method (email,
    //    password or webauthn)
    // 4) if not, send a login email

    see_other("/")
}
