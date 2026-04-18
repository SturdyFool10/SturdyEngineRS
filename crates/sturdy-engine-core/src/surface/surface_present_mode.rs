#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum SurfacePresentMode {
    /// Queue; guaranteed available. One image presented per vblank.
    #[default]
    Fifo,
    /// Triple-buffer; lowest latency, may tear if unavailable.
    Mailbox,
    /// Immediate; no wait, may tear.
    Immediate,
    /// Like FIFO but may tear if a frame is late.
    RelaxedFifo,
}
