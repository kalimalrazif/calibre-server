pub mod models;

use models::{Author, Book, BookFormat, BookMetadata, Category, CustomColumn, CustomValue};
use sqlx::{Row, sqlite::SqlitePool};
use std::collections::HashMap;

/// Database connection and query methods
pub struct CalibreDb {
    pool: SqlitePool,
}

impl CalibreDb {
    /// Connect to Calibre database
    pub async fn connect(db_path: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(&format!("sqlite://{}?mode=ro", db_path)).await?;
        Ok(Self { pool })
    }

    /// Get all custom column definitions
    pub async fn get_custom_columns(&self) -> Result<Vec<CustomColumn>, sqlx::Error> {
        sqlx::query_as::<_, CustomColumn>(
            "SELECT id, label, name, datatype, mark_for_delete, editable, display, is_multiple, normalized 
             FROM custom_columns"
        )
        .fetch_all(&self.pool)
        .await
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

        let mut result = Vec::new();
        for book in books {
            match self.get_book_metadata(book.id).await {
                Ok(metadata) => result.push(metadata),
                Err(e) => {
                    tracing::warn!("Failed to load metadata for book {}: {}", book.id, e);
                    // Continue with next book instead of failing
                }
            }
        }

        Ok(result)
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

        let authors = self.get_book_authors(book_id).await?;
        let formats = self.get_book_formats(book_id).await?;
        let custom_columns = self.get_book_custom_columns(book_id).await?;
        let (series_id, series_name, series_index) = self.get_book_series(book_id).await?;
        let tags = self.get_book_tags(book_id).await?;
        let publisher = self.get_book_publisher(book_id).await?;
        let comment = self.get_book_comment(book_id).await?;

        Ok(BookMetadata {
            book,
            authors,
            formats,
            custom_columns,
            series_id,
            series_name,
            series_index,
            tags,
            publisher,
            comment,
        })
    }

    /// Get authors for a book
    async fn get_book_authors(&self, book_id: i64) -> Result<Vec<Author>, sqlx::Error> {
        sqlx::query_as::<_, Author>(
            "SELECT a.* FROM authors a 
             JOIN books_authors_link bal ON a.id = bal.author 
             WHERE bal.book = ? 
             ORDER BY bal.id",
        )
        .bind(book_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Get formats for a book
    async fn get_book_formats(&self, book_id: i64) -> Result<Vec<BookFormat>, sqlx::Error> {
        sqlx::query_as::<_, BookFormat>("SELECT * FROM data WHERE book = ?")
            .bind(book_id)
            .fetch_all(&self.pool)
            .await
    }

    /// Get series info for a book
    async fn get_book_series(
        &self,
        book_id: i64,
    ) -> Result<(Option<i64>, Option<String>, Option<f64>), sqlx::Error> {
        let row = sqlx::query(
            "SELECT s.id, s.name, b.series_index 
             FROM series s 
             JOIN books_series_link bsl ON s.id = bsl.series 
             JOIN books b ON b.id = bsl.book
             WHERE bsl.book = ?",
        )
        .bind(book_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            Ok((
                Some(row.get("id")),
                Some(row.get("name")),
                Some(row.get("series_index")),
            ))
        } else {
            Ok((None, None, None))
        }
    }

    /// Get tags for a book
    async fn get_book_tags(&self, book_id: i64) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT t.name 
             FROM tags t 
             JOIN books_tags_link btl ON t.id = btl.tag 
             WHERE btl.book = ? 
             ORDER BY t.name",
        )
        .bind(book_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|row| row.get("name")).collect())
    }

    /// Get publisher for a book
    async fn get_book_publisher(&self, book_id: i64) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT p.name 
             FROM publishers p 
             JOIN books_publishers_link bpl ON p.id = bpl.publisher 
             WHERE bpl.book = ?",
        )
        .bind(book_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.get("name")))
    }

    /// Get comment/synopsis for a book
    async fn get_book_comment(&self, book_id: i64) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query("SELECT text FROM comments WHERE book = ?")
            .bind(book_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| r.get("text")))
    }

    /// Get custom column values for a book
    async fn get_book_custom_columns(
        &self,
        book_id: i64,
    ) -> Result<HashMap<String, CustomValue>, sqlx::Error> {
        let columns = match self.get_custom_columns().await {
            Ok(cols) => cols,
            Err(_) => return Ok(HashMap::new()), // No custom columns or error reading them
        };
        let mut result = HashMap::new();

        for col in columns {
            if col.mark_for_delete {
                continue;
            }

            let table_name = format!("custom_column_{}", col.id);
            // Ignore errors for individual columns
            if let Ok(Some(val)) = self
                .get_custom_column_value(book_id, &table_name, &col.datatype)
                .await
            {
                result.insert(col.label.clone(), val);
            }
        }

        Ok(result)
    }

    /// Get a single custom column value
    async fn get_custom_column_value(
        &self,
        book_id: i64,
        table_name: &str,
        datatype: &str,
    ) -> Result<Option<CustomValue>, sqlx::Error> {
        let query = format!("SELECT value FROM {} WHERE book = ?", table_name);

        let row = sqlx::query(&query)
            .bind(book_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let value = match datatype {
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
            Ok(value)
        } else {
            Ok(None)
        }
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
    }

    /// Get recently added books
    pub async fn get_recent_books(&self, limit: i64) -> Result<Vec<BookMetadata>, sqlx::Error> {
        let books =
            sqlx::query_as::<_, Book>("SELECT * FROM books ORDER BY timestamp DESC LIMIT ?")
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            if let Ok(metadata) = self.get_book_metadata(book.id).await {
                result.push(metadata);
            }
        }
        Ok(result)
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

        let mut result = Vec::new();
        for book in books {
            match self.get_book_metadata(book.id).await {
                Ok(metadata) => result.push(metadata),
                Err(e) => {
                    tracing::warn!("Failed to load metadata for book {}: {}", book.id, e);
                }
            }
        }

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

        let mut result = Vec::new();
        for book in books {
            match self.get_book_metadata(book.id).await {
                Ok(metadata) => result.push(metadata),
                Err(e) => {
                    tracing::warn!("Failed to load metadata for book {}: {}", book.id, e);
                }
            }
        }

        Ok(result)
    }
}
