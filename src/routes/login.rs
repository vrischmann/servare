use crate::domain::{User, UserEmail};
use crate::routes::{see_other, Error};
use crate::startup::ApplicationState;
use askama::Template;
use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse};
use sqlx::PgPool;
use tracing::{error, info};

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
    pub email: UserEmail,
}

pub async fn submit(
    State(state): State<ApplicationState>,
    Form(login_form_data): Form<LoginFormData>,
) -> impl IntoResponse {
    let pool = &state.pool;

    // 1) check if the user exists; if it doesn't send the login email
    match user_exists(pool, &login_form_data.email).await {
        Ok(exists) => {
            info!(
                exists = exists,
                email = %login_form_data.email,
                "got the existing state"
            );
        }
        Err(err) => {
            error!(err = %err, "unable to check if user exists");
        }
    }

    see_other("/")
}

async fn user_exists(pool: &PgPool, email: &UserEmail) -> Result<bool, sqlx::Error> {
    let record = sqlx::query!(r#"SELECT id FROM users WHERE email = $1"#, email.as_ref())
        .fetch_optional(pool)
        .await?;

    Ok(record.is_some())
}
