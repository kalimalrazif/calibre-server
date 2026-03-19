mod config;
mod db;
mod epub;
mod html;
mod opds;

use askama_axum::IntoResponse as AskamaIntoResponse;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use clap::Parser;
use config::Config;
use db::CalibreDb;
use html::{BooksTemplate, CategoriesTemplate, IndexTemplate};
use opds::OpdsGenerator;
use serde::Deserialize;
use std::sync::Arc;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Calibre OPDS Server - A lightweight OPDS server for Calibre libraries
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to Calibre library directory
    #[arg(short, long, env = "CALIBRE_LIBRARY_PATH")]
    library: std::path::PathBuf,

    /// Server host address
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,

    /// Server port
    #[arg(short, long, env = "PORT", default_value = "8080")]
    port: u16,

    /// Base URL for the server (used in OPDS links)
    #[arg(short, long, env = "BASE_URL")]
    base_url: Option<String>,
}

/// Application state
#[derive(Clone)]
struct AppState {
    db: Arc<CalibreDb>,
    config: Arc<Config>,
}

/// Query parameters for pagination
#[derive(Deserialize)]
struct PaginationQuery {
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_per_page")]
    per_page: i64,
    #[serde(default)]
    sort: String,
    #[serde(default = "default_view")]
    view: String,
}

/// Query parameters for search
#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_per_page")]
    per_page: i64,
    #[serde(default)]
    sort: String,
    #[serde(default = "default_view")]
    view: String,
}

fn default_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    50
}

fn default_view() -> String {
    "list".to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "calibre_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Build configuration from arguments
    let config = Config {
        library_path: args.library,
        host: args.host,
        port: args.port,
        base_url: args.base_url,
    };

    tracing::info!("Connecting to Calibre database at {:?}", config.db_path());
    let db = CalibreDb::connect(config.db_path().to_str().unwrap()).await?;

    let state = AppState {
        db: Arc::new(db),
        config: Arc::new(config.clone()),
    };

    // Build router
    let app = Router::new()
        // HTML routes
        .route("/html", get(html_index))
        .route("/html/books", get(html_books))
        .route("/html/recent", get(html_recent))
        .route("/html/search", get(html_search))
        .route("/html/authors", get(html_authors))
        .route("/html/authors/:id", get(html_books_by_author))
        .route("/html/series", get(html_series))
        .route("/html/series/:id", get(html_books_by_series))
        .route("/html/tags", get(html_tags))
        .route("/html/tags/:id", get(html_books_by_tag))
        .route("/html/publishers", get(html_publishers))
        .route("/html/publishers/:id", get(html_books_by_publisher))
        .route("/html/languages", get(html_languages))
        .route("/html/languages/:id", get(html_books_by_language))
        .route("/html/ratings", get(html_ratings))
        .route("/html/ratings/:id", get(html_books_by_rating))
        .route("/html/read-years", get(html_read_years))
        .route("/html/read-years/:year", get(html_books_read_in_year))
        .route("/html/last-read-years", get(html_last_read_years))
        .route(
            "/html/last-read-years/:year",
            get(html_books_last_read_in_year),
        )
        // OPDS routes
        .route("/", get(root_catalog))
        .route("/search.xml", get(opensearch_descriptor))
        .route("/search", get(search_books))
        .route("/books", get(books_catalog))
        .route("/recent", get(recent_books))
        .route("/authors", get(authors_catalog))
        .route("/authors/:id", get(books_by_author))
        .route("/series", get(series_catalog))
        .route("/series/:id", get(books_by_series))
        .route("/tags", get(tags_catalog))
        .route("/tags/:id", get(books_by_tag))
        .route("/publishers", get(publishers_catalog))
        .route("/publishers/:id", get(books_by_publisher))
        .route("/languages", get(languages_catalog))
        .route("/languages/:id", get(books_by_language))
        .route("/ratings", get(ratings_catalog))
        .route("/ratings/:id", get(books_by_rating))
        .route("/read-years", get(read_years_catalog))
        .route("/read-years/:year", get(books_read_in_year))
        .route("/last-read-years", get(last_read_years_catalog))
        .route("/last-read-years/:year", get(books_last_read_in_year))
        .route("/download/:id/:format", get(download_book))
        .route("/cover/:id", get(get_cover))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Root OPDS catalog
async fn root_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_root()?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// OpenSearch descriptor
async fn opensearch_descriptor(State(state): State<AppState>) -> Result<Response, AppError> {
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_opensearch()?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/opensearchdescription+xml",
        )],
        xml,
    )
        .into_response())
}

/// Search books
async fn search_books(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .search_books(&params.q, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Books acquisition catalog
async fn books_catalog(
    State(state): State<AppState>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books(params.per_page, offset, &params.sort)
        .await?;
    let total = state.db.get_book_count().await?;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Download a book in specific format
async fn download_book(
    State(state): State<AppState>,
    Path((id, format)): Path<(i64, String)>,
) -> Result<Response, AppError> {
    let book_meta = state.db.get_book_metadata(id).await?;

    let format_upper = format.to_uppercase();
    let book_format = book_meta
        .formats
        .iter()
        .find(|f| f.format == format_upper)
        .ok_or_else(|| AppError::NotFound("Format not found".to_string()))?;

    let file_path = state
        .config
        .library_path
        .join(&book_meta.book.path)
        .join(format!("{}.{}", book_format.name, format.to_lowercase()));

    if !file_path.exists() {
        return Err(AppError::NotFound("File not found".to_string()));
    }

    // For EPUB files, update metadata before sending
    let content = if format_upper == "EPUB" {
        tracing::info!("Updating EPUB metadata for book {}", id);
        let file_path_clone = file_path.clone();
        let book_meta_clone = book_meta.clone();

        match tokio::task::spawn_blocking(move || {
            epub::update_epub_metadata(&file_path_clone, &book_meta_clone)
        })
        .await
        {
            Ok(Ok(updated_content)) => {
                tracing::info!("Successfully updated EPUB metadata");
                updated_content
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    "Failed to update EPUB metadata: {}, sending original file",
                    e
                );
                tokio::fs::read(&file_path).await?
            }
            Err(e) => {
                tracing::warn!("Task failed: {}, sending original file", e);
                tokio::fs::read(&file_path).await?
            }
        }
    } else {
        tokio::fs::read(&file_path).await?
    };

    let mime_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    let filename = format!("{}.{}", book_meta.book.title, format.to_lowercase());

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        content,
    )
        .into_response())
}

/// Get book cover image
async fn get_cover(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let book_meta = state.db.get_book_metadata(id).await?;

    let cover_path = state
        .config
        .library_path
        .join(&book_meta.book.path)
        .join("cover.jpg");

    if !cover_path.exists() {
        return Err(AppError::NotFound("Cover not found".to_string()));
    }

    let content = tokio::fs::read(&cover_path).await?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/jpeg")],
        content,
    )
        .into_response())
}

/// Recently added books
async fn recent_books(State(state): State<AppState>) -> Result<Response, AppError> {
    let books = state.db.get_recent_books(100).await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, 1, 100, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Authors catalog
async fn authors_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let authors = state.db.get_authors().await?;
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Authors", "/authors", authors)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books by author
async fn books_by_author(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_author(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64; // Simplified, should query count

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Series catalog
async fn series_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let series = state.db.get_series().await?;
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Series", "/series", series)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books by series
async fn books_by_series(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_series(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Tags catalog
async fn tags_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let tags = state.db.get_tags().await?;
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Tags", "/tags", tags)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books by tag
async fn books_by_tag(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_tag(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Publishers catalog
async fn publishers_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let publishers = state.db.get_publishers().await?;
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Publishers", "/publishers", publishers)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books by publisher
async fn books_by_publisher(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_publisher(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Languages catalog
async fn languages_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let languages = state.db.get_languages().await?;
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Languages", "/languages", languages)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books by language
async fn books_by_language(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_language(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Ratings catalog
async fn ratings_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let ratings = state.db.get_ratings().await?;
    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Ratings", "/ratings", ratings)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books by rating
async fn books_by_rating(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_rating(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Read years catalog
async fn read_years_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let categories = state.db.get_read_years_with_counts().await?;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_category_feed("Books Read by Year", "/read-years", categories)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books read in a specific year
async fn books_read_in_year(
    State(state): State<AppState>,
    Path(year): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_read_in_year(&year, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

/// Last read years catalog (leidoel)
async fn last_read_years_catalog(State(state): State<AppState>) -> Result<Response, AppError> {
    let categories = state.db.get_last_read_years_with_counts().await?;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml =
        generator.generate_category_feed("Last Read by Year", "/last-read-years", categories)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=navigation",
        )],
        xml,
    )
        .into_response())
}

/// Books last read in a specific year
async fn books_last_read_in_year(
    State(state): State<AppState>,
    Path(year): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> Result<Response, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_last_read_in_year(&year, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    let generator = OpdsGenerator::new(state.config.base_url());
    let xml = generator.generate_books_feed(books, params.page, params.per_page, total)?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/atom+xml;profile=opds-catalog;kind=acquisition",
        )],
        xml,
    )
        .into_response())
}

// ============================================================================
// HTML Handlers
// ============================================================================

/// HTML index page
async fn html_index() -> impl AskamaIntoResponse {
    IndexTemplate
}

/// HTML all books
async fn html_books(
    State(state): State<AppState>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books(params.per_page, offset, &params.sort)
        .await?;
    let total = state.db.get_book_count().await?;

    Ok(BooksTemplate {
        title: "All Books".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML recent books
async fn html_recent(
    State(state): State<AppState>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let books = state.db.get_recent_books(100).await?;

    Ok(BooksTemplate {
        title: "Recently Added".to_string(),
        books,
        page: 1,
        per_page: 100,
        total: 100,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML search
async fn html_search(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .search_books(&params.q, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: format!("Search results for '{}'", params.q),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML authors list
async fn html_authors(State(state): State<AppState>) -> Result<impl AskamaIntoResponse, AppError> {
    let authors = state.db.get_authors().await?;

    Ok(CategoriesTemplate {
        title: "Authors".to_string(),
        categories: authors,
        base_path: "/html/authors".to_string(),
    })
}

/// HTML books by author
async fn html_books_by_author(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_author(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: "Books by Author".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML series list
async fn html_series(State(state): State<AppState>) -> Result<impl AskamaIntoResponse, AppError> {
    let series = state.db.get_series().await?;

    Ok(CategoriesTemplate {
        title: "Series".to_string(),
        categories: series,
        base_path: "/html/series".to_string(),
    })
}

/// HTML books by series
async fn html_books_by_series(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_series(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: "Books in Series".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML tags list
async fn html_tags(State(state): State<AppState>) -> Result<impl AskamaIntoResponse, AppError> {
    let tags = state.db.get_tags().await?;

    Ok(CategoriesTemplate {
        title: "Tags".to_string(),
        categories: tags,
        base_path: "/html/tags".to_string(),
    })
}

/// HTML books by tag
async fn html_books_by_tag(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_tag(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: "Books with Tag".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML publishers list
async fn html_publishers(
    State(state): State<AppState>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let publishers = state.db.get_publishers().await?;

    Ok(CategoriesTemplate {
        title: "Publishers".to_string(),
        categories: publishers,
        base_path: "/html/publishers".to_string(),
    })
}

/// HTML books by publisher
async fn html_books_by_publisher(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_publisher(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: "Books by Publisher".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML languages list
async fn html_languages(
    State(state): State<AppState>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let languages = state.db.get_languages().await?;

    Ok(CategoriesTemplate {
        title: "Languages".to_string(),
        categories: languages,
        base_path: "/html/languages".to_string(),
    })
}

/// HTML books by language
async fn html_books_by_language(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_language(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: "Books in Language".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML ratings list
async fn html_ratings(State(state): State<AppState>) -> Result<impl AskamaIntoResponse, AppError> {
    let ratings = state.db.get_ratings().await?;

    Ok(CategoriesTemplate {
        title: "Ratings".to_string(),
        categories: ratings,
        base_path: "/html/ratings".to_string(),
    })
}

/// HTML books by rating
async fn html_books_by_rating(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_by_rating(id, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: "Books with Rating".to_string(),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML read years list
async fn html_read_years(
    State(state): State<AppState>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let years = state.db.get_read_years_with_counts().await?;

    Ok(CategoriesTemplate {
        title: "First Read by Year".to_string(),
        categories: years,
        base_path: "/html/read-years".to_string(),
    })
}

/// HTML books read in year
async fn html_books_read_in_year(
    State(state): State<AppState>,
    Path(year): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_read_in_year(&year, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: format!("First Read in {}", year),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// HTML last read years list
async fn html_last_read_years(
    State(state): State<AppState>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let years = state.db.get_last_read_years_with_counts().await?;

    Ok(CategoriesTemplate {
        title: "Last Read by Year".to_string(),
        categories: years,
        base_path: "/html/last-read-years".to_string(),
    })
}

/// HTML books last read in year
async fn html_books_last_read_in_year(
    State(state): State<AppState>,
    Path(year): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> Result<impl AskamaIntoResponse, AppError> {
    let offset = (params.page - 1) * params.per_page;
    let books = state
        .db
        .get_books_last_read_in_year(&year, params.per_page, offset)
        .await?;
    let total = books.len() as i64;

    Ok(BooksTemplate {
        title: format!("Last Read in {}", year),
        books,
        page: params.page,
        per_page: params.per_page,
        total,
        sort: params.sort.clone(),
        view: params.view.clone(),
    })
}

/// Application error type
#[derive(Debug)]
enum AppError {
    Database(sqlx::Error),
    Io(std::io::Error),
    Opds(Box<dyn std::error::Error>),
    NotFound(String),
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err)
    }
}

impl From<Box<dyn std::error::Error>> for AppError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        AppError::Opds(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Database(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::Opds(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
        };

        tracing::error!("Request error: {:?}", self);
        (status, message).into_response()
    }
}
