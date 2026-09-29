use super::*;

#[test]
fn fuzzy_filter_returns_matching_labels() {
    let items = vec![
        FuzzyItem {
            id: "1".to_string(),
            label: "Open File".to_string(),
        },
        FuzzyItem {
            id: "2".to_string(),
            label: "Save Workbook".to_string(),
        },
    ];

    assert_eq!(fuzzy_filter("open", &items).len(), 1);
}
