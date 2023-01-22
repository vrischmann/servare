use url::Url;

/// Parses a URL from something implementing [`AsRef<str>`].
///
/// # Panics
///
/// Panics if .
pub fn parse_url<U>(url: U) -> Url
where
    U: AsRef<str>,
{
    Url::parse(url.as_ref()).expect("unable to parse URL")
}

pub async fn fetch(url: &Url) -> bytes::Bytes {
    let client = reqwest::Client::new();

    let response = client.get(url.to_string()).send().await.unwrap();
    let response_body = response.bytes().await.unwrap();

    response_body
}
