use crate::{DataError, DataFileKind};

pub(crate) fn i64_from_u64(value: u64) -> Result<i64, DataError> {
    i64::try_from(value).map_err(|_| DataError::IdOverflow(value))
}

pub(crate) fn u64_from_i64(value: i64) -> Result<u64, DataError> {
    u64::try_from(value).map_err(|_| DataError::NegativeMetadataId(value))
}

pub(crate) fn option_i64_from_u64(value: Option<u64>) -> Result<Option<i64>, DataError> {
    value.map(i64_from_u64).transpose()
}

pub(crate) fn source_kind_label(kind: DataFileKind) -> &'static str {
    match kind {
        DataFileKind::Csv => "csv",
        DataFileKind::Parquet => "parquet",
        DataFileKind::Arrow => "arrow",
    }
}

pub(crate) fn source_kind_from_label(label: String) -> Result<DataFileKind, DataError> {
    match label.as_str() {
        "csv" => Ok(DataFileKind::Csv),
        "parquet" => Ok(DataFileKind::Parquet),
        "arrow" => Ok(DataFileKind::Arrow),
        _ => Err(DataError::UnknownSourceKind(label)),
    }
}
