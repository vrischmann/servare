use crate::helpers::spawn_app;

#[tokio::test]
async fn foobar_should_work() {
    let app = spawn_app().await;

    let response = app.get_foobar_html().await;
    assert_eq!(&response, "foobar");
}
