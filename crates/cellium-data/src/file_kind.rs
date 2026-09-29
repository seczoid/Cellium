use std::path::Path;

use memmap2::MmapOptions;

use crate::{DataError, DataFileKind};

pub fn sniff_file_kind(path: &Path) -> Result<DataFileKind, DataError> {
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        match extension.to_ascii_lowercase().as_str() {
            "csv" => return Ok(DataFileKind::Csv),
            "parquet" => return Ok(DataFileKind::Parquet),
            "arrow" | "ipc" | "feather" => return Ok(DataFileKind::Arrow),
            _ => {}
        }
    }

    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len().min(8) as usize;
    let map = unsafe { MmapOptions::new().len(len).map(&file)? };
    if map.starts_with(b"PAR1") {
        return Ok(DataFileKind::Parquet);
    }
    Err(DataError::UnsupportedFile)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::Path, sync::Arc};

    use arrow::{array::ArrayRef, record_batch::RecordBatch};
    use parquet::arrow::ArrowWriter;

    use super::*;

    #[test]
    fn sniff_file_kind_detects_csv_extension() {
        assert_eq!(
            sniff_file_kind(Path::new("sample.csv")).unwrap(),
            DataFileKind::Csv
        );
    }

    #[test]
    fn sniff_file_kind_uses_parquet_magic_when_extension_is_missing() {
        let directory = temp_fixture_dir("sniff");
        let path = directory.join("orders.data");
        let batch = RecordBatch::try_from_iter(vec![(
            "id",
            Arc::new(arrow::array::Int64Array::from(vec![1, 2])) as ArrayRef,
        )])
        .unwrap();
        let file = File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        assert_eq!(sniff_file_kind(&path).unwrap(), DataFileKind::Parquet);
    }

    fn temp_fixture_dir(label: &str) -> std::path::PathBuf {
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
