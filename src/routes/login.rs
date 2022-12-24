use crate::domain::{User, UserEmail};
use crate::routes::{see_other, Error};
use crate::startup::ApplicationState;
use askama::Template;
use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse};
use sqlx::PgPool;

#[derive(askama::Template)]
#[template(path = "login.html.j2")]
struct LoginTemplate {
    pub user: Option<User>,
}

#[tracing::instrument(name = "Login form")]
pub async fn form() -> Result<Html<String>, Error> {
    let tpl = LoginTemplate { user: None };

    let response = Html(tpl.render()?);

    Ok(response)
}

#[derive(serde::Deserialize)]
pub struct LoginFormData {
    pub email: UserEmail,
    pub password: String,
}

#[tracing::instrument(name = "Login submit", skip(state, form_data), fields())]
pub async fn submit(
    State(state): State<ApplicationState>,
    Form(form_data): Form<LoginFormData>,
) -> Result<impl IntoResponse, Error> {
    let _pool = &state.pool;

    tracing::Span::current().record("email", &tracing::field::display(&form_data.email));

    Ok(see_other("/"))

    // match fetch_user_login_methods(pool, &form_data.email).await {
    //     Ok(methods) => {
    //         if methods.email && methods.password {
    //             // TODO(vincent): need to keep the email in a session
    //             let response = see_other("/login/alternative").into_response();

    //             Ok(response)
    //         } else {
    //             // TODO(vincent): implement sending the login email
    //             let response = see_other("/").into_response();

    //             Ok(response)
    //         }
    //     }
    //     Err(err) => {
    //         error!(err = %err, "unable to check if user exists");
    //         Err(err.into())
    //     }
    // }
}

#[derive(Debug, PartialEq)]
pub struct LoginMethods {
    email: bool,
    password: bool,
}

/// Fetches all available [`LoginMethod`] for the given `email`.
///
/// If no user with this email exists then the only available method will be
/// [`LoginMethod::Email`].
///
/// If a user exists then its configured login methods will be returned.
///
/// TODO(vincent): remove me
#[tracing::instrument(name = "Fetch user login methods", skip(pool))]
pub async fn fetch_user_login_methods(
    pool: &PgPool,
    email: &UserEmail,
) -> anyhow::Result<LoginMethods> {
    let record = sqlx::query!(
        r#"SELECT id, hashed_password FROM users WHERE email = $1"#,
        email.as_ref()
    )
    .fetch_optional(pool)
    .await?;

    // The "email" login method is always available.
    let mut login_methods = LoginMethods {
        email: true,
        password: false,
    };

    if let Some(record) = record {
        login_methods.password = record.hashed_password.is_some();
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
        assert!(login_methods.email, "expected 'email' login method");
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
                VALUES($1, $2, $3)
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
        assert!(login_methods.email, "expected 'email' login method");
        assert!(login_methods.password, "expected 'password' login method");
    }
}
