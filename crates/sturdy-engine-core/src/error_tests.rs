// Tests extracted from crates/sturdy-engine-core/src/error.rs
// See scripts/extract_tests.py for the extraction logic.

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
        (
            Error::DeviceLost("gpu hang".into()),
            ErrorCategory::DeviceLost,
        ),
        (
            Error::SurfaceLost("out of date".into()),
            ErrorCategory::SurfaceLost,
        ),
        (Error::Unknown("mystery".into()), ErrorCategory::Unknown),
    ];

    for (error, category) in cases {
        assert_eq!(error.category(), category);
    }
}
