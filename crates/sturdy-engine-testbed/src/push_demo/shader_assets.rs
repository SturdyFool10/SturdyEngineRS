use sturdy_engine::{spirv_words_from_bytes, Error, Result};

pub fn included_spirv(name: &str) -> Result<Vec<u32>> {
    match name {
        "push_vertex.spv" => {
            spirv_words_from_bytes(include_bytes!(concat!(env!("OUT_DIR"), "/push_vertex.spv")))
        }
        "push_fragment.spv" => spirv_words_from_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/push_fragment.spv"
        ))),
        _ => Err(Error::InvalidInput(format!(
            "unknown included SPIR-V: {name}"
        ))),
    }
}
