use std::env;
use std::path::PathBuf;
use std::process::{self, Command};

fn main() {
    println!("cargo:rerun-if-env-changed=STURDY_GENERATE_HEADER");
    println!("cargo:rerun-if-changed=src/lib.rs");

    if let Err(error) = run() {
        println!("cargo:error={error}");
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    if env::var_os("STURDY_GENERATE_HEADER").is_none() {
        return Ok(());
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .map_err(|error| {
            format!("STURDY_GENERATE_HEADER=1 requires CARGO_MANIFEST_DIR: {error}")
        })?;
    let workspace_dir = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .ok_or_else(|| {
            format!(
                "STURDY_GENERATE_HEADER=1 could not infer workspace root from ffi crate path: {}",
                manifest_dir.display()
            )
        })?;

    let config = workspace_dir.join("cbindgen.toml");
    let output = workspace_dir.join("include/sturdy_engine.h");
    let command = format!(
        "cbindgen --config {} --crate sturdy-engine-ffi --output {}",
        config.display(),
        output.display()
    );

    let status = Command::new("cbindgen")
        .arg("--config")
        .arg(&config)
        .arg("--crate")
        .arg("sturdy-engine-ffi")
        .arg("--output")
        .arg(&output)
        .current_dir(&workspace_dir)
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "cbindgen header generation failed with status {status}; output={}; command={command}",
            output.display()
        )),
        Err(error) => Err(format!(
            "failed to run cbindgen for header generation: {error}; output={}; command={command}",
            output.display()
        )),
    }
}
