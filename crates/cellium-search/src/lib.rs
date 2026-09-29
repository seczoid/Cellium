//! Fuzzy command search and text indexing boundaries.

mod error;
mod fuzzy;
mod text_index;

pub use error::SearchError;
pub use fuzzy::{FuzzyItem, fuzzy_filter};
pub use text_index::TextIndex;

#[cfg(test)]
mod tests;
