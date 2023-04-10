use feed_rs::model::Entry as RawFeedEntry;
use feed_rs::model::Feed as RawFeed;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Holds feed data parsed from a [`feed_rs::model::Feed`].
///
/// This means this struct should _not_ be used to represent data from the database.
pub struct ParsedFeed {
    pub url: Url,
    pub title: String,
    pub site_link: Option<Url>,
    pub description: String,
}

impl ParsedFeed {
    pub fn parse(url: &Url, data: &[u8]) -> Result<Self, ParseError> {
        let raw_feed = feed_rs::parser::parse(data).map_err(Into::<anyhow::Error>::into)?;

        Ok(Self::from_raw_feed(url, raw_feed))
    }

    fn get_site_link(feed: &RawFeed) -> Option<String> {
        let mut site_link = None;

        for link in &feed.links {
            if link.rel.is_none() {
                continue;
            }

            site_link = Some(link.href.clone());
        }

        site_link
    }

    pub fn from_raw_feed(url: &Url, feed: RawFeed) -> Self {
        let site_link_url = Self::get_site_link(&feed)
            .as_ref()
            .and_then(|v| Url::parse(v).ok());

        ParsedFeed {
            url: url.clone(),
            title: feed.title.map(|v| v.content).unwrap_or_default(),
            site_link: site_link_url,
            description: feed.description.map(|v| v.content).unwrap_or_default(),
        }
    }
}

/// Holds feed entry data parsed from a [`feed_rs::model::Entry`].
///
/// This means this struct should _not_ be used to represent data from the database.
pub struct ParsedFeedEntry {
    pub external_id: String,
    pub url: Option<Url>,
    pub title: String,
    pub summary: String,
    pub authors: Vec<String>,
}

impl ParsedFeedEntry {
    pub fn from_raw_feed_entry(entry: RawFeedEntry) -> Self {
        let url = entry
            .links
            .iter()
            .flat_map(|v| Url::parse(&v.href).ok())
            .take(1)
            .last();

        let title = entry.title.map(|v| v.content).unwrap_or_default();
        let summary = entry.summary.map(|v| v.content).unwrap_or_default();

        // TODO(vincent): see if there's anything better to do ?
        let authors: Vec<String> = entry
            .authors
            .into_iter()
            .map(|person| {
                if let Some(ref email) = person.email {
                    email.clone()
                } else {
                    person.name
                }
            })
            .collect();

        Self {
            external_id: entry.id,
            url,
            title,
            summary,
            authors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_parse_should_work() {
        const DATA: &str = r#"
<rss xmlns:atom="http://www.w3.org/2005/Atom" version="2.0">
<channel>
<title>Foo</title>
<link>https://example.com/blog/</link>
<description>Foo</description>
<atom:link href="https://example.com/blog/index.xml" rel="self" type="application/rss+xml"/>
</channel>
</rss>"#;

        let url = Url::parse("https://example.com/blog/").unwrap();

        let feed = ParsedFeed::parse(&url, DATA.as_bytes()).unwrap();
        assert_eq!(feed.title, "Foo");
        assert_eq!(feed.site_link, Some(url));
        assert_eq!(feed.description, "Foo");
    }

    #[test]
    fn feed_parse_should_work_even_with_links_not_in_order() {
        // Move the relevant site link _after_ the "self" link.
        // We expect to ignore the self link.

        const DATA: &str = r#"
<rss xmlns:atom="http://www.w3.org/2005/Atom" version="2.0">
<channel>
<atom:link href="https://example.com/blog/index.xml" rel="self" type="application/rss+xml"/>
<title>Foo</title>
<description>Foo</description>
<link>https://example.com/blog/</link>
</channel>
</rss>"#;

        let url = Url::parse("https://example.com/blog/").unwrap();

        let feed = ParsedFeed::parse(&url, DATA.as_bytes()).unwrap();
        assert_eq!(feed.title, "Foo");
        assert_eq!(feed.site_link, Some(url));
        assert_eq!(feed.description, "Foo");
    }
}
