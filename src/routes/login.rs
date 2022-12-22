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

#[tracing::instrument(name = "Login submit", skip(state, form_data), fields())]
pub async fn submit(
    State(state): State<ApplicationState>,
    Form(form_data): Form<LoginFormData>,
) -> impl IntoResponse {
    let pool = &state.pool;

    tracing::Span::current().record("email", &tracing::field::display(&form_data.email));

    match fetch_user_login_methods(pool, &form_data.email).await {
        Ok(methods) => {
            info!(
                methods = ?methods,
                email = %form_data.email,
                "got the existing state"
            );
        }
        Err(err) => {
            error!(err = %err, "unable to check if user exists");
        }
    }

    see_other("/")
}

#[derive(Debug, PartialEq)]
pub enum LoginMethod {
    Email,
    Password,
    // WebAuthn, TODO(vincent): implement this !
}

pub async fn fetch_user_login_methods(
    pool: &PgPool,
    email: &UserEmail,
) -> anyhow::Result<Vec<LoginMethod>> {
    let record = sqlx::query!(
        r#"SELECT id, hashed_password FROM users WHERE email = $1"#,
        email.as_ref()
    )
    .fetch_optional(pool)
    .await?;

    // The "email" login method is always available.
    let mut login_methods = vec![LoginMethod::Email];

    if let Some(record) = record {
        if record.hashed_password.is_some() {
            login_methods.push(LoginMethod::Password);
        }
    }

    Ok(login_methods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::get_configuration;
    use crate::domain::UserId;
    use crate::startup::get_connection_pool;
    use fake::faker::internet::en::SafeEmail;
    use fake::Fake;

    async fn get_pool() -> sqlx::PgPool {
        let config = get_configuration().unwrap();
        get_connection_pool(&config.database).await.unwrap()
    }

    #[tokio::test]
    async fn login_methods_on_nonexisting_user() {
        let pool = get_pool().await;

        let email = UserEmail::parse(SafeEmail().fake()).unwrap();

        let login_methods = fetch_user_login_methods(&pool, &email).await.unwrap();
        // Expect to only get the "email" login method here
        assert_eq!(1, login_methods.len());
    }

    #[tokio::test]
    async fn login_methods_on_existing_user() {
        let pool = get_pool().await;

        let user_id = UserId::default();
        let email = UserEmail::parse(SafeEmail().fake()).unwrap();

        // This is a quick hack to set the login methods for this user
        {
            sqlx::query!(
                r#"
                INSERT INTO users(id, email, hashed_password)
                VALUES($1::uuid, $2, $3)
                "#,
                &user_id.0,
                &email.0,
                "foobar",
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let login_methods = fetch_user_login_methods(&pool, &email).await.unwrap();
        // Expect to get the login methods set on the user, here we set these up:
        // * password
        assert_eq!(
            vec![LoginMethod::Email, LoginMethod::Password],
            login_methods
        );
    }
}
