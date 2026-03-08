use crate::db::models::{BookMetadata, Category};
use askama::Template;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate;

#[derive(Template)]
#[template(path = "categories.html")]
pub struct CategoriesTemplate {
    pub title: String,
    pub categories: Vec<Category>,
    pub base_path: String,
}

#[derive(Template)]
#[template(path = "books.html")]
pub struct BooksTemplate {
    pub title: String,
    pub books: Vec<BookMetadata>,
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
}
