use askama::Template;
use axum::http::StatusCode;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Response;

pub mod login;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Askama(#[from] askama::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status_code = match self {
            Error::Askama(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status_code, status_code.to_string()).into_response()
    }
}

#[derive(askama::Template)]
#[template(path = "home.html.j2")]
struct HomeTemplate {}

pub async fn home() -> Result<Html<String>, Error> {
    let tpl = HomeTemplate {};

    let response = Html(tpl.render()?);

    Ok(response)
}
