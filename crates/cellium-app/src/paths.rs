use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use cellium_core::WorkbookId;

pub fn library_index_path() -> Result<PathBuf> {
    let dir = cellium_data_dir();
    std::fs::create_dir_all(&dir).context("failed to create Cellium app data directory")?;
    Ok(dir.join("library.db"))
}

pub fn cellium_data_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("Cellium");
    }
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("cellium");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Cellium");
    }
    std::env::temp_dir().join("cellium")
}

pub fn default_workbook_path(id: WorkbookId, name: &str) -> PathBuf {
    cellium_data_dir().join("workbooks").join(format!(
        "{}-{}.cellium",
        id.0,
        sanitized_file_stem(name)
    ))
}

pub fn session_database_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("cellium-{}-{timestamp}.duckdb", std::process::id()))
}

fn sanitized_file_stem(name: &str) -> String {
    let stem = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let stem = stem.trim_matches('-').chars().take(48).collect::<String>();
    if stem.is_empty() {
        "workbook".to_string()
    } else {
        stem
    }
}
