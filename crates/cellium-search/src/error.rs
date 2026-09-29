use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("query parser error: {0}")]
    QueryParser(#[from] tantivy::query::QueryParserError),
}
