use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=STURDY_GENERATE_HEADER");
    println!("cargo:rerun-if-changed=src/lib.rs");

    if env::var_os("STURDY_GENERATE_HEADER").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("ffi crate should live under workspace/crates")
        .to_path_buf();
    let config = workspace_dir.join("cbindgen.toml");
    let output = workspace_dir.join("include/sturdy_engine.h");

    let status = Command::new("cbindgen")
        .arg("--config")
        .arg(config)
        .arg("--crate")
        .arg("sturdy-engine-ffi")
        .arg("--output")
        .arg(output)
        .current_dir(&workspace_dir)
        .status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => panic!("cbindgen failed with status {status}"),
        Err(error) => panic!("failed to run cbindgen: {error}"),
    }
}
