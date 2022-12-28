use crate::helpers::LoginBody;
use crate::helpers::{assert_is_redirect_to, read_body, spawn_app};

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
async fn login_should_work() {
    let app = spawn_app().await;

    let login_body = LoginBody {
        email: app.test_user.email.clone(),
        password: app.test_user.password.clone(),
    };

    let login_response = app.post_login(&login_body).await;
    assert_is_redirect_to(&login_response, "/");

    let login_response_body = read_body(login_response).await;
    assert!(login_response_body.contains("Successully logged in"));
}
