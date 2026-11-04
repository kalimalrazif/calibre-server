use crate::db::models::{BookMetadata, Category, CustomValue};
use chrono::{DateTime, Utc};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use std::io::Cursor;

/// Generate OPDS feed XML.
///
/// Links (`href`) are emitted as **absolute URLs** (`{base_url}/books`) so a
/// reader that keeps `user:pass@` embedded in the catalog URL re-sends the
/// credential to every sublink. Feed `<id>` values also use the absolute
/// `base_url` so they stay globally unique and stable.
pub struct OpdsGenerator {
    /// Absolute base URL, used for both `<id>` values and `href`s.
    base_url: String,
}

impl OpdsGenerator {
    /// Build a generator from the absolute base URL.
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }

    /// Absolute base for every `href` (scheme + host + mount path, no trailing
    /// slash), e.g. `https://host/calibre`.
    ///
    /// OPDS `href`s are emitted ABSOLUTE rather than root-relative so a reader
    /// that keeps `user:pass@` embedded in the catalog URL re-sends the
    /// credential to each sublink: a root-relative `/calibre/books` resolves
    /// against the bare host and drops the embedded credential, forcing the
    /// reader to re-prompt on every feed. An absolute href to the same origin
    /// carries it through (this is how cops' single `feed.php` behaved).
    fn href_base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    /// Emit the OpenSearch discovery `<link rel="search">`. It must appear in
    /// EVERY feed, not only the root: FBReader enables its search box from the
    /// feed the user is currently viewing, so a sub-feed (All Books, By Author,
    /// …) without this link shows no search affordance and never fires a query.
    /// `title` is included because some readers key the button label/presence
    /// on it (cops sends `title="Search here"`).
    fn write_search_link(
        &self,
        writer: &mut Writer<Cursor<Vec<u8>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "search"));
        link.push_attribute(("href", format!("{}/search.xml", self.href_base()).as_str()));
        link.push_attribute(("type", "application/opensearchdescription+xml"));
        link.push_attribute(("title", "Search"));
        writer.write_event(Event::Empty(link))?;
        Ok(())
    }

    /// Turn a book title into a safe, URL/filename-friendly slug for the last
    /// segment of a download href. Keeps ASCII alphanumerics, collapses every
    /// other run of characters into a single `_`, and trims leading/trailing
    /// `_`. A title that reduces to nothing falls back to `book`.
    ///
    /// ponytail: ASCII-only slug. Non-ASCII letters (accents, CJK) collapse to
    /// `_`; that is deliberate for a path segment that must survive any reader
    /// and any filesystem. The real title is still in `<title>` and in the
    /// Content-Disposition filename.
    fn filename_slug(title: &str) -> String {
        let mut slug = String::with_capacity(title.len());
        let mut prev_sep = false;
        for c in title.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c);
                prev_sep = false;
            } else if !prev_sep {
                slug.push('_');
                prev_sep = true;
            }
        }
        let trimmed = slug.trim_matches('_');
        if trimmed.is_empty() {
            "book".to_string()
        } else {
            trimmed.to_string()
        }
    }

    /// Generate root catalog feed
    pub fn generate_root(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        feed.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
        feed.push_attribute(("xmlns:opds", "http://opds-spec.org/2010/catalog"));
        // OpenSearch namespace: some readers (FBReader) only enable the search
        // UI when the feed declares it, as cops does.
        feed.push_attribute(("xmlns:opensearch", "http://a9.com/-/spec/opensearch/1.1/"));
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
        link.push_attribute(("href", format!("{}/", self.href_base()).as_str()));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        ));
        writer.write_event(Event::Empty(link))?;

        // OpenSearch discovery link.
        self.write_search_link(&mut writer)?;

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

    /// Generate acquisition feed for all books.
    pub fn generate_books_feed(
        &self,
        books: Vec<BookMetadata>,
        page: i64,
        per_page: i64,
        total: i64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.generate_acquisition_feed(books, page, per_page, total, "/books", "All Books", "")
    }

    /// Generate acquisition feed for a search result.
    ///
    /// The feed identifies itself (`<id>`, self/next/prev links) with the
    /// `/search?q=...` URL that produced it, NOT `/books`. A reader paginates
    /// and validates against the self link, so a search feed that claimed to be
    /// `/books` made readers (FBReader) drop the results or page out of the
    /// search into the full catalog.
    pub fn generate_search_feed(
        &self,
        books: Vec<BookMetadata>,
        query: &str,
        page: i64,
        per_page: i64,
        total: i64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // `q` is carried as an extra query param so pagination stays inside the
        // search. It is percent-encoded so a term with spaces or `&` round-trips.
        let extra = format!("&q={}", urlencode(query));
        let title = format!("Search: {}", query);
        self.generate_acquisition_feed(books, page, per_page, total, "/search", &title, &extra)
    }

    /// Shared acquisition-feed body. `path` is the feed's own route (`/books`
    /// or `/search`), `extra_query` is appended to every self/next/prev href
    /// (e.g. `&q=robot`) so pagination round-trips on the same route.
    #[allow(clippy::too_many_arguments)]
    fn generate_acquisition_feed(
        &self,
        books: Vec<BookMetadata>,
        page: i64,
        per_page: i64,
        total: i64,
        path: &str,
        title: &str,
        extra_query: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        feed.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
        feed.push_attribute(("xmlns:opds", "http://opds-spec.org/2010/catalog"));
        feed.push_attribute(("xmlns:calibre", "http://calibre-ebook.com"));
        feed.push_attribute(("xmlns:opensearch", "http://a9.com/-/spec/opensearch/1.1/"));
        writer.write_event(Event::Start(feed))?;

        self.write_element(&mut writer, "id", &format!("{}{}", self.base_url, path))?;
        self.write_element(&mut writer, "title", title)?;
        self.write_element(&mut writer, "updated", &Utc::now().to_rfc3339())?;

        // Self link
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "self"));
        link.push_attribute((
            "href",
            format!(
                "{}{}?page={}&per_page={}{}",
                self.href_base(),
                path,
                page,
                per_page,
                extra_query
            )
            .as_str(),
        ));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        ));
        writer.write_event(Event::Empty(link))?;

        // Search affordance on this feed too, so the reader's search box works
        // while browsing results/listings, not only from the root.
        self.write_search_link(&mut writer)?;

        // Next page link
        let total_pages = (total + per_page - 1) / per_page;
        if page < total_pages {
            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "next"));
            link.push_attribute((
                "href",
                format!(
                    "{}{}?page={}&per_page={}{}",
                    self.href_base(),
                    path,
                    page + 1,
                    per_page,
                    extra_query
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
                    "{}{}?page={}&per_page={}{}",
                    self.href_base(),
                    path,
                    page - 1,
                    per_page,
                    extra_query
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

        // Authors. Each carries a <uri> linking to the author's own feed, like
        // cops — lets a reader jump to "other books by this author".
        for author in &book_meta.authors {
            writer.write_event(Event::Start(BytesStart::new("author")))?;
            self.write_element(writer, "name", &author.name)?;
            self.write_element(
                writer,
                "uri",
                &format!("{}/authors/{}", self.href_base(), author.id),
            )?;
            writer.write_event(Event::End(BytesEnd::new("author")))?;
        }

        // Description. Calibre stores the comment as HTML, so emit it as
        // type="text/html" with the markup preserved — this is what cops does
        // and what lets an OPDS reader (FBReader) render the description with
        // its formatting (bold, paragraphs) instead of a flattened blob.
        // quick-xml escapes the HTML into the text node; the reader unescapes
        // and renders it, per the Atom text/html construct.
        if let Some(comment) = &book_meta.comment {
            self.write_typed_element(writer, "content", "text/html", comment)?;
        }

        // Publisher
        if let Some(publisher) = &book_meta.publisher {
            self.write_element(writer, "dcterms:publisher", publisher)?;
        }

        // Language (Calibre ISO code, e.g. "spa"/"eng"), like cops.
        if let Some(language) = &book_meta.language {
            self.write_element(writer, "dcterms:language", language)?;
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
            // Full cover image first, then the thumbnail — the order cops uses.
            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "http://opds-spec.org/image"));
            link.push_attribute((
                "href",
                format!("{}/cover/{}", self.href_base(), book.id).as_str(),
            ));
            link.push_attribute(("type", "image/jpeg"));
            writer.write_event(Event::Empty(link))?;

            // Thumbnail points at the RESIZED endpoint (164px), like cops, so the
            // reader shows a small cover in the catalog list instead of fetching
            // the full-size image (which FBReader did not render in the list).
            let mut thumb = BytesStart::new("link");
            thumb.push_attribute(("rel", "http://opds-spec.org/image/thumbnail"));
            thumb.push_attribute((
                "href",
                format!("{}/cover/{}/thumbnail", self.href_base(), book.id).as_str(),
            ));
            thumb.push_attribute(("type", "image/jpeg"));
            writer.write_event(Event::Empty(thumb))?;
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

            // The href ends in a real filename with extension
            // (`.../download/{id}/{format}/{slug}.{format}`) so a reader that
            // ignores Content-Disposition names the saved file from the URL's
            // last segment instead of the bare format (which produced
            // `epub.epub`). cops does the same with its `/fetch/.../{slug}.epub`.
            let ext = format.format.to_lowercase();
            let slug = Self::filename_slug(&book.title);
            let mut link = BytesStart::new("link");
            link.push_attribute(("rel", "http://opds-spec.org/acquisition"));
            link.push_attribute((
                "href",
                format!(
                    "{}/download/{}/{}/{}.{}",
                    self.href_base(),
                    book.id,
                    ext,
                    slug,
                    ext
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

    /// Like `write_element` but sets an Atom `type` attribute (e.g. "text").
    fn write_typed_element(
        &self,
        writer: &mut Writer<Cursor<Vec<u8>>>,
        tag: &str,
        atom_type: &str,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut el = BytesStart::new(tag);
        el.push_attribute(("type", atom_type));
        writer.write_event(Event::Start(el))?;
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
        link.push_attribute(("href", format!("{}{}", self.href_base(), path).as_str()));
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
        feed.push_attribute(("xmlns:opensearch", "http://a9.com/-/spec/opensearch/1.1/"));
        writer.write_event(Event::Start(feed))?;

        self.write_element(&mut writer, "id", &format!("{}{}", self.base_url, path))?;
        self.write_element(&mut writer, "title", title)?;
        self.write_element(&mut writer, "updated", &Utc::now().to_rfc3339())?;

        // Self link
        let mut link = BytesStart::new("link");
        link.push_attribute(("rel", "self"));
        link.push_attribute(("href", format!("{}{}", self.href_base(), path).as_str()));
        link.push_attribute((
            "type",
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        ));
        writer.write_event(Event::Empty(link))?;

        // Search affordance on navigation/category feeds too.
        self.write_search_link(&mut writer)?;

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
                format!("{}{}/{}", self.href_base(), path, category.id).as_str(),
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
        self.write_element(&mut writer, "InputEncoding", "UTF-8")?;
        self.write_element(&mut writer, "OutputEncoding", "UTF-8")?;

        // The <Url> `type` is the bare `application/atom+xml`, as cops emits it.
        // FBReader matches the descriptor's Url type to decide whether a catalog
        // is searchable; the fuller `profile=opds-catalog;kind=acquisition`
        // value made it not recognize the search template, so the in-app search
        // never fired. The template keeps `q={searchTerms}`, matching the
        // `/search?q=` route and its `SearchQuery.q` handler.
        let mut url = BytesStart::new("Url");
        url.push_attribute(("type", "application/atom+xml"));
        url.push_attribute((
            "template",
            format!("{}/search?q={{searchTerms}}", self.href_base()).as_str(),
        ));
        writer.write_event(Event::Empty(url))?;

        // Example query: some readers surface it, and it is harmless otherwise.
        let mut example = BytesStart::new("Query");
        example.push_attribute(("role", "example"));
        example.push_attribute(("searchTerms", "robot"));
        writer.write_event(Event::Empty(example))?;

        writer.write_event(Event::End(BytesEnd::new("OpenSearchDescription")))?;

        let result = writer.into_inner().into_inner();
        Ok(String::from_utf8(result)?)
    }
}

/// Percent-encode a query-string VALUE. Minimal on purpose: only the bytes that
/// would break an OPDS `href` query param are escaped (space, `&`, `#`, `+`,
/// `%`, `?`, `=`); everything else (letters, digits, accents) passes through.
///
/// ponytail: not a general-purpose URL encoder — it covers the characters a
/// book search term realistically carries. If a term ever needs full RFC 3986
/// escaping, swap this for the `percent-encoding` crate.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '&' => out.push_str("%26"),
            '#' => out.push_str("%23"),
            '+' => out.push_str("%2B"),
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3F"),
            '=' => out.push_str("%3D"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_feed_links_are_absolute() {
        // Mounted at root: hrefs are absolute URLs to the host.
        let root = OpdsGenerator::new("http://host:8080".to_string())
            .generate_root()
            .unwrap();
        assert!(root.contains("href=\"http://host:8080/books\""));
        assert!(root.contains("href=\"http://host:8080/search.xml\""));

        // Under /calibre: hrefs carry the full origin + mount path, so a reader
        // that embeds user:pass@ in the catalog URL re-sends it to each sublink.
        let sub = OpdsGenerator::new("https://host/calibre".to_string())
            .generate_root()
            .unwrap();
        assert!(sub.contains("href=\"https://host/calibre/books\""));
        assert!(sub.contains("href=\"https://host/calibre/search.xml\""));
        // No root-relative hrefs remain.
        assert!(!sub.contains("href=\"/calibre/books\""));

        // A trailing slash in base_url must not double up.
        let slash = OpdsGenerator::new("https://host/calibre/".to_string())
            .generate_root()
            .unwrap();
        assert!(slash.contains("href=\"https://host/calibre/books\""));
    }

    #[test]
    fn search_feed_identifies_as_search_not_books() {
        let g = OpdsGenerator::new("https://host/calibre".to_string());
        let feed = g.generate_search_feed(vec![], "robot", 1, 50, 0).unwrap();
        // The feed describes ITSELF as /search?...&q=robot, so a reader paginates
        // inside the search instead of paging out into the full /books catalog.
        assert!(feed.contains("<id>https://host/calibre/search</id>"));
        assert!(feed.contains(
            "rel=\"self\" href=\"https://host/calibre/search?page=1&amp;per_page=50&amp;q=robot\""
        ));
        // It must NOT claim to be the /books feed.
        assert!(!feed.contains("<id>https://host/calibre/books</id>"));
    }

    #[test]
    fn search_feed_encodes_query_terms() {
        let g = OpdsGenerator::new("https://host/calibre".to_string());
        // A term with a space and an ampersand must round-trip in the href.
        let feed = g
            .generate_search_feed(vec![], "foo & bar", 1, 50, 0)
            .unwrap();
        assert!(feed.contains("q=foo%20%26%20bar"));
        // Accented characters are preserved (not corrupted byte-by-byte).
        assert_eq!(urlencode("ción"), "ción");
    }

    #[test]
    fn every_feed_advertises_search_not_only_root() {
        let g = OpdsGenerator::new("https://host/calibre".to_string());
        // Root advertises search.
        let root = g.generate_root().unwrap();
        assert!(
            root.contains(r#"rel="search""#),
            "root must advertise search"
        );
        // An acquisition (books) feed must advertise it too, so FBReader's
        // search box works while browsing a listing.
        let books = g.generate_books_feed(vec![], 1, 50, 0).unwrap();
        assert!(
            books.contains(r#"rel="search""#),
            "books feed must advertise search; got: {books}"
        );
        // A navigation/category feed must advertise it too.
        let cats = g
            .generate_category_feed("Authors", "/authors", vec![])
            .unwrap();
        assert!(
            cats.contains(r#"rel="search""#),
            "category feed must advertise search; got: {cats}"
        );
    }

    #[test]
    fn opensearch_url_type_is_bare_atom_for_fbreader() {
        let g = OpdsGenerator::new("https://host/calibre".to_string());
        let xml = g.generate_opensearch().unwrap();
        // FBReader needs the bare atom type on the <Url>, not the fuller
        // profile/kind variant, to recognize the catalog as searchable.
        assert!(
            xml.contains(r#"<Url type="application/atom+xml" template="https://host/calibre/search?q={searchTerms}""#),
            "descriptor Url must use bare atom type; got: {xml}"
        );
        assert!(xml.contains("<InputEncoding>UTF-8</InputEncoding>"));
    }

    #[test]
    fn filename_slug_is_safe_and_never_empty() {
        // Spaces and punctuation collapse to single underscores, trimmed.
        assert_eq!(
            OpdsGenerator::filename_slug("Cooking for Geeks: Real Science!"),
            "Cooking_for_Geeks_Real_Science"
        );
        // Non-ASCII collapses to `_`; result still usable.
        assert_eq!(
            OpdsGenerator::filename_slug("El caballero dragón"),
            "El_caballero_drag_n"
        );
        // A title that reduces to nothing falls back to `book`.
        assert_eq!(OpdsGenerator::filename_slug("¿?¡!"), "book");
    }

    #[test]
    fn book_entry_download_href_ends_in_slug_with_extension() {
        use crate::db::models::{Author, Book, BookFormat, BookMetadata};
        use std::collections::HashMap;
        let g = OpdsGenerator::new("https://host/calibre".to_string());
        let meta = BookMetadata {
            book: Book {
                id: 7,
                title: "My Book".to_string(),
                sort: None,
                timestamp: None,
                pubdate: None,
                series_index: None,
                author_sort: None,
                path: "x".to_string(),
                uuid: None,
                has_cover: Some(false),
                last_modified: None,
            },
            authors: vec![Author {
                id: 1,
                name: "A".to_string(),
                sort: None,
                link: None,
            }],
            formats: vec![BookFormat {
                id: 1,
                book: 7,
                format: "EPUB".to_string(),
                uncompressed_size: None,
                name: "My Book".to_string(),
            }],
            custom_columns: HashMap::new(),
            series_id: None,
            series_name: None,
            series_index: None,
            tags: vec![],
            publisher: None,
            comment: None,
            language: None,
        };
        let feed = g.generate_books_feed(vec![meta], 1, 50, 1).unwrap();
        // The acquisition href ends in `{slug}.{ext}`, not the bare format.
        assert!(
            feed.contains("/download/7/epub/My_Book.epub"),
            "download href should end in a named file; feed was: {feed}"
        );
    }
}
