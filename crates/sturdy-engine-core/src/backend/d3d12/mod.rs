mod config;

use crate::backend::BackendKind;

pub use config::D3d12BackendConfig;

pub const KIND: BackendKind = BackendKind::D3d12;
