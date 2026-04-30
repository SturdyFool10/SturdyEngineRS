use std::collections::HashMap;

use crate::{VertexFormat, VertexInputReflection};

// SPIR-V opcodes
const OP_NAME: u16 = 5;
const OP_DECORATE: u16 = 71;
const OP_TYPE_FLOAT: u16 = 22;
const OP_TYPE_INT: u16 = 21;
const OP_TYPE_VECTOR: u16 = 23;
const OP_TYPE_POINTER: u16 = 32;
const OP_VARIABLE: u16 = 59;

// Decoration values
const DECORATION_LOCATION: u32 = 30;
const DECORATION_BUILTIN: u32 = 11;

// Storage class values
const STORAGE_CLASS_INPUT: u32 = 1;

/// One parsed input variable before format resolution.
#[derive(Debug)]
struct InputVar {
    type_id: u32,
    name: Option<String>,
}

/// Intermediate type representation (only what we need for format derivation).
#[derive(Debug, Clone)]
enum SpirvType {
    Float { width: u32 },
    Int { width: u32 },
    Vector { component_type_id: u32, count: u32 },
    Pointer { storage_class: u32, pointee_id: u32 },
    Other,
}

/// Reflect vertex shader input attributes from SPIR-V words.
///
/// Returns one entry per non-built-in `Input` variable, sorted by location.
/// Attributes whose format cannot be determined are silently skipped.
pub fn reflect_spirv_vertex_inputs(words: &[u32]) -> Vec<VertexInputReflection> {
    if words.len() < 5 || words[0] != 0x0723_0203 {
        return Vec::new();
    }

    let mut types: HashMap<u32, SpirvType> = HashMap::new();
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut locations: HashMap<u32, u32> = HashMap::new(); // id → location
    let mut builtins: std::collections::HashSet<u32> = std::collections::HashSet::new(); // ids with BuiltIn
    let mut input_vars: HashMap<u32, InputVar> = HashMap::new(); // id → InputVar

    let mut index = 5usize;
    while index < words.len() {
        let first_word = words[index];
        let word_count = (first_word >> 16) as usize;
        let opcode = (first_word & 0xffff) as u16;
        if word_count == 0 || index + word_count > words.len() {
            break;
        }
        let instr = &words[index..index + word_count];
        index += word_count;

        match opcode {
            OP_NAME if word_count >= 3 => {
                let target_id = instr[1];
                let name = decode_string(&instr[2..]);
                if !name.is_empty() {
                    names.insert(target_id, name);
                }
            }
            OP_TYPE_FLOAT if word_count >= 3 => {
                let result_id = instr[1];
                let width = instr[2];
                types.insert(result_id, SpirvType::Float { width });
            }
            OP_TYPE_INT if word_count >= 4 => {
                let result_id = instr[1];
                let width = instr[2];
                types.insert(result_id, SpirvType::Int { width });
            }
            OP_TYPE_VECTOR if word_count >= 4 => {
                let result_id = instr[1];
                let component_type_id = instr[2];
                let count = instr[3];
                types.insert(result_id, SpirvType::Vector { component_type_id, count });
            }
            OP_TYPE_POINTER if word_count >= 4 => {
                let result_id = instr[1];
                let storage_class = instr[2];
                let pointee_id = instr[3];
                types.insert(result_id, SpirvType::Pointer { storage_class, pointee_id });
            }
            OP_VARIABLE if word_count >= 4 => {
                let type_id = instr[1];
                let result_id = instr[2];
                let storage_class = instr[3];
                if storage_class == STORAGE_CLASS_INPUT {
                    input_vars.insert(result_id, InputVar { type_id, name: None });
                }
            }
            OP_DECORATE if word_count >= 3 => {
                let target_id = instr[1];
                let decoration = instr[2];
                if decoration == DECORATION_LOCATION && word_count >= 4 {
                    locations.insert(target_id, instr[3]);
                } else if decoration == DECORATION_BUILTIN {
                    builtins.insert(target_id);
                }
            }
            _ => {}
        }
    }

    // Resolve names into InputVars.
    for (id, var) in input_vars.iter_mut() {
        var.name = names.get(id).cloned();
    }

    // Build the result.
    let mut results: Vec<VertexInputReflection> = Vec::new();
    for (var_id, var) in &input_vars {
        // Skip built-in variables (gl_Position, gl_VertexIndex, etc.).
        if builtins.contains(var_id) {
            continue;
        }
        let Some(&location) = locations.get(var_id) else {
            continue;
        };
        let Some(format) = resolve_format(&types, var.type_id) else {
            continue;
        };
        results.push(VertexInputReflection {
            name: var.name.clone().unwrap_or_else(|| format!("_input_{location}")),
            location,
            format,
        });
    }

    results.sort_by_key(|r| r.location);
    results
}

/// Follow the type chain to determine the `VertexFormat`.
///
/// Returns `None` for types we can't map (matrices, integers wider than 32 bits, etc.).
fn resolve_format(types: &HashMap<u32, SpirvType>, type_id: u32) -> Option<VertexFormat> {
    match types.get(&type_id)? {
        SpirvType::Pointer { storage_class, pointee_id } => {
            if *storage_class != STORAGE_CLASS_INPUT {
                return None;
            }
            resolve_format(types, *pointee_id)
        }
        SpirvType::Float { width: 32 } => None, // scalar float — no direct mapping, skip
        SpirvType::Int { .. } => None,
        SpirvType::Vector { component_type_id, count } => {
            let component = types.get(component_type_id)?;
            match (component, *count) {
                (SpirvType::Float { width: 32 }, 2) => Some(VertexFormat::Float32x2),
                (SpirvType::Float { width: 32 }, 3) => Some(VertexFormat::Float32x3),
                (SpirvType::Float { width: 32 }, 4) => Some(VertexFormat::Float32x4),
                _ => None,
            }
        }
        _ => None,
    }
}

fn decode_string(words: &[u32]) -> String {
    let bytes: Vec<u8> = words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    bytes
        .split(|&b| b == 0)
        .next()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
        .to_owned()
}
