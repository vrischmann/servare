use crate::domain::User;
use askama::Template;
use axum::http::StatusCode;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Response;

pub mod login;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] http::Error),
    #[error(transparent)]
    Askama(#[from] askama::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status_code = match self {
            Error::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::Askama(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::Unexpected(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status_code, status_code.to_string()).into_response()
    }
}

pub fn see_other<T>(location: T) -> impl IntoResponse
where
    T: AsRef<str>,
{
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", location.as_ref())
        .body(axum::body::Empty::new())
        .map_err(Into::<Error>::into)
}

#[derive(askama::Template)]
#[template(path = "home.html.j2")]
struct HomeTemplate {
    pub user: Option<User>,
}

pub async fn home() -> Result<Html<String>, Error> {
    let tpl = HomeTemplate { user: None };

    let response = Html(tpl.render()?);

    Ok(response)
}
