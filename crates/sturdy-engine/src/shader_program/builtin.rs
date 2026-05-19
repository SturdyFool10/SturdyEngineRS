use std::path::PathBuf;

pub(crate) fn builtin_shader_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}
