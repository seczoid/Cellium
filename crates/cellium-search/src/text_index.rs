use tantivy::{
    Index, TantivyDocument,
    collector::TopDocs,
    query::QueryParser,
    schema::{Schema, TEXT, Value},
};

use crate::SearchError;

pub struct TextIndex {
    index: Index,
    schema: Schema,
}

impl TextIndex {
    #[must_use]
    pub fn in_memory() -> Self {
        let mut builder = Schema::builder();
        builder.add_text_field("body", TEXT);
        let schema = builder.build();
        let index = Index::create_in_ram(schema.clone());
        Self { index, schema }
    }

    pub fn replace_documents(&self, documents: &[String]) -> Result<(), SearchError> {
        let body = self.schema.get_field("body")?;
        let mut writer = self.index.writer(50_000_000)?;
        writer.delete_all_documents()?;
        for document in documents {
            writer.add_document(tantivy::doc!(body => document.as_str()))?;
        }
        writer.commit()?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<String>, SearchError> {
        let body = self.schema.get_field("body")?;
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&self.index, vec![body]);
        let query = parser.parse_query(query)?;
        let docs = searcher.search(&query, &TopDocs::with_limit(limit))?;
        docs.into_iter()
            .map(|(_, address)| {
                let doc = searcher.doc::<TantivyDocument>(address)?;
                Ok(doc
                    .get_first(body)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string())
            })
            .collect()
    }
}
