use thiserror::Error;

use crate::{ColumnId, SheetId, TableId};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("sheet `{0:?}` was not found")]
    SheetNotFound(SheetId),
    #[error("sheet `{0:?}` already exists")]
    SheetAlreadyExists(SheetId),
    #[error("connected table `{0:?}` was not found")]
    TableNotFound(TableId),
    #[error("connected table `{0:?}` already exists")]
    TableAlreadyExists(TableId),
    #[error("computed column `{0:?}` was not found")]
    ComputedColumnNotFound(ColumnId),
    #[error("computed column `{0:?}` already exists")]
    ComputedColumnAlreadyExists(ColumnId),
    #[error("saved view `{0}` was not found")]
    SavedViewNotFound(String),
    #[error("saved view `{0}` already exists")]
    SavedViewAlreadyExists(String),
    #[error("no command is available to undo")]
    NothingToUndo,
    #[error("no command is available to redo")]
    NothingToRedo,
}
