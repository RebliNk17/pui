//! Fixed-capacity selection: None means whole file; Some(0) means no hunks selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection(pub Option<u64>);
impl Selection {
    pub(crate) fn contains(self, index: usize) -> bool {
        index < 64 && self.0.is_some_and(|bits| bits & (1u64 << index) != 0)
    }
    pub(crate) fn toggle(&mut self, index: usize) {
        if index < 64 {
            self.0 = Some(self.0.unwrap_or(0) ^ (1u64 << index));
        }
    }
    pub(crate) fn valid(self, count: usize) -> bool {
        self.0.is_none_or(|bits| {
            bits != 0 && count > 0 && count <= 64 && (count == 64 || bits >> count == 0)
        })
    }
}
