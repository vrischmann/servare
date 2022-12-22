use crate::helpers::spawn_app;

mod login;

#[tokio::test]
async fn home_should_work() {
    let app = spawn_app().await;

    let response = app.get_home_html().await;
    assert!(
        response.contains("Home"),
        "home page doesn't contain the title 'Home'"
    );
}
