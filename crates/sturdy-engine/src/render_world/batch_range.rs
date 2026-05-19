/// Contiguous slice of the render-world GPU scene buffer for one mesh batch.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderWorldBatchRange {
    pub base: u32,
    pub count: u32,
}

impl RenderWorldBatchRange {
    pub const fn new(base: u32, count: u32) -> Self {
        Self { base, count }
    }

    pub const fn is_empty(self) -> bool {
        self.count == 0
    }
}
