mod config;

use crate::backend::BackendKind;

pub use config::MetalBackendConfig;

pub const KIND: BackendKind = BackendKind::Metal;
