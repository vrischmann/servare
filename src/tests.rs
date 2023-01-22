use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use url::Url;

/// Reads the content of a test data file at `path` relative to the Cargo manifest directory, which
/// is the root of our repository.
///
/// # Panics
///
/// Panics if:
/// * the CARGO_MANIFEST_DIR environment variable doesn't exist
/// * the test file can't be read to the end
pub fn testdata(path: &str) -> Vec<u8> {
    let mut contents = Vec::new();

    let cargo_dir = std::env::var("CARGO_MANIFEST_DIR").expect("unable to get the Cargo directory");

    let mut full_path = PathBuf::from(cargo_dir);
    full_path.push("testdata");
    full_path.push(path);

    let mut fs = File::open(full_path).expect("unable to open file");
    fs.read_to_end(&mut contents)
        .expect("unable to read file to end");

    contents
}

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
