use std::{env, error::Error, path::PathBuf};

use sturdy_engine_core::{ShaderStage, SlangCompileDesc, compile_slang_to_file};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // Vertex shaders use "vs_main" as entry point
    compile_shader_with_entry(
        manifest_dir.join("shaders").join("triangle_vertex.slang"),
        out_dir.join("triangle_vertex.spv"),
        ShaderStage::Vertex,
        "vs_main",
    )?;
    // Fragment shaders use "ps_main" as entry point
    compile_shader_with_entry(
        manifest_dir.join("shaders").join("triangle_fragment.slang"),
        out_dir.join("triangle_fragment.spv"),
        ShaderStage::Fragment,
        "ps_main",
    )?;
    compile_shader_with_entry(
        manifest_dir.join("shaders").join("textured_vertex.slang"),
        out_dir.join("textured_vertex.spv"),
        ShaderStage::Vertex,
        "vs_main",
    )?;
    compile_shader_with_entry(
        manifest_dir.join("shaders").join("textured_fragment.slang"),
        out_dir.join("textured_fragment.spv"),
        ShaderStage::Fragment,
        "ps_main",
    )?;
    compile_shader_with_entry(
        manifest_dir.join("shaders").join("push_vertex.slang"),
        out_dir.join("push_vertex.spv"),
        ShaderStage::Vertex,
        "vs_main",
    )?;
    compile_shader_with_entry(
        manifest_dir.join("shaders").join("push_fragment.slang"),
        out_dir.join("push_fragment.spv"),
        ShaderStage::Fragment,
        "ps_main",
    )?;


    Ok(())
}

fn compile_shader_with_entry(
    source: PathBuf,
    output: PathBuf,
    stage: ShaderStage,
    entry_point: &str,
) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={}", source.display());
    let mut desc = SlangCompileDesc::spirv(source, output);
    desc.stage = stage;
    desc.entry_point = entry_point.to_owned();
    compile_slang_to_file(&desc)?;
    Ok(())
}
