use crate::domain::UserId;
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http;
use actix_web::http::{header, StatusCode};
use actix_web::HttpResponse;
use actix_web_flash_messages::FlashMessage;
use anyhow::anyhow;
use std::convert::From;
use std::fmt;
use tracing::{event, Level};

/// Creates a [`InternalError<T>`] with the code 500 Internal Server Error.
///
/// This is useful to convert an arbitrary error into a 500 usable by actix.
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

/// Create a 303 See Other [`HttpResponse`] redirect to `location`.
pub fn see_other(location: &str) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header((header::LOCATION, location))
        .finish()
}

/// This is a helper function used to extract the [`UserId`] from a [`TypedSession`].
///
/// If there's no user id in the session _or_ the session is somehow corrupted, this returns a
/// [`InternalError<E>`] that will redirect to the login page.
///
/// # Errors
///
/// Actual errors are converted to a 500 Internal Server Error using the [`e500`] function.
#[tracing::instrument(name = "Get user id or redirect", level = "TRACE", skip(session))]
pub fn get_user_id_or_redirect<E>(session: &TypedSession) -> Result<UserId, InternalError<E>>
where
    E: From<anyhow::Error> + fmt::Display + fmt::Debug + 'static,
{
    let user_id = session
        .get_user_id()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(Into::<E>::into)
        .map_err(e500)?;

    if let Some(user_id) = user_id {
        event!(Level::TRACE, %user_id, "found user");

        Ok(user_id)
    } else {
        let response = see_other("/login");
        let err = anyhow!("The user has not logged in");

        event!(Level::TRACE, "found no user");

        Err(InternalError::from_response(err.into(), response))
    }
}

/// This creates a [`InternalError<E>`] from `err` and a 303 See Other response.
/// It also sets a flash message with the content of the error [`ToString::to_string()`] method call.
///
/// Use this whenever you want to handle an error without returning a 500 Internal Server Error.
pub fn error_redirect<E>(err: E, location: &str) -> InternalError<E>
where
    E: fmt::Display,
{
    FlashMessage::error(err.to_string()).send();

    let response = HttpResponse::SeeOther()
        .insert_header((http::header::LOCATION, location))
        .finish();

    InternalError::from_response(err, response)
}

pub async fn handle_status() -> HttpResponse {
    HttpResponse::Ok().finish()
}

pub(crate) const FEEDS_PAGE: &str = "feeds";
pub(crate) const HOME_PAGE: &str = "home";
pub(crate) const LOGIN_PAGE: &str = "login";
pub(crate) const SETTINGS_PAGE: &str = "settings";
pub(crate) const UNREAD_PAGE: &str = "unread";

mod feeds;
mod home;
mod login;
mod settings;
mod unread;

pub use feeds::*;
pub use home::handle_home;
pub use login::*;
pub use settings::*;
pub use unread::*;
