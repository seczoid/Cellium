use std::sync::Arc;

use arrow::{
    array::{Array, ArrayRef, StringArray},
    datatypes::{DataType, Field, Schema, SchemaRef},
    record_batch::RecordBatch,
};

use crate::DataError;

#[derive(Debug, Clone)]
pub struct ArrowWindow {
    pub schema: SchemaRef,
    pub batch: RecordBatch,
    pub offset: u64,
    pub row_ids: Vec<Option<u64>>,
}

impl ArrowWindow {
    pub(crate) fn from_columns(
        names: Vec<String>,
        values: Vec<Vec<String>>,
        offset: u64,
        row_ids: Vec<Option<u64>>,
    ) -> Result<Self, DataError> {
        let fields = names
            .iter()
            .map(|name| Field::new(name, DataType::Utf8, true))
            .collect::<Vec<_>>();
        let schema = Arc::new(Schema::new(fields));
        let arrays = values
            .into_iter()
            .map(|values| Arc::new(StringArray::from(values)) as ArrayRef)
            .collect::<Vec<_>>();
        let batch = RecordBatch::try_new(schema.clone(), arrays)?;
        Ok(Self {
            schema,
            batch,
            offset,
            row_ids,
        })
    }

    #[must_use]
    pub fn column_names(&self) -> Vec<String> {
        self.schema
            .fields()
            .iter()
            .map(|field| field.name().to_string())
            .collect()
    }

    #[must_use]
    pub fn rows_as_strings(&self) -> Vec<Vec<String>> {
        (0..self.batch.num_rows())
            .map(|row| {
                self.batch
                    .columns()
                    .iter()
                    .map(|column| string_array_value(column, row).unwrap_or_default())
                    .collect()
            })
            .collect()
    }
}

pub(crate) fn string_array_value(column: &ArrayRef, row: usize) -> Option<String> {
    if column.is_null(row) {
        return None;
    }
    column
        .as_any()
        .downcast_ref::<StringArray>()
        .map(|array| array.value(row).to_string())
}
