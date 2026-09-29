//! Local workbook persistence backed by libsql.

mod entries;
mod error;
mod local;
mod sync;
mod traits;

pub use entries::{WorkbookLibraryEntry, WorkbookListFilter};
pub use error::StoreError;
pub use local::LocalWorkbookRepository;
pub use sync::SyncStatus;
pub use traits::{LibraryRepository, WorkbookRepository};
