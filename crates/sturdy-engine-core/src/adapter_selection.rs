use crate::AdapterKind;

/// Controls which physical adapter the engine picks at device creation time.
#[derive(Clone, Debug, Default)]
pub enum AdapterSelection {
    /// Let the engine pick the best available adapter (prefers discrete GPU).
    #[default]
    Auto,
    /// Pick by zero-based enumeration index.
    ByIndex(usize),
    /// Pick the first adapter whose name contains the given substring.
    ByName(String),
    /// Pick the first adapter with the given PCI vendor ID.
    ByVendorId(u32),
    /// Pick the first adapter of the given kind.
    ByKind(AdapterKind),
}
