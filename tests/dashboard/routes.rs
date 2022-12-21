use crate::helpers::LoginBody;
use crate::helpers::{assert_is_redirect_to, spawn_app};

#[tokio::test]
async fn home_should_work() {
    let app = spawn_app().await;

    let response = app.get_home_html().await;
    assert!(
        response.contains("Home"),
        "home page doesn't contain the title 'Home'"
    );
}

#[tokio::test]
async fn login_form_should_work() {
    let app = spawn_app().await;

    let response = app.get_login_html().await;
    assert!(
        response.contains("login"),
        "login page doesn't contain the title 'login'"
    );
}

#[tokio::test]
async fn login_post_should_redirect() {
    let app = spawn_app().await;

    let login_body = LoginBody {
        email: app.test_user.email.clone(),
    };
    let login_response = app.post_login(&login_body).await;

    assert_is_redirect_to(&login_response, "/");

    let response = app.get_login_html().await;
    assert!(
        response.contains("login"),
        "login page doesn't contain the title 'login'"
    );
}
