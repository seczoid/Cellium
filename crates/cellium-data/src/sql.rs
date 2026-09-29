use duckdb::types::Value;

use cellium_core::{FilterExpr, FilterOperator, SortDirection, SortSpec, TableViewState};

use crate::{DataError, ROW_ID_COLUMN};

pub(crate) fn validate_identifier(identifier: &str) -> Result<&str, DataError> {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return Err(DataError::InvalidIdentifier(identifier.to_string()));
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
    {
        return Err(DataError::InvalidIdentifier(identifier.to_string()));
    }
    Ok(identifier)
}

pub(crate) fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn quote_sql_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(crate) fn compile_view_filter(
    view: &TableViewState,
    available_columns: &[String],
    row_id_range: Option<(u64, u64)>,
) -> Result<Option<String>, DataError> {
    let mut predicates = view
        .filters
        .iter()
        .map(|filter| compile_filter(filter, available_columns))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some((start, end)) = row_id_range {
        predicates.push(format!(
            "{row_id} BETWEEN {start} AND {end}",
            row_id = quote_sql_identifier(ROW_ID_COLUMN)
        ));
    }
    Ok((!predicates.is_empty()).then(|| predicates.join(" AND ")))
}

pub(crate) fn compile_view_sort(
    sorts: &[SortSpec],
    available_columns: &[String],
    include_row_id: bool,
) -> Result<Option<String>, DataError> {
    let mut clauses = sorts
        .iter()
        .map(|sort| compile_sort(sort, available_columns))
        .collect::<Result<Vec<_>, _>>()?;
    if include_row_id {
        clauses.push(format!("{} ASC", quote_sql_identifier(ROW_ID_COLUMN)));
    }
    Ok((!clauses.is_empty()).then(|| clauses.join(", ")))
}

fn compile_filter(filter: &FilterExpr, available_columns: &[String]) -> Result<String, DataError> {
    validate_view_column(&filter.column, available_columns)?;
    let column = quote_sql_identifier(&filter.column);
    let text_column = format!("CAST({column} AS VARCHAR)");
    let predicate = match filter.operator {
        FilterOperator::Contains => {
            let value = required_filter_value(filter)?;
            format!(
                "contains(lower({text_column}), lower({}))",
                sql_string_literal(value)
            )
        }
        FilterOperator::Equals => {
            let value = required_filter_value(filter)?;
            format!("{text_column} = {}", sql_string_literal(value))
        }
        FilterOperator::NotEquals => {
            let value = required_filter_value(filter)?;
            format!("{text_column} <> {}", sql_string_literal(value))
        }
        FilterOperator::StartsWith => {
            let value = required_filter_value(filter)?;
            format!(
                "starts_with(lower({text_column}), lower({}))",
                sql_string_literal(value)
            )
        }
        FilterOperator::EndsWith => {
            let value = required_filter_value(filter)?;
            format!(
                "ends_with(lower({text_column}), lower({}))",
                sql_string_literal(value)
            )
        }
        FilterOperator::GreaterThan => numeric_predicate(filter, &column, ">")?,
        FilterOperator::GreaterThanOrEqual => numeric_predicate(filter, &column, ">=")?,
        FilterOperator::LessThan => numeric_predicate(filter, &column, "<")?,
        FilterOperator::LessThanOrEqual => numeric_predicate(filter, &column, "<=")?,
        FilterOperator::IsEmpty => {
            format!("({column} IS NULL OR {text_column} = '')")
        }
        FilterOperator::IsNotEmpty => {
            format!("({column} IS NOT NULL AND {text_column} <> '')")
        }
    };
    Ok(predicate)
}

fn compile_sort(sort: &SortSpec, available_columns: &[String]) -> Result<String, DataError> {
    validate_view_column(&sort.column, available_columns)?;
    let direction = match sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    Ok(format!(
        "{} {direction} NULLS LAST",
        quote_sql_identifier(&sort.column)
    ))
}

fn validate_view_column(column: &str, available_columns: &[String]) -> Result<(), DataError> {
    if available_columns
        .iter()
        .any(|candidate| candidate == column)
    {
        Ok(())
    } else {
        Err(DataError::UnknownColumn(column.to_string()))
    }
}

fn required_filter_value(filter: &FilterExpr) -> Result<&str, DataError> {
    filter
        .value
        .as_deref()
        .ok_or_else(|| DataError::MissingFilterValue(filter.column.clone()))
}

fn numeric_predicate(
    filter: &FilterExpr,
    quoted_column: &str,
    operator: &str,
) -> Result<String, DataError> {
    let value = required_filter_value(filter)?;
    let number = value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or_else(|| DataError::InvalidNumericFilter(value.to_string()))?;
    Ok(format!(
        "TRY_CAST({quoted_column} AS DOUBLE) {operator} {number}"
    ))
}

pub(crate) fn value_to_grid_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Boolean(value) => value.to_string(),
        Value::TinyInt(value) => value.to_string(),
        Value::SmallInt(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::BigInt(value) => value.to_string(),
        Value::HugeInt(value) => value.to_string(),
        Value::UTinyInt(value) => value.to_string(),
        Value::USmallInt(value) => value.to_string(),
        Value::UInt(value) => value.to_string(),
        Value::UBigInt(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Double(value) => value.to_string(),
        Value::Decimal(value) => value.to_string(),
        Value::Timestamp(_, value) => value.to_string(),
        Value::Text(value) => value.clone(),
        Value::Blob(value) => format!("{value:?}"),
        Value::Date32(value) => value.to_string(),
        Value::Time64(_, value) => value.to_string(),
        Value::Interval {
            months,
            days,
            nanos,
        } => format!("{months} months {days} days {nanos} ns"),
        Value::List(values) | Value::Array(values) => format!("{values:?}"),
        Value::Enum(value) => value.clone(),
        Value::Struct(values) => format!("{values:?}"),
        Value::Map(values) => format!("{values:?}"),
        Value::Union(value) => format!("{value:?}"),
    }
}
