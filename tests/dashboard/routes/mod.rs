use crate::helpers::spawn_app;

mod feeds;
mod login;
mod settings;

#[tokio::test]
async fn home_should_work() {
    let app = spawn_app().await;

    let response = app.get_html("/").await;
    assert!(
        response.contains("Home"),
        "home page doesn't contain the title 'Home'"
    );
}
