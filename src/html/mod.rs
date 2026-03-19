use crate::db::models::{BookMetadata, Category, CustomValue};
use askama::Template;

/// Custom Askama filters
mod filters {
    use chrono::{NaiveDate, Utc};

    /// Format a datetime string as relative time with date, e.g. "3 months ago (2026-01-26)"
    pub fn relative_date(value: &super::CustomValue) -> askama::Result<String> {
        let raw = value.to_string();
        let date_str = raw.split_whitespace().next().unwrap_or(&raw);
        let date_str = date_str.split('T').next().unwrap_or(date_str);

        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .unwrap_or_else(|_| Utc::now().date_naive());

        let today = Utc::now().date_naive();
        let days = (today - date).num_days();

        let relative = if days == 0 {
            "today".to_string()
        } else if days == 1 {
            "yesterday".to_string()
        } else if days < 30 {
            format!("{} days ago", days)
        } else if days < 365 {
            let months = days / 30;
            if months == 1 {
                "1 month ago".to_string()
            } else {
                format!("{} months ago", months)
            }
        } else {
            let years = days / 365;
            if years == 1 {
                "1 year ago".to_string()
            } else {
                format!("{} years ago", years)
            }
        };

        Ok(format!("{} ({})", relative, date_str))
    }
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub prefix: String,
}

#[derive(Template)]
#[template(path = "categories.html")]
pub struct CategoriesTemplate {
    pub title: String,
    pub categories: Vec<Category>,
    pub base_path: String,
    pub prefix: String,
}

#[derive(Template)]
#[template(path = "books.html")]
pub struct BooksTemplate {
    pub title: String,
    pub books: Vec<BookMetadata>,
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub sort: String,
    pub view: String,
    pub prefix: String,
}
