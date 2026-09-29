use std::path::Path;

use anyhow::Error;
use cellium_store::WorkbookLibraryEntry;
use cellium_ui::DashboardWorkbook;

use crate::ids::unix_seconds;

pub fn projected_columns(columns: &[String], start_column: u32, column_count: u32) -> Vec<String> {
    columns
        .iter()
        .skip(start_column as usize)
        .take(column_count as usize)
        .cloned()
        .collect()
}

pub fn table_name_for_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("table");
    let mut table_name = String::with_capacity(stem.len() + 8);
    table_name.push_str("import_");
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            table_name.push(character.to_ascii_lowercase());
        } else {
            table_name.push('_');
        }
    }
    if table_name == "import_" {
        table_name.push_str("table");
    }
    table_name
}

pub fn dashboard_workbook(entry: WorkbookLibraryEntry) -> DashboardWorkbook {
    DashboardWorkbook {
        id: entry.id.0,
        name: entry.name,
        file_path: entry.file_path,
        updated_label: relative_time_label(entry.last_opened_at),
        starred: entry.starred,
    }
}

pub fn relative_time_label(timestamp: i64) -> String {
    let elapsed = unix_seconds().saturating_sub(timestamp).max(0);
    match elapsed {
        0..=59 => "Moments ago".to_string(),
        60..=3_599 => format!("{} minutes ago", elapsed / 60),
        3_600..=86_399 => format!("{} hours ago", elapsed / 3_600),
        _ => format!("{} days ago", elapsed / 86_400),
    }
}

pub fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), ToString::to_string)
}

pub fn format_error_chain(error: &Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_columns_returns_requested_band() {
        let columns = ["a", "b", "c", "d"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();

        assert_eq!(
            projected_columns(&columns, 1, 2),
            vec!["b".to_string(), "c".to_string()]
        );
    }
}
