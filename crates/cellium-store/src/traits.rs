use cellium_core::{Workbook, WorkbookId};

use crate::{StoreError, WorkbookLibraryEntry, WorkbookListFilter};

pub trait WorkbookRepository {
    fn save_workbook(
        &self,
        workbook: &Workbook,
    ) -> impl std::future::Future<Output = Result<(), StoreError>>;

    fn load_workbook(
        &self,
        id: WorkbookId,
    ) -> impl std::future::Future<Output = Result<Workbook, StoreError>>;
}

pub trait LibraryRepository {
    fn upsert_workbook(
        &self,
        workbook: &WorkbookLibraryEntry,
    ) -> impl std::future::Future<Output = Result<(), StoreError>>;

    fn list_workbooks(
        &self,
        filter: WorkbookListFilter,
        limit: u32,
    ) -> impl std::future::Future<Output = Result<Vec<WorkbookLibraryEntry>, StoreError>>;
}
