use crate::domain::User;
use crate::routes::{see_other, Error};
use askama::Template;
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

pub async fn submit() -> impl IntoResponse {
    see_other("/")
}
