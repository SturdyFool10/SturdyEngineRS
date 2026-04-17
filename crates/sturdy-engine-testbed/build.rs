use std::{env, error::Error, path::PathBuf};

use sturdy_engine_core::{ShaderStage, SlangCompileDesc, compile_slang_to_file};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    compile_shader(
        manifest_dir.join("shaders").join("triangle_vertex.slang"),
        out_dir.join("triangle_vertex.spv"),
        ShaderStage::Vertex,
    )?;
    compile_shader(
        manifest_dir.join("shaders").join("triangle_fragment.slang"),
        out_dir.join("triangle_fragment.spv"),
        ShaderStage::Fragment,
    )?;

    Ok(())
}

fn compile_shader(
    source: PathBuf,
    output: PathBuf,
    stage: ShaderStage,
) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={}", source.display());
    let mut desc = SlangCompileDesc::spirv(source, output);
    desc.stage = stage;
    compile_slang_to_file(&desc)?;
    Ok(())
}
