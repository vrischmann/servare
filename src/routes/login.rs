use crate::routes::Error;
use askama::Template;
use axum::response::{Html, Redirect};

#[derive(askama::Template)]
#[template(path = "login.html.j2")]
struct LoginTemplate {
    pub title: String,
}

pub async fn form() -> Result<Html<String>, Error> {
    let tpl = LoginTemplate {
        title: "Home page".to_string(),
    };

    let response = Html(tpl.render()?);

    Ok(response)
}

pub async fn submit() -> Result<Redirect, Error> {
    Ok(Redirect::temporary("/"))
}
