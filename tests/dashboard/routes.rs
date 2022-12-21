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

    let response = app.get_login().await.text().await.unwrap();
    assert!(
        response.contains("login"),
        "login page doesn't contain the title 'login'"
    );
}

#[tokio::test]
async fn successful_login_should_work() {
    // 1) Setup

    let app = spawn_app().await;

    // 2) Submit the login form

    let login_body = LoginBody {
        email: app.test_user.email.clone(),
    };

    let login_response = app.post_login(&login_body).await;
    assert_is_redirect_to(&login_response, "/");

    // 3) The login page should now be a redirect to the home

    // TODO(vincent): uncomment and implement this
    // let response = app.get_login().await;
    // assert_is_redirect_to(&response, "/");
}
