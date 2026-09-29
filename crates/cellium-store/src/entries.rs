use cellium_core::WorkbookId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbookListFilter {
    Recent,
    Starred,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbookLibraryEntry {
    pub id: WorkbookId,
    pub name: String,
    pub file_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_opened_at: i64,
    pub starred: bool,
}
