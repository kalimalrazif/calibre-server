use crate::db::models::{BookMetadata, Category, CustomValue};
use chrono::{DateTime, Utc};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;

/// Generate OPDS feed XML.
///
/// Links (`href`) are emitted as **root-relative** paths (`{prefix}/books`),
/// so the feed works unchanged whether the service is mounted at `/` or
/// behind a reverse proxy at `/some/path/`. Feed `<id>` values use the
/// absolute `base_url` so they stay globally unique and stable.
pub struct OpdsGenerator {
    /// Absolute base URL, used only for stable feed `<id>` values.
    base_url: String,
    /// Root-relative mount prefix (e.g. "" or "/cops"), used for all `href`s.
    prefix: String,
}

impl OpdsGenerator {
    /// Build a generator from the absolute base URL. The href prefix is
    /// derived from the path component of `base_url` (empty when mounted at
    /// root), so a single `BASE_URL` config value drives both.
    pub fn new(base_url: String) -> Self {
        let prefix = Self::prefix_from(&base_url);
        Self { base_url, prefix }
    }

    /// Extract the root-relative path prefix from an absolute URL.
    /// `https://host/cops` -> `/cops`; `https://host` or `https://host/` -> ``.
    fn prefix_from(url: &str) -> String {
        if let Some(pos) = url.find("://") {
            let after_scheme = &url[pos + 3..];
            if let Some(slash) = after_scheme.find('/') {
                return after_scheme[slash..].trim_end_matches('/').to_string();
            }
        }
        String::new()
    }

    /// Generate root catalog feed
    pub fn generate_root(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        feed.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
        feed.push_attribute(("xmlns:opds", "http://opds-spec.org/2010/catalog"));
        writer.write_event(Event::Start(feed))?;

        self.write_element(&mut writer, "id", &format!("{}/", self.base_url))?;
        self.write_element(&mut writer, "title", "Calibre Library")?;
        self.write_element(&mut writer, "updated", &Utc::now().to_rfc3339())?;

        let author = BytesStart::new("author");
        writer.write_event(Event::Start(author.clone()))?;
        self.write_element(&mut writer, "name", "Calibre OPDS Server")?;
        writer.write_event(Event::End(BytesEnd::new("author")))?;

        // Link to self
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "self"));
        link.push_attribute(("href", format!("{}/", self.prefix).as_str()));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        ));
        writer.write_event(Event::Empty(link))?;

        // OpenSearch link
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "search"));
        link.push_attribute(("href", format!("{}/search.xml", self.prefix).as_str()));
        link.push_attribute(("type", "application/opensearchdescription+xml"));
        writer.write_event(Event::Empty(link))?;

        // Link to all books
        self.write_navigation_entry(
            &mut writer,
            "All Books",
            "/books",
            Some("Browse all books in the library"),
        )?;

        // Link to recent books
        self.write_navigation_entry(
            &mut writer,
            "Recently Added",
            "/recent",
            Some("100 most recently added books"),
        )?;

        // Link to authors
        self.write_navigation_entry(
            &mut writer,
            "By Author",
            "/authors",
            Some("Browse books by author"),
        )?;

        // Link to series
        self.write_navigation_entry(
            &mut writer,
            "By Series",
            "/series",
            Some("Browse books by series"),
        )?;

        // Link to tags
        self.write_navigation_entry(
            &mut writer,
            "By Tag",
            "/tags",
            Some("Browse books by tag/genre"),
        )?;

        // Link to publishers
        self.write_navigation_entry(
            &mut writer,
            "By Publisher",
            "/publishers",
            Some("Browse books by publisher"),
        )?;

        // Link to languages
        self.write_navigation_entry(
            &mut writer,
            "By Language",
            "/languages",
            Some("Browse books by language"),
        )?;

        // Link to ratings
        self.write_navigation_entry(
            &mut writer,
            "By Rating",
            "/ratings",
            Some("Browse books by rating"),
        )?;

        // Link to read years
        self.write_navigation_entry(
            &mut writer,
            "First Read by Year",
            "/read-years",
            Some("Browse books by year of first reading"),
        )?;

        // Link to last read years
        self.write_navigation_entry(
            &mut writer,
            "Last Read by Year",
            "/last-read-years",
            Some("Browse books by year of last reading"),
        )?;

        writer.write_event(Event::End(BytesEnd::new("feed")))?;

        let result = writer.into_inner().into_inner();
        Ok(String::from_utf8(result)?)
    }

    /// Generate acquisition feed for books
    pub fn generate_books_feed(
        &self,
        books: Vec<BookMetadata>,
        page: i64,
        per_page: i64,
        total: i64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        feed.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
        feed.push_attribute(("xmlns:opds", "http://opds-spec.org/2010/catalog"));
        feed.push_attribute(("xmlns:calibre", "http://calibre-ebook.com"));
        writer.write_event(Event::Start(feed))?;

        self.write_element(&mut writer, "id", &format!("{}/books", self.base_url))?;
        self.write_element(&mut writer, "title", "All Books")?;
        self.write_element(&mut writer, "updated", &Utc::now().to_rfc3339())?;

        // Self link
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "self"));
        link.push_attribute((
            "href",
            format!("{}/books?page={}&per_page={}", self.prefix, page, per_page).as_str(),
        ));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        ));
        writer.write_event(Event::Empty(link))?;

        // Next page link
        let total_pages = (total + per_page - 1) / per_page;
        if page < total_pages {
            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "next"));
            link.push_attribute((
                "href",
                format!(
                    "{}/books?page={}&per_page={}",
                    self.prefix,
                    page + 1,
                    per_page
                )
                .as_str(),
            ));
            link.push_attribute((
                "type",
                "application/atom+xml;profile=opds-catalog;kind=acquisition",
            ));
            writer.write_event(Event::Empty(link))?;
        }

        // Previous page link
        if page > 1 {
            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "previous"));
            link.push_attribute((
                "href",
                format!(
                    "{}/books?page={}&per_page={}",
                    self.prefix,
                    page - 1,
                    per_page
                )
                .as_str(),
            ));
            link.push_attribute((
                "type",
                "application/atom+xml;profile=opds-catalog;kind=acquisition",
            ));
            writer.write_event(Event::Empty(link))?;
        }

        // Write book entries
        for book_meta in books {
            self.write_book_entry(&mut writer, &book_meta)?;
        }

        writer.write_event(Event::End(BytesEnd::new("feed")))?;

        let result = writer.into_inner().into_inner();
        Ok(String::from_utf8(result)?)
    }

    /// Write a single book entry
    fn write_book_entry(
        &self,
        writer: &mut Writer<Cursor<Vec<u8>>>,
        book_meta: &BookMetadata,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let book = &book_meta.book;

        writer.write_event(Event::Start(BytesStart::new("entry")))?;

        // Basic metadata
        self.write_element(writer, "title", &book.title)?;
        self.write_element(
            writer,
            "id",
            &format!(
                "urn:uuid:{}",
                book.uuid.as_ref().unwrap_or(&book.id.to_string())
            ),
        )?;

        let updated = book
            .last_modified
            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).to_rfc3339())
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        self.write_element(writer, "updated", &updated)?;

        // Published date
        if let Some(pubdate) = book.pubdate {
            let pub_str = DateTime::<Utc>::from_naive_utc_and_offset(pubdate, Utc).to_rfc3339();
            self.write_element(writer, "published", &pub_str)?;
            self.write_element(writer, "dcterms:issued", &pub_str)?;
        }

        // Authors
        for author in &book_meta.authors {
            writer.write_event(Event::Start(BytesStart::new("author")))?;
            self.write_element(writer, "name", &author.name)?;
            writer.write_event(Event::End(BytesEnd::new("author")))?;
        }

        // Summary/Synopsis
        if let Some(comment) = &book_meta.comment {
            self.write_element(writer, "summary", comment)?;
            self.write_element(writer, "content", comment)?;
        }

        // Publisher
        if let Some(publisher) = &book_meta.publisher {
            self.write_element(writer, "dcterms:publisher", publisher)?;
        }

        // Series
        if let Some(series_name) = &book_meta.series_name {
            // Standard OPDS way using category
            let mut category = BytesStart::new("category");
            category.push_attribute(("term", "series"));
            category.push_attribute(("label", series_name.as_str()));
            if let Some(idx) = book_meta.series_index {
                category.push_attribute(("scheme", format!("#{}", idx).as_str()));
            }
            writer.write_event(Event::Empty(category))?;

            // Calibre-specific fields
            let series_info = if let Some(idx) = book_meta.series_index {
                format!("{} #{}", series_name, idx)
            } else {
                series_name.clone()
            };
            self.write_element(writer, "calibre:series", &series_info)?;
            if let Some(idx) = book_meta.series_index {
                self.write_element(writer, "calibre:series_index", &idx.to_string())?;
            }
        }

        // Tags
        for tag in &book_meta.tags {
            let mut category = BytesStart::new("category");
            category.push_attribute(("term", tag.as_str()));
            category.push_attribute(("label", tag.as_str()));
            writer.write_event(Event::Empty(category))?;
        }

        // Custom columns as metadata
        for (label, value) in &book_meta.custom_columns {
            let value_str = match value {
                CustomValue::Text(s) => s.clone(),
                CustomValue::Int(i) => i.to_string(),
                CustomValue::Float(f) => f.to_string(),
                CustomValue::Bool(b) => b.to_string(),
                CustomValue::DateTime(s) => s.clone(),
            };

            let tag_name = format!("calibre:{}", label);
            self.write_element(writer, &tag_name, &value_str)?;
        }

        // Cover image
        if book.has_cover.unwrap_or(false) {
            // Thumbnail
            let mut thumb = BytesStart::new("link");
            thumb.push_attribute(("rel", "http://opds-spec.org/image/thumbnail"));
            thumb.push_attribute((
                "href",
                format!("{}/cover/{}", self.prefix, book.id).as_str(),
            ));
            thumb.push_attribute(("type", "image/jpeg"));
            writer.write_event(Event::Empty(thumb))?;

            // Full image
            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "http://opds-spec.org/image"));
            link.push_attribute((
                "href",
                format!("{}/cover/{}", self.prefix, book.id).as_str(),
            ));
            link.push_attribute(("type", "image/jpeg"));
            writer.write_event(Event::Empty(link))?;
        }

        // Acquisition links for each format
        for format in &book_meta.formats {
            let mime_type = match format.format.to_lowercase().as_str() {
                "epub" => "application/epub+zip",
                "pdf" => "application/pdf",
                "mobi" => "application/x-mobipocket-ebook",
                "azw3" => "application/vnd.amazon.ebook",
                _ => "application/octet-stream",
            };

            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "http://opds-spec.org/acquisition"));
            link.push_attribute((
                "href",
                format!(
                    "{}/download/{}/{}",
                    self.prefix,
                    book.id,
                    format.format.to_lowercase()
                )
                .as_str(),
            ));
            link.push_attribute(("type", mime_type));
            writer.write_event(Event::Empty(link))?;
        }

        writer.write_event(Event::End(BytesEnd::new("entry")))?;
        Ok(())
    }

    /// Helper to write a simple text element
    fn write_element(
        &self,
        writer: &mut Writer<Cursor<Vec<u8>>>,
        tag: &str,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        writer.write_event(Event::Start(BytesStart::new(tag)))?;
        writer.write_event(Event::Text(BytesText::new(text)))?;
        writer.write_event(Event::End(BytesEnd::new(tag)))?;
        Ok(())
    }

    /// Helper to write a navigation entry
    fn write_navigation_entry(
        &self,
        writer: &mut Writer<Cursor<Vec<u8>>>,
        title: &str,
        path: &str,
        summary: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        writer.write_event(Event::Start(BytesStart::new("entry")))?;
        self.write_element(writer, "title", title)?;
        self.write_element(writer, "id", &format!("{}{}", self.base_url, path))?;
        self.write_element(writer, "updated", &Utc::now().to_rfc3339())?;

        if let Some(summary_text) = summary {
            self.write_element(writer, "summary", summary_text)?;
        }

        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "subsection"));
        link.push_attribute(("href", format!("{}{}", self.prefix, path).as_str()));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        ));
        writer.write_event(Event::Empty(link))?;

        writer.write_event(Event::End(BytesEnd::new("entry")))?;
        Ok(())
    }

    /// Generate category list feed (authors, series, tags, etc.)
    pub fn generate_category_feed(
        &self,
        title: &str,
        path: &str,
        categories: Vec<Category>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        feed.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
        feed.push_attribute(("xmlns:opds", "http://opds-spec.org/2010/catalog"));
        writer.write_event(Event::Start(feed))?;

        self.write_element(&mut writer, "id", &format!("{}{}", self.base_url, path))?;
        self.write_element(&mut writer, "title", title)?;
        self.write_element(&mut writer, "updated", &Utc::now().to_rfc3339())?;

        // Self link
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "self"));
        link.push_attribute(("href", format!("{}{}", self.prefix, path).as_str()));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        ));
        writer.write_event(Event::Empty(link))?;

        // Write category entries
        for category in categories {
            writer.write_event(Event::Start(BytesStart::new("entry")))?;

            let title_with_count = format!("{} ({})", category.name, category.count);
            self.write_element(&mut writer, "title", &title_with_count)?;
            self.write_element(
                &mut writer,
                "id",
                &format!("{}{}/{}", self.base_url, path, category.id),
            )?;
            self.write_element(&mut writer, "updated", &Utc::now().to_rfc3339())?;

            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "subsection"));
            link.push_attribute((
                "href",
                format!("{}{}/{}", self.prefix, path, category.id).as_str(),
            ));
            link.push_attribute((
                "type",
                "application/atom+xml;profile=opds-catalog;kind=acquisition",
            ));
            writer.write_event(Event::Empty(link))?;

            writer.write_event(Event::End(BytesEnd::new("entry")))?;
        }

        writer.write_event(Event::End(BytesEnd::new("feed")))?;

        let result = writer.into_inner().into_inner();
        Ok(String::from_utf8(result)?)
    }

    /// Generate OpenSearch descriptor
    pub fn generate_opensearch(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut desc = BytesStart::new("OpenSearchDescription");
        desc.push_attribute(("xmlns", "http://a9.com/-/spec/opensearch/1.1/"));
        writer.write_event(Event::Start(desc))?;

        self.write_element(&mut writer, "ShortName", "Calibre Library")?;
        self.write_element(&mut writer, "Description", "Search Calibre Library")?;

        let mut url = BytesStart::new("Url");
        url.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        ));
        url.push_attribute((
            "template",
            format!("{}/search?q={{searchTerms}}", self.prefix).as_str(),
        ));
        writer.write_event(Event::Empty(url))?;

        writer.write_event(Event::End(BytesEnd::new("OpenSearchDescription")))?;

        let result = writer.into_inner().into_inner();
        Ok(String::from_utf8(result)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_from_handles_root_and_subpath() {
        // Mounted at root -> empty prefix.
        assert_eq!(OpdsGenerator::prefix_from("http://host:8080"), "");
        assert_eq!(OpdsGenerator::prefix_from("http://host:8080/"), "");
        // Mounted under a subpath -> that path, trailing slash trimmed.
        assert_eq!(OpdsGenerator::prefix_from("https://host/cops"), "/cops");
        assert_eq!(OpdsGenerator::prefix_from("https://host/cops/"), "/cops");
        assert_eq!(
            OpdsGenerator::prefix_from("https://host/path/to/svc"),
            "/path/to/svc"
        );
    }

    #[test]
    fn root_feed_links_are_relative_to_mount() {
        // At root: hrefs start with "/", not the absolute host.
        let root = OpdsGenerator::new("http://host:8080".to_string())
            .generate_root()
            .unwrap();
        assert!(root.contains("href=\"/books\""));
        assert!(root.contains("href=\"/search.xml\""));
        assert!(!root.contains("href=\"http://host:8080/books\""));

        // Under /cops: same links, prefixed — portable without code changes.
        let sub = OpdsGenerator::new("https://host/cops".to_string())
            .generate_root()
            .unwrap();
        assert!(sub.contains("href=\"/cops/books\""));
        assert!(sub.contains("href=\"/cops/search.xml\""));
    }
}
