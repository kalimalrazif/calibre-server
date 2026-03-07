mod config;
mod db;
mod opds;

use axum::{
    Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use config::Config;
use db::CalibreDb;
use opds::OpdsGenerator;
use serde::Deserialize;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
}

/// Query parameters for search
#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_per_page")]
    per_page: i64,
}

fn default_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    50
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "calibre_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration (for now, hardcoded - can be extended to read from file/env)
    let config = Config {
        library_path: std::env::var("CALIBRE_LIBRARY_PATH")
            .unwrap_or_else(|_| "/path/to/calibre/library".to_string())
            .into(),
        host: std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port: std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080),
        base_url: std::env::var("BASE_URL").ok(),
    };

    tracing::info!("Connecting to Calibre database at {:?}", config.db_path());
    let db = CalibreDb::connect(config.db_path().to_str().unwrap()).await?;

    let state = AppState {
        db: Arc::new(db),
        config: Arc::new(config.clone()),
    };

    // Build router
    let app = Router::new()
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
        .route("/download/:id/:format", get(download_book))
        .route("/cover/:id", get(get_cover))
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
    let books = state.db.get_books(params.per_page, offset).await?;
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

    let content = tokio::fs::read(&file_path).await?;
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
