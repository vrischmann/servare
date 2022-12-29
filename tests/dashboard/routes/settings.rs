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
    let response = app.get_settings_html().await;
    assert!(response.contains("Successfully logged in"));

    // Check
    assert!(response.contains("Settings stuff"));
}
