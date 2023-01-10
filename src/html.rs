use crate::fetch_bytes;
use select::document::Document;
use select::predicate::Name;
use std::io;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum FetchDocumentError {
    #[error(transparent)]
    IO(#[from] io::Error),
    #[error(transparent)]
    HTTP(#[from] reqwest::Error),
}

/// Fetch the document at `url` using `client`.
///
/// # Errors
///
/// This function will return an error if:
/// * the HTTP fetch fails for any reason
/// * the response is not a valid HTML document
#[tracing::instrument(name = "Fetch document", skip(client))]
pub async fn fetch_document(
    client: &reqwest::Client,
    url: &Url,
) -> Result<Document, FetchDocumentError> {
    let response = fetch_bytes(client, url).await?;

    let document = Document::from_read(&response[..])?;

    Ok(document)
}

/// Criteria when finding a link in a document
pub enum FindLinkCriteria {
    /// Single rel attribute value to find
    Rel(&'static str),
    /// Any type attribute to find
    AnyType(&'static [&'static str]),
}

/// Find the first link in a [`select::document::Document`] matching a [`FindLinkCriteria`].
pub fn find_link_in_document(
    url: &Url,
    document: &Document,
    criteria: FindLinkCriteria,
) -> Option<Url> {
    for link in document.find(Name("link")) {
        let link_href = link.attr("href").unwrap_or_default();

        // The href might be absolute
        let url = if !link_href.starts_with("http") {
            url.join(link_href)
        } else {
            Url::parse(link_href)
        };

        if let Ok(url) = url {
            match criteria {
                FindLinkCriteria::Rel(rel) => {
                    let link_rel = link.attr("rel").unwrap_or_default();
                    if link_rel == rel {
                        return Some(url);
                    }
                }
                FindLinkCriteria::AnyType(types) => {
                    let link_type = link.attr("type").unwrap_or_default();
                    for typ in types {
                        if link_type == *typ {
                            return Some(url);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_link_in_document_with_rel() {
        let url = Url::parse("https://example.com").unwrap();
        let document = Document::from(
            r#"
            <html>
            <head>
            <link rel="foobar" href="/hello">
            </head>
            </html>
        "#,
        );

        let link = find_link_in_document(&url, &document, FindLinkCriteria::Rel("foobar"));
        assert!(link.is_some());
        assert_eq!("https://example.com/hello", link.unwrap().to_string())
    }

    #[test]
    fn find_link_in_document_with_type() {
        let url = Url::parse("https://example.com").unwrap();
        let document = Document::from(
            r#"
            <html>
            <head>
            <link href="/yesterday" type="foo">
            </head>
            </html>
        "#,
        );

        let link = find_link_in_document(&url, &document, FindLinkCriteria::AnyType(&["foo"]));
        assert!(link.is_some());
        assert_eq!("https://example.com/yesterday", link.unwrap().to_string())
    }
}
