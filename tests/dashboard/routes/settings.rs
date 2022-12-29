use crate::helpers::LoginBody;
use crate::helpers::{assert_is_redirect_to, spawn_app};

#[tokio::test]
async fn settings_page_should_work_if_logged_in() {
    // Setup, login
    let app = spawn_app().await;

    let login_body = LoginBody {
        email: app.test_user.email.clone(),
        password: app.test_user.password.clone(),
    };
    let login_response = app.post_login(&login_body).await;
    assert_is_redirect_to(&login_response, "/");

    // Fetch the settings page
    let response = app.get_html("/settings").await;
    assert!(response.contains("Successfully logged in"));

    // Check
    assert!(response.contains("Settings stuff"));
}

#[tokio::test]
async fn settings_page_should_redirect_if_not_logged_in() {
    // Setup
    let app = spawn_app().await;

    // Fetch the settings page
    let response = app.get("/settings").await;
    assert_is_redirect_to(&response, "/login");
}
