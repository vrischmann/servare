use actix_web::http::{header, StatusCode};
use actix_web::HttpResponse;
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

pub async fn handle_status() -> HttpResponse {
    HttpResponse::Ok().finish()
}

mod home;
mod login;

pub use home::handle_home;
pub use login::*;
