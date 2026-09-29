#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyItem {
    pub id: String,
    pub label: String,
}

#[must_use]
pub fn fuzzy_filter<'a>(query: &str, items: &'a [FuzzyItem]) -> Vec<&'a FuzzyItem> {
    let query = query.to_ascii_lowercase();
    let _matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
    items
        .iter()
        .filter(|item| item.label.to_ascii_lowercase().contains(&query))
        .collect()
}
