use crate::helpers::LoginBody;
use crate::helpers::{assert_is_redirect_to, spawn_app};

#[tokio::test]
async fn login_form_should_work() {
    let app = spawn_app().await;

    let response = app.get_login().await.text().await.unwrap();
    assert!(
        response.contains("login"),
        "login page doesn't contain the title 'login'"
    );
}

#[tokio::test]
async fn initial_login_should_send_an_email() {
    // 1) Setup

    let app = spawn_app().await;

    // 2) Submit the login form

    let login_body = LoginBody {
        email: app.test_user.email.clone(),
        password: app.test_user.password.clone(),
    };

    let login_response = app.post_login(&login_body).await;
    assert_is_redirect_to(&login_response, "/");

    // TODO(vincent): need to add and validate flash messages here
}
