use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

/// Book record from Calibre database
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct Book {
    pub id: i64,
    pub title: String,
    pub sort: Option<String>,
    pub timestamp: Option<NaiveDateTime>,
    pub pubdate: Option<NaiveDateTime>,
    pub series_index: Option<f64>,
    pub author_sort: Option<String>,
    pub path: String,
    pub uuid: Option<String>,
    pub has_cover: Option<bool>,
    pub last_modified: Option<NaiveDateTime>,
}

/// Author record
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct Author {
    pub id: i64,
    pub name: String,
    pub sort: Option<String>,
    pub link: Option<String>,
}

/// Book format (epub, pdf, etc)
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct BookFormat {
    pub id: i64,
    pub book: i64,
    pub format: String,
    pub uncompressed_size: Option<i64>,
    pub name: String,
}

/// Custom column definition
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct CustomColumn {
    pub id: i64,
    pub label: String,
    pub name: String,
    pub datatype: String,
    pub mark_for_delete: bool,
    pub editable: bool,
    pub display: String,
    pub is_multiple: bool,
    pub normalized: bool,
}

/// Custom column value (generic)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CustomValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    DateTime(String),
}

/// Category for navigation
#[derive(Debug, Clone)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

/// Book with all metadata including custom columns
#[derive(Debug, Clone)]
pub struct BookMetadata {
    pub book: Book,
    pub authors: Vec<Author>,
    pub formats: Vec<BookFormat>,
    pub custom_columns: HashMap<String, CustomValue>,
    pub series_name: Option<String>,
    pub series_index: Option<f64>,
    pub tags: Vec<String>,
    pub publisher: Option<String>,
    pub comment: Option<String>,
}
