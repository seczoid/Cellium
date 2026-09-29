use cellium_core::{CellRef, CellValue};

pub trait CellLookup {
    fn value_at(&self, reference: &CellRef) -> CellValue;
}

impl<F> CellLookup for F
where
    F: Fn(&CellRef) -> CellValue,
{
    fn value_at(&self, reference: &CellRef) -> CellValue {
        self(reference)
    }
}
