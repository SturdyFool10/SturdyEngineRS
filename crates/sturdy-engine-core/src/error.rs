use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorCategory {
    HardIncompatible,
    Unsupported,
    Degraded,
    InvalidInput,
    BackendFailure,
    PlatformFailure,
    ResourceStateCorruption,
    /// GPU device is lost and cannot recover — process must exit or recreate the device.
    DeviceLost,
    /// Swapchain / surface is no longer valid — caller should recreate the surface and retry.
    SurfaceLost,
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Error {
    InvalidHandle,
    HardIncompatible(String),
    Unsupported(&'static str),
    Degraded(String),
    CompileFailed(String),
    OutOfMemory,
    InvalidInput(String),
    Backend(String),
    Platform(String),
    ResourceStateCorruption(String),
    /// The GPU device was lost (VK_ERROR_DEVICE_LOST or equivalent).
    ///
    /// The device and all resources it owns are no longer usable. The application
    /// must exit or re-initialise the engine from scratch. Contains a diagnostic
    /// hint from the backend (e.g. the VK_EXT_device_fault reason, if available).
    DeviceLost(String),
    /// The swapchain or window surface is no longer valid.
    ///
    /// This is a recoverable, transient condition: the caller should call
    /// `Surface::recreate(...)` and retry the frame. Common causes: window
    /// resize, monitor change, driver-initiated swapchain invalidation.
    SurfaceLost(String),
    Unknown(String),
}

impl Error {
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::InvalidHandle => ErrorCategory::ResourceStateCorruption,
            Self::HardIncompatible(_) => ErrorCategory::HardIncompatible,
            Self::Unsupported(_) => ErrorCategory::Unsupported,
            Self::Degraded(_) => ErrorCategory::Degraded,
            Self::CompileFailed(_) => ErrorCategory::InvalidInput,
            Self::OutOfMemory => ErrorCategory::BackendFailure,
            Self::InvalidInput(_) => ErrorCategory::InvalidInput,
            Self::Backend(_) => ErrorCategory::BackendFailure,
            Self::Platform(_) => ErrorCategory::PlatformFailure,
            Self::ResourceStateCorruption(_) => ErrorCategory::ResourceStateCorruption,
            Self::DeviceLost(_) => ErrorCategory::DeviceLost,
            Self::SurfaceLost(_) => ErrorCategory::SurfaceLost,
            Self::Unknown(_) => ErrorCategory::Unknown,
        }
    }

    /// Returns `true` when the GPU device is unrecoverably lost.
    pub fn is_device_lost(&self) -> bool {
        matches!(self, Self::DeviceLost(_))
    }

    /// Returns `true` when only the surface/swapchain is invalid (recoverable by recreate).
    pub fn is_surface_lost(&self) -> bool {
        matches!(self, Self::SurfaceLost(_))
    }

    pub fn code(&self) -> i32 {
        match self {
            Self::InvalidHandle => 1,
            Self::HardIncompatible(_) => 7,
            Self::Unsupported(_) => 2,
            Self::Degraded(_) => 8,
            Self::CompileFailed(_) => 3,
            Self::OutOfMemory => 4,
            Self::InvalidInput(_) => 5,
            Self::Backend(_) => 6,
            Self::Platform(_) => 9,
            Self::ResourceStateCorruption(_) => 10,
            Self::DeviceLost(_) => 11,
            Self::SurfaceLost(_) => 12,
            Self::Unknown(_) => 0x7fff_ffff,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHandle => write!(f, "invalid handle"),
            Self::HardIncompatible(msg) => write!(f, "hard incompatible: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            Self::Degraded(msg) => write!(f, "degraded: {msg}"),
            Self::CompileFailed(msg) => write!(f, "shader compile failed: {msg}"),
            Self::OutOfMemory => write!(f, "out of memory"),
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
            Self::Platform(msg) => write!(f, "platform error: {msg}"),
            Self::ResourceStateCorruption(msg) => write!(f, "resource state corruption: {msg}"),
            Self::DeviceLost(msg) => write!(f, "device lost: {msg}"),
            Self::SurfaceLost(msg) => write!(f, "surface lost: {msg}"),
            Self::Unknown(msg) => write!(f, "unknown error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Error, ErrorCategory};

    #[test]
    fn errors_report_stable_categories() {
        let cases = [
            (Error::InvalidHandle, ErrorCategory::ResourceStateCorruption),
            (
                Error::HardIncompatible("missing backend".into()),
                ErrorCategory::HardIncompatible,
            ),
            (Error::Unsupported("feature"), ErrorCategory::Unsupported),
            (Error::Degraded("fallback".into()), ErrorCategory::Degraded),
            (
                Error::CompileFailed("bad shader".into()),
                ErrorCategory::InvalidInput,
            ),
            (Error::OutOfMemory, ErrorCategory::BackendFailure),
            (
                Error::InvalidInput("bad descriptor".into()),
                ErrorCategory::InvalidInput,
            ),
            (
                Error::Backend("driver failure".into()),
                ErrorCategory::BackendFailure,
            ),
            (
                Error::Platform("window failure".into()),
                ErrorCategory::PlatformFailure,
            ),
            (
                Error::ResourceStateCorruption("missing allocation".into()),
                ErrorCategory::ResourceStateCorruption,
            ),
            (Error::DeviceLost("gpu hang".into()), ErrorCategory::DeviceLost),
            (Error::SurfaceLost("out of date".into()), ErrorCategory::SurfaceLost),
            (Error::Unknown("mystery".into()), ErrorCategory::Unknown),
        ];

        for (error, category) in cases {
            assert_eq!(error.category(), category);
        }
    }
}
