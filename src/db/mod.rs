pub mod models;

use models::{Author, Book, BookFormat, BookMetadata, Category, CustomColumn, CustomValue};
use sqlx::{Row, sqlite::SqlitePool};
use std::collections::HashMap;

/// (series_id, series_name, series_index) for a book.
type SeriesInfo = (Option<i64>, Option<String>, Option<f64>);

/// Database connection and query methods
pub struct CalibreDb {
    pool: SqlitePool,
    /// Custom column definitions, cached at connect time.
    /// Calibre's column *definitions* don't change while the server runs,
    /// so loading them once avoids re-querying `custom_columns` per book.
    custom_columns: Vec<CustomColumn>,
}

impl CalibreDb {
    /// Connect to Calibre database
    pub async fn connect(db_path: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(&format!("sqlite://{}?mode=ro", db_path)).await?;
        // Cache custom column definitions once; absence of the table is not fatal.
        let custom_columns = sqlx::query_as::<_, CustomColumn>(
            "SELECT id, label, name, datatype, mark_for_delete, editable, display, is_multiple, normalized
             FROM custom_columns",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        Ok(Self {
            pool,
            custom_columns,
        })
    }

    /// Load full metadata for a set of books in bulk.
    ///
    /// This replaces the old per-book fan-out (7 queries per book) with a
    /// fixed number of batched queries using `WHERE book IN (...)`, then
    /// stitches the related rows back onto each book in memory. For a page of
    /// N books this is a handful of round-trips instead of ~7*N.
    ///
    /// Order of the input `books` is preserved in the output.
    async fn load_metadata_batch(
        &self,
        books: Vec<Book>,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        if books.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = books.iter().map(|b| b.id).collect();
        // SQLite has no array binding; build "?,?,?" and bind each id.
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");

        // Helpers to run a batched "WHERE book IN (...)" query, binding each
        // id. They take the SQL by value (`String`) so it outlives the query
        // builder — a `&format!(...)` temporary would be dropped too early.
        macro_rules! fetch_dyn {
            ($sql:expr) => {{
                let sql: String = $sql;
                let mut q = sqlx::query(&sql);
                for id in &ids {
                    q = q.bind(id);
                }
                q.fetch_all(&self.pool).await
            }};
        }
        macro_rules! fetch_as {
            ($ty:ty, $sql:expr) => {{
                let sql: String = $sql;
                let mut q = sqlx::query_as::<_, $ty>(&sql);
                for id in &ids {
                    q = q.bind(id);
                }
                q.fetch_all(&self.pool).await
            }};
        }

        // --- Authors (ordered by link id to keep Calibre's author order) ---
        let author_rows = fetch_dyn!(format!(
            "SELECT bal.book AS book, a.id, a.name, a.sort, a.link
             FROM authors a
             JOIN books_authors_link bal ON a.id = bal.author
             WHERE bal.book IN ({placeholders})
             ORDER BY bal.id"
        ))?;
        let mut authors_by_book: HashMap<i64, Vec<Author>> = HashMap::new();
        for row in author_rows {
            authors_by_book
                .entry(row.get("book"))
                .or_default()
                .push(Author {
                    id: row.get("id"),
                    name: row.get("name"),
                    sort: row.get("sort"),
                    link: row.get("link"),
                });
        }

        // --- Formats ---
        let format_rows = fetch_as!(
            BookFormat,
            format!("SELECT * FROM data WHERE book IN ({placeholders})")
        )?;
        let mut formats_by_book: HashMap<i64, Vec<BookFormat>> = HashMap::new();
        for f in format_rows {
            formats_by_book.entry(f.book).or_default().push(f);
        }

        // --- Tags (ordered by name) ---
        let tag_rows = fetch_dyn!(format!(
            "SELECT btl.book AS book, t.name
             FROM tags t
             JOIN books_tags_link btl ON t.id = btl.tag
             WHERE btl.book IN ({placeholders})
             ORDER BY t.name"
        ))?;
        let mut tags_by_book: HashMap<i64, Vec<String>> = HashMap::new();
        for row in tag_rows {
            tags_by_book
                .entry(row.get("book"))
                .or_default()
                .push(row.get("name"));
        }

        // --- Series ---
        let series_rows = fetch_dyn!(format!(
            "SELECT bsl.book AS book, s.id, s.name, b.series_index
             FROM series s
             JOIN books_series_link bsl ON s.id = bsl.series
             JOIN books b ON b.id = bsl.book
             WHERE bsl.book IN ({placeholders})"
        ))?;
        let mut series_by_book: HashMap<i64, SeriesInfo> = HashMap::new();
        for row in series_rows {
            series_by_book.insert(
                row.get("book"),
                (
                    Some(row.get("id")),
                    Some(row.get("name")),
                    Some(row.get("series_index")),
                ),
            );
        }

        // --- Publisher ---
        let publisher_rows = fetch_dyn!(format!(
            "SELECT bpl.book AS book, p.name
             FROM publishers p
             JOIN books_publishers_link bpl ON p.id = bpl.publisher
             WHERE bpl.book IN ({placeholders})"
        ))?;
        let mut publisher_by_book: HashMap<i64, String> = HashMap::new();
        for row in publisher_rows {
            publisher_by_book.insert(row.get("book"), row.get("name"));
        }

        // --- Comments ---
        let comment_rows = fetch_dyn!(format!(
            "SELECT book, text FROM comments WHERE book IN ({placeholders})"
        ))?;
        let mut comment_by_book: HashMap<i64, String> = HashMap::new();
        for row in comment_rows {
            comment_by_book.insert(row.get("book"), row.get("text"));
        }

        // --- Custom columns (one query per non-deleted column, over all books) ---
        let mut custom_by_book: HashMap<i64, HashMap<String, CustomValue>> = HashMap::new();
        for col in &self.custom_columns {
            if col.mark_for_delete {
                continue;
            }
            let table_name = format!("custom_column_{}", col.id);
            // Table may not exist / be readable; skip the column on error.
            let rows = match fetch_dyn!(format!(
                "SELECT book, value FROM {table_name} WHERE book IN ({placeholders})"
            )) {
                Ok(rows) => rows,
                Err(_) => continue,
            };
            for row in rows {
                let book_id: i64 = row.get("book");
                let value = match col.datatype.as_str() {
                    "text" | "comments" | "series" | "enumeration" => row
                        .try_get::<String, _>("value")
                        .ok()
                        .map(CustomValue::Text),
                    "int" | "rating" => row.try_get::<i64, _>("value").ok().map(CustomValue::Int),
                    "float" => row.try_get::<f64, _>("value").ok().map(CustomValue::Float),
                    "bool" => row.try_get::<bool, _>("value").ok().map(CustomValue::Bool),
                    "datetime" => row
                        .try_get::<String, _>("value")
                        .ok()
                        .map(CustomValue::DateTime),
                    _ => None,
                };
                if let Some(value) = value {
                    custom_by_book
                        .entry(book_id)
                        .or_default()
                        .insert(col.label.clone(), value);
                }
            }
        }

        // --- Stitch everything back together, preserving input order ---
        let result = books
            .into_iter()
            .map(|book| {
                let id = book.id;
                let (series_id, series_name, series_index) = series_by_book
                    .get(&id)
                    .cloned()
                    .unwrap_or((None, None, None));
                BookMetadata {
                    book,
                    authors: authors_by_book.remove(&id).unwrap_or_default(),
                    formats: formats_by_book.remove(&id).unwrap_or_default(),
                    custom_columns: custom_by_book.remove(&id).unwrap_or_default(),
                    series_id,
                    series_name,
                    series_index,
                    tags: tags_by_book.remove(&id).unwrap_or_default(),
                    publisher: publisher_by_book.remove(&id),
                    comment: comment_by_book.remove(&id),
                }
            })
            .collect();

        Ok(result)
    }

    /// Get all books with pagination and sorting
    pub async fn get_books(
        &self,
        limit: i64,
        offset: i64,
        sort: &str,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let order_clause = match sort {
            "title_asc" => "ORDER BY books.sort ASC",
            "title_desc" => "ORDER BY books.sort DESC",
            "author_asc" => {
                "ORDER BY (SELECT name FROM authors JOIN books_authors_link ON authors.id = books_authors_link.author WHERE books_authors_link.book = books.id LIMIT 1) ASC"
            }
            "author_desc" => {
                "ORDER BY (SELECT name FROM authors JOIN books_authors_link ON authors.id = books_authors_link.author WHERE books_authors_link.book = books.id LIMIT 1) DESC"
            }
            "date_asc" => "ORDER BY books.timestamp ASC",
            "date_desc" => "ORDER BY books.timestamp DESC",
            "rating_asc" => {
                "ORDER BY (SELECT ratings.rating FROM books_ratings_link JOIN ratings ON books_ratings_link.rating = ratings.id WHERE books_ratings_link.book = books.id) ASC NULLS LAST"
            }
            "rating_desc" => {
                "ORDER BY (SELECT ratings.rating FROM books_ratings_link JOIN ratings ON books_ratings_link.rating = ratings.id WHERE books_ratings_link.book = books.id) DESC NULLS LAST"
            }
            _ => "ORDER BY books.id",
        };

        let query = format!("SELECT * FROM books {} LIMIT ? OFFSET ?", order_clause);
        let books = sqlx::query_as::<_, Book>(&query)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        self.load_metadata_batch(books).await
    }

    /// Get books filtered by read status
    /// Get total book count
    pub async fn get_book_count(&self) -> Result<i64, sqlx::Error> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM books")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("count"))
    }

    /// Get complete metadata for a single book
    pub async fn get_book_metadata(&self, book_id: i64) -> Result<BookMetadata, sqlx::Error> {
        let book = sqlx::query_as::<_, Book>("SELECT * FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_one(&self.pool)
            .await?;

        // Reuse the batched loader so single-book and list paths share one code path.
        self.load_metadata_batch(vec![book])
            .await?
            .pop()
            .ok_or(sqlx::Error::RowNotFound)
    }

    /// Get all authors with book count
    pub async fn get_authors(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT a.id, a.name, COUNT(bal.book) as count 
             FROM authors a 
             JOIN books_authors_link bal ON a.id = bal.author 
             GROUP BY a.id 
             ORDER BY a.sort COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get("id"),
                name: row.get("name"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books by author
    pub async fn get_books_by_author(
        &self,
        author_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN books_authors_link bal ON b.id = bal.book 
             WHERE bal.author = ? 
             ORDER BY b.sort COLLATE NOCASE 
             LIMIT ? OFFSET ?",
        )
        .bind(author_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get all series with book count
    pub async fn get_series(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, COUNT(bsl.book) as count 
             FROM series s 
             JOIN books_series_link bsl ON s.id = bsl.series 
             GROUP BY s.id 
             ORDER BY s.sort COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get("id"),
                name: row.get("name"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books by series
    pub async fn get_books_by_series(
        &self,
        series_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN books_series_link bsl ON b.id = bsl.book 
             WHERE bsl.series = ? 
             ORDER BY b.series_index 
             LIMIT ? OFFSET ?",
        )
        .bind(series_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get all tags with book count
    pub async fn get_tags(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT t.id, t.name, COUNT(btl.book) as count 
             FROM tags t 
             JOIN books_tags_link btl ON t.id = btl.tag 
             GROUP BY t.id 
             ORDER BY t.name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get("id"),
                name: row.get("name"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books by tag
    pub async fn get_books_by_tag(
        &self,
        tag_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN books_tags_link btl ON b.id = btl.book 
             WHERE btl.tag = ? 
             ORDER BY b.sort COLLATE NOCASE 
             LIMIT ? OFFSET ?",
        )
        .bind(tag_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get all publishers with book count
    pub async fn get_publishers(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, COUNT(bpl.book) as count 
             FROM publishers p 
             JOIN books_publishers_link bpl ON p.id = bpl.publisher 
             GROUP BY p.id 
             ORDER BY p.name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get("id"),
                name: row.get("name"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books by publisher
    pub async fn get_books_by_publisher(
        &self,
        publisher_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN books_publishers_link bpl ON b.id = bpl.book 
             WHERE bpl.publisher = ? 
             ORDER BY b.sort COLLATE NOCASE 
             LIMIT ? OFFSET ?",
        )
        .bind(publisher_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get all languages with book count
    pub async fn get_languages(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT l.id, l.lang_code as name, COUNT(bll.book) as count 
             FROM languages l 
             JOIN books_languages_link bll ON l.id = bll.lang_code 
             GROUP BY l.id 
             ORDER BY l.lang_code",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get("id"),
                name: row.get("name"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books by language
    pub async fn get_books_by_language(
        &self,
        language_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN books_languages_link bll ON b.id = bll.book 
             WHERE bll.lang_code = ? 
             ORDER BY b.sort COLLATE NOCASE 
             LIMIT ? OFFSET ?",
        )
        .bind(language_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get all ratings with book count
    pub async fn get_ratings(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT r.id, CAST(r.rating AS TEXT) as name, COUNT(brl.book) as count 
             FROM ratings r 
             JOIN books_ratings_link brl ON r.id = brl.rating 
             GROUP BY r.id 
             ORDER BY r.rating DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get("id"),
                name: row.get("name"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books by rating
    pub async fn get_books_by_rating(
        &self,
        rating_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN books_ratings_link brl ON b.id = brl.book 
             WHERE brl.rating = ? 
             ORDER BY b.sort COLLATE NOCASE 
             LIMIT ? OFFSET ?",
        )
        .bind(rating_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get recently added books
    pub async fn get_recent_books(&self, limit: i64) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books =
            sqlx::query_as::<_, Book>("SELECT * FROM books ORDER BY timestamp DESC LIMIT ?")
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;

        self.load_metadata_batch(books).await
    }

    /// Search books by title, author, or tags
    pub async fn search_books(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let search_pattern = format!("%{}%", query);

        let books = sqlx::query_as::<_, Book>(
            "SELECT DISTINCT b.* FROM books b
             LEFT JOIN books_authors_link bal ON b.id = bal.book
             LEFT JOIN authors a ON a.id = bal.author
             LEFT JOIN books_tags_link btl ON b.id = btl.book
             LEFT JOIN tags t ON t.id = btl.tag
             WHERE b.title LIKE ? COLLATE NOCASE
                OR a.name LIKE ? COLLATE NOCASE
                OR t.name LIKE ? COLLATE NOCASE
             ORDER BY b.sort COLLATE NOCASE
             LIMIT ? OFFSET ?",
        )
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    /// Get years when books were first read with counts (primera_vez)
    pub async fn get_read_years_with_counts(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT strftime('%Y', value) as year, COUNT(*) as count 
             FROM custom_column_3 
             WHERE value IS NOT NULL 
             GROUP BY year 
             ORDER BY year DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get::<String, _>("year").parse().unwrap_or(0),
                name: row.get("year"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get years when books were last read with counts (leidoel)
    pub async fn get_last_read_years_with_counts(&self) -> Result<Vec<Category>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT strftime('%Y', value) as year, COUNT(*) as count 
             FROM custom_column_2 
             WHERE value IS NOT NULL 
             GROUP BY year 
             ORDER BY year DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Category {
                id: row.get::<String, _>("year").parse().unwrap_or(0),
                name: row.get("year"),
                count: row.get("count"),
            })
            .collect())
    }

    /// Get books read in a specific year
    pub async fn get_books_read_in_year(
        &self,
        year: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        tracing::info!("Fetching books read in year: {}", year);

        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN custom_column_3 cc ON b.id = cc.book 
             WHERE strftime('%Y', cc.value) = ? 
             ORDER BY cc.value DESC 
             LIMIT ? OFFSET ?",
        )
        .bind(year)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        tracing::info!("Found {} books for year {}", books.len(), year);

        let result = self.load_metadata_batch(books).await?;

        tracing::info!("Returning {} books with metadata", result.len());
        Ok(result)
    }

    /// Get books last read in a specific year (leidoel)
    pub async fn get_books_last_read_in_year(
        &self,
        year: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books = sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b 
             JOIN custom_column_2 cc ON b.id = cc.book 
             WHERE strftime('%Y', cc.value) = ? 
             ORDER BY cc.value DESC 
             LIMIT ? OFFSET ?",
        )
        .bind(year)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        self.load_metadata_batch(books).await
    }

    // ------------------------------------------------------------------
    // Count helpers: return the TOTAL number of books matching a filter
    // (not just the current page), so the paginator can emit next/prev
    // links and show the real total.
    // ------------------------------------------------------------------

    /// Count books linked to an author.
    pub async fn count_books_by_author(&self, author_id: i64) -> Result<i64, sqlx::Error> {
        self.count_by_link("books_authors_link", "author", author_id)
            .await
    }

    /// Count books in a series.
    pub async fn count_books_by_series(&self, series_id: i64) -> Result<i64, sqlx::Error> {
        self.count_by_link("books_series_link", "series", series_id)
            .await
    }

    /// Count books with a tag.
    pub async fn count_books_by_tag(&self, tag_id: i64) -> Result<i64, sqlx::Error> {
        self.count_by_link("books_tags_link", "tag", tag_id).await
    }

    /// Count books by a publisher.
    pub async fn count_books_by_publisher(&self, publisher_id: i64) -> Result<i64, sqlx::Error> {
        self.count_by_link("books_publishers_link", "publisher", publisher_id)
            .await
    }

    /// Count books in a language.
    pub async fn count_books_by_language(&self, language_id: i64) -> Result<i64, sqlx::Error> {
        self.count_by_link("books_languages_link", "lang_code", language_id)
            .await
    }

    /// Count books with a rating.
    pub async fn count_books_by_rating(&self, rating_id: i64) -> Result<i64, sqlx::Error> {
        self.count_by_link("books_ratings_link", "rating", rating_id)
            .await
    }

    /// Shared COUNT over a Calibre `books_*_link` table.
    ///
    /// `table` and `column` are hard-coded identifiers from the call sites
    /// above (never user input), so interpolating them is safe; the filter
    /// value is still a bound parameter.
    async fn count_by_link(&self, table: &str, column: &str, id: i64) -> Result<i64, sqlx::Error> {
        let query = format!("SELECT COUNT(*) AS count FROM {table} WHERE {column} = ?");
        let row = sqlx::query(&query).bind(id).fetch_one(&self.pool).await?;
        Ok(row.get("count"))
    }

    /// Count books matching a search query (same predicate as `search_books`).
    pub async fn count_search(&self, query: &str) -> Result<i64, sqlx::Error> {
        let search_pattern = format!("%{}%", query);
        let row = sqlx::query(
            "SELECT COUNT(DISTINCT b.id) AS count FROM books b
             LEFT JOIN books_authors_link bal ON b.id = bal.book
             LEFT JOIN authors a ON a.id = bal.author
             LEFT JOIN books_tags_link btl ON b.id = btl.book
             LEFT JOIN tags t ON t.id = btl.tag
             WHERE b.title LIKE ? COLLATE NOCASE
                OR a.name LIKE ? COLLATE NOCASE
                OR t.name LIKE ? COLLATE NOCASE",
        )
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("count"))
    }

    /// Count books first-read in a given year (custom_column_3).
    pub async fn count_books_read_in_year(&self, year: &str) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS count FROM custom_column_3 WHERE strftime('%Y', value) = ?",
        )
        .bind(year)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("count"))
    }

    /// Count books last-read in a given year (custom_column_2).
    pub async fn count_books_last_read_in_year(&self, year: &str) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS count FROM custom_column_2 WHERE strftime('%Y', value) = ?",
        )
        .bind(year)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("count"))
    }
}
