use std::{
    fs::File,
    path::{Path, PathBuf},
};

use arrow::ipc::reader::FileReader as ArrowFileReader;
use memmap2::MmapOptions;
use parquet::file::reader::{FileReader as ParquetFileReader, SerializedFileReader};

use crate::{DataError, DataFileKind, async_engine::spawn_data_task, sniff_file_kind};

pub async fn exact_file_row_count(path: PathBuf) -> Result<u64, DataError> {
    spawn_data_task(move || exact_file_row_count_blocking(&path)).await
}

pub fn exact_file_row_count_blocking(path: &Path) -> Result<u64, DataError> {
    match sniff_file_kind(path)? {
        DataFileKind::Csv => exact_csv_row_count(path),
        DataFileKind::Parquet => exact_parquet_row_count(path),
        DataFileKind::Arrow => exact_arrow_ipc_row_count(path),
    }
}

fn exact_csv_row_count(path: &Path) -> Result<u64, DataError> {
    let file = File::open(path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(0);
    }
    let map = unsafe { MmapOptions::new().map(&file)? };
    Ok(csv_data_row_count(&map))
}

fn csv_data_row_count(bytes: &[u8]) -> u64 {
    csv_record_count(bytes).saturating_sub(1)
}

fn csv_record_count(bytes: &[u8]) -> u64 {
    let mut records = 0_u64;
    let mut in_quotes = false;
    let mut seen_byte_in_record = false;
    let mut index = 0_usize;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                if in_quotes && bytes.get(index + 1) == Some(&b'"') {
                    index += 1;
                } else {
                    in_quotes = !in_quotes;
                }
                seen_byte_in_record = true;
            }
            b'\n' if !in_quotes => {
                records = records.saturating_add(1);
                seen_byte_in_record = false;
            }
            b'\r' if !in_quotes => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
                records = records.saturating_add(1);
                seen_byte_in_record = false;
            }
            _ => {
                seen_byte_in_record = true;
            }
        }
        index += 1;
    }

    if seen_byte_in_record {
        records = records.saturating_add(1);
    }
    records
}

fn exact_parquet_row_count(path: &Path) -> Result<u64, DataError> {
    let file = File::open(path)?;
    let reader = SerializedFileReader::new(file)?;
    let rows = reader.metadata().file_metadata().num_rows();
    u64::try_from(rows).map_err(|_| DataError::NegativeRowCount(rows))
}

fn exact_arrow_ipc_row_count(path: &Path) -> Result<u64, DataError> {
    let file = File::open(path)?;
    let reader = ArrowFileReader::try_new(file, None)?;
    let mut row_count = 0_u64;
    for batch in reader {
        row_count = row_count
            .checked_add(u64::try_from(batch?.num_rows()).map_err(|_| DataError::RowCountOverflow)?)
            .ok_or(DataError::RowCountOverflow)?;
    }
    Ok(row_count)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, sync::Arc};

    use arrow::{
        array::{ArrayRef, Float64Array, Int64Array},
        record_batch::RecordBatch,
    };
    use parquet::arrow::ArrowWriter;

    use super::*;

    #[test]
    fn exact_file_row_count_counts_csv_with_trailing_newline() {
        let directory = temp_fixture_dir("csv-count-trailing");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();

        assert_eq!(exact_file_row_count_blocking(&path).unwrap(), 2);
    }

    #[test]
    fn exact_file_row_count_counts_csv_without_trailing_newline() {
        let directory = temp_fixture_dir("csv-count-no-trailing");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0").unwrap();

        assert_eq!(exact_file_row_count_blocking(&path).unwrap(), 2);
    }

    #[test]
    fn exact_file_row_count_counts_csv_with_quoted_newline() {
        let directory = temp_fixture_dir("csv-count-quoted-newline");
        let path = directory.join("notes.csv");
        std::fs::write(&path, "id,note\n1,\"hello\nworld\"\n2,done\n").unwrap();

        assert_eq!(exact_file_row_count_blocking(&path).unwrap(), 2);
    }

    #[test]
    fn exact_file_row_count_reads_parquet_metadata() {
        let directory = temp_fixture_dir("parquet-count");
        let path = directory.join("orders.parquet");
        let batch = orders_batch();
        let file = File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        assert_eq!(exact_file_row_count_blocking(&path).unwrap(), 2);
    }

    fn orders_batch() -> RecordBatch {
        RecordBatch::try_from_iter(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2])) as ArrayRef),
            (
                "amount",
                Arc::new(Float64Array::from(vec![12.5, 25.0])) as ArrayRef,
            ),
        ])
        .unwrap()
    }

    fn temp_fixture_dir(label: &str) -> PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("cellium-data-{label}-{}-{now}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
