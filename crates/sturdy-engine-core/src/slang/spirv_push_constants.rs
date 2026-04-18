use std::collections::{BTreeMap, BTreeSet};

const OP_NAME: u16 = 5;
const OP_MEMBER_DECORATE: u16 = 72;
const OP_TYPE_INT: u16 = 21;
const OP_TYPE_FLOAT: u16 = 22;
const OP_TYPE_VECTOR: u16 = 23;
const OP_TYPE_MATRIX: u16 = 24;
const OP_TYPE_ARRAY: u16 = 28;
const OP_TYPE_RUNTIME_ARRAY: u16 = 29;
const OP_TYPE_STRUCT: u16 = 30;
const OP_TYPE_POINTER: u16 = 32;
const OP_CONSTANT: u16 = 43;
const OP_VARIABLE: u16 = 59;

const DECORATION_OFFSET: u32 = 35;
const DECORATION_MATRIX_STRIDE: u32 = 7;
const STORAGE_CLASS_PUSH_CONSTANT: u32 = 9;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct PushConstantReflection {
    pub bytes: u32,
    pub names: BTreeSet<String>,
}

#[derive(Debug)]
enum Type {
    Scalar { bytes: u32 },
    Vector { component_id: u32, count: u32 },
    Matrix { column_type_id: u32, count: u32 },
    Array { element_id: u32, length_id: u32 },
    RuntimeArray { element_id: u32 },
    Struct { member_ids: Vec<u32> },
    Pointer { pointee_id: u32 },
}

#[derive(Copy, Clone, Debug, Default)]
struct MemberLayout {
    offset: Option<u32>,
    matrix_stride: Option<u32>,
}

#[derive(Debug, Default)]
struct Module {
    names: BTreeMap<u32, String>,
    types: BTreeMap<u32, Type>,
    constants: BTreeMap<u32, u32>,
    member_layouts: BTreeMap<(u32, u32), MemberLayout>,
    push_constant_variables: Vec<(u32, u32)>,
}

pub fn reflect_spirv_push_constants(words: &[u32]) -> PushConstantReflection {
    let Some(module) = parse_module(words) else {
        return PushConstantReflection::default();
    };

    let mut reflection = PushConstantReflection::default();
    for (variable_type_id, variable_id) in module.push_constant_variables.iter().copied() {
        if let Some(name) = module.names.get(&variable_id) {
            reflection.names.insert(name.clone());
        }
        reflection.bytes = reflection
            .bytes
            .max(type_size(&module, variable_type_id, None).unwrap_or(0));
    }
    reflection
}

fn parse_module(words: &[u32]) -> Option<Module> {
    if words.len() < 5 || words[0] != 0x0723_0203 {
        return None;
    }

    let mut module = Module::default();
    let mut index = 5;
    while index < words.len() {
        let first_word = words[index];
        let word_count = (first_word >> 16) as usize;
        let opcode = (first_word & 0xffff) as u16;
        if word_count == 0 || index + word_count > words.len() {
            return None;
        }
        let operands = &words[index + 1..index + word_count];
        parse_instruction(&mut module, opcode, operands);
        index += word_count;
    }

    Some(module)
}

fn parse_instruction(module: &mut Module, opcode: u16, operands: &[u32]) {
    match opcode {
        OP_NAME if operands.len() >= 2 => {
            module
                .names
                .insert(operands[0], decode_spirv_string(&operands[1..]));
        }
        OP_MEMBER_DECORATE if operands.len() >= 4 && operands[2] == DECORATION_OFFSET => {
            module
                .member_layouts
                .entry((operands[0], operands[1]))
                .or_default()
                .offset = Some(operands[3]);
        }
        OP_MEMBER_DECORATE if operands.len() >= 4 && operands[2] == DECORATION_MATRIX_STRIDE => {
            module
                .member_layouts
                .entry((operands[0], operands[1]))
                .or_default()
                .matrix_stride = Some(operands[3]);
        }
        OP_TYPE_INT if operands.len() >= 2 => {
            module.types.insert(
                operands[0],
                Type::Scalar {
                    bytes: operands[1] / 8,
                },
            );
        }
        OP_TYPE_FLOAT if operands.len() >= 2 => {
            module.types.insert(
                operands[0],
                Type::Scalar {
                    bytes: operands[1] / 8,
                },
            );
        }
        OP_TYPE_VECTOR if operands.len() >= 3 => {
            module.types.insert(
                operands[0],
                Type::Vector {
                    component_id: operands[1],
                    count: operands[2],
                },
            );
        }
        OP_TYPE_MATRIX if operands.len() >= 3 => {
            module.types.insert(
                operands[0],
                Type::Matrix {
                    column_type_id: operands[1],
                    count: operands[2],
                },
            );
        }
        OP_TYPE_ARRAY if operands.len() >= 3 => {
            module.types.insert(
                operands[0],
                Type::Array {
                    element_id: operands[1],
                    length_id: operands[2],
                },
            );
        }
        OP_TYPE_RUNTIME_ARRAY if operands.len() >= 2 => {
            module.types.insert(
                operands[0],
                Type::RuntimeArray {
                    element_id: operands[1],
                },
            );
        }
        OP_TYPE_STRUCT if !operands.is_empty() => {
            module.types.insert(
                operands[0],
                Type::Struct {
                    member_ids: operands[1..].to_vec(),
                },
            );
        }
        OP_TYPE_POINTER if operands.len() >= 3 => {
            module.types.insert(
                operands[0],
                Type::Pointer {
                    pointee_id: operands[2],
                },
            );
        }
        OP_CONSTANT if operands.len() >= 3 => {
            module.constants.insert(operands[1], operands[2]);
        }
        OP_VARIABLE if operands.len() >= 3 && operands[2] == STORAGE_CLASS_PUSH_CONSTANT => {
            module
                .push_constant_variables
                .push((operands[0], operands[1]));
        }
        _ => {}
    }
}

fn type_size(module: &Module, type_id: u32, member_layout: Option<MemberLayout>) -> Option<u32> {
    match module.types.get(&type_id)? {
        Type::Scalar { bytes } => Some(*bytes),
        Type::Vector {
            component_id,
            count,
        } => Some(type_size(module, *component_id, None)? * *count),
        Type::Matrix {
            column_type_id,
            count,
        } => {
            let column_size = type_size(module, *column_type_id, None)?;
            let stride = member_layout
                .and_then(|layout| layout.matrix_stride)
                .unwrap_or(column_size);
            Some(stride * *count)
        }
        Type::Array {
            element_id,
            length_id,
        } => Some(type_size(module, *element_id, None)? * *module.constants.get(length_id)?),
        Type::RuntimeArray { element_id } => type_size(module, *element_id, None),
        Type::Struct { member_ids } => {
            let mut size = 0;
            for (member_index, member_id) in member_ids.iter().copied().enumerate() {
                let layout = module
                    .member_layouts
                    .get(&(type_id, member_index as u32))
                    .copied()
                    .unwrap_or_default();
                let offset = layout.offset.unwrap_or(0);
                size = size.max(offset + type_size(module, member_id, Some(layout))?);
            }
            Some(size)
        }
        Type::Pointer { pointee_id } => type_size(module, *pointee_id, None),
    }
}

fn decode_spirv_string(words: &[u32]) -> String {
    let mut bytes = Vec::new();
    for word in words {
        for byte in word.to_le_bytes() {
            if byte == 0 {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
            bytes.push(byte);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
