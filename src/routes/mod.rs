use crate::domain::UserId;
use crate::sessions::TypedSession;
use actix_web::error::InternalError;
use actix_web::http::{header, StatusCode};
use actix_web::HttpResponse;
use anyhow::anyhow;
use std::convert::From;
use std::fmt;

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

#[tracing::instrument(name = "Get user id or redirect", skip(session))]
pub fn get_user_id_or_redirect(
    session: &TypedSession,
) -> Result<UserId, InternalError<anyhow::Error>> {
    let user_id = session
        .get_user_id()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(e500)?;

    match user_id {
        Some(user_id) => Ok(user_id),
        None => {
            let response = see_other("/login");
            let err = anyhow!("The user has not logged in");

            Err(InternalError::from_response(err, response))
        }
    }
}

pub fn get_user_id_or_redirect2<E>(session: &TypedSession) -> Result<UserId, InternalError<E>>
where
    E: From<anyhow::Error> + fmt::Display + fmt::Debug + 'static,
{
    let user_id = session
        .get_user_id()
        .map_err(Into::<anyhow::Error>::into)
        .map_err(Into::<E>::into)
        .map_err(e500)?;

    match user_id {
        Some(user_id) => Ok(user_id),
        None => {
            let response = see_other("/login");
            let err = anyhow!("The user has not logged in");

            Err(InternalError::from_response(err.into(), response))
        }
    }
}

pub async fn handle_status() -> HttpResponse {
    HttpResponse::Ok().finish()
}

mod feeds;
mod home;
mod login;
mod settings;

pub use feeds::*;
pub use home::handle_home;
pub use login::*;
pub use settings::*;
