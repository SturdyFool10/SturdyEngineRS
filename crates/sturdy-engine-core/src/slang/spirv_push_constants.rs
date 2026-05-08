// SPIR-V push constant reflection.
//
// Parses raw SPIR-V words to extract:
//   - Total push constant block size in bytes
//   - Names of push constant variables (block-level names)
//   - Per-field reflection: name, kind, byte offset, byte size
//
// The `kind` field classifies the Slang/GLSL types that matter for auto-UI
// generation in `ShaderPlayground` (Track 5).

use std::collections::{BTreeMap, BTreeSet};

const OP_NAME: u16 = 5;
const OP_MEMBER_NAME: u16 = 6;
const OP_MEMBER_DECORATE: u16 = 72;
const OP_TYPE_VOID: u16 = 19;
const OP_TYPE_BOOL: u16 = 20;
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

// ── Public types ──────────────────────────────────────────────────────────────

/// Classification of a push constant field for auto-UI generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PcFieldKind {
    /// `float` — renders as a slider \[min, max\].
    Float,
    /// `float2` / `vec2` — renders as a paired (x, y) input.
    Float2,
    /// `float3` / `vec3` — renders as an XYZ input or colour picker.
    Float3,
    /// `float4` / `vec4` — renders as an XYZW input or colour picker.
    Float4,
    /// `int` / `int32_t` — renders as an integer step input.
    Int,
    /// `uint` / `uint32_t` — renders as an unsigned integer step input.
    Uint,
    /// `bool` (Vulkan: 4-byte uint32 in push constants) — renders as a checkbox.
    Bool,
    /// `float4x4` / `mat4` — large opaque block, shown as raw hex or skipped.
    Mat4,
    /// Any type not covered above (e.g. structs, arrays, double).
    Other { byte_size: u32 },
}

impl PcFieldKind {
    pub fn byte_size(&self) -> u32 {
        match self {
            Self::Float  => 4,  Self::Int  => 4, Self::Uint => 4, Self::Bool => 4,
            Self::Float2 => 8,
            Self::Float3 => 12,
            Self::Float4 => 16,
            Self::Mat4   => 64,
            Self::Other { byte_size } => *byte_size,
        }
    }

    /// Whether the field should be exposed in the playground UI (not padding/matrices).
    pub fn is_editable(&self) -> bool {
        !matches!(self, Self::Mat4 | Self::Other { .. })
    }
}

/// One reflected field inside a push constant block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushConstantField {
    /// Field name as declared in the Slang/GLSL source.
    pub name: String,
    /// Type classification for auto-UI.
    pub kind: PcFieldKind,
    /// Byte offset from the start of the push constant block.
    pub byte_offset: u32,
}

/// Full push constant reflection result.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct PushConstantReflection {
    /// Total size of all push constant blocks in bytes.
    pub bytes: u32,
    /// Names of top-level push constant variables (usually one block struct).
    pub names: BTreeSet<String>,
    /// Per-field detail for all fields in the primary push constant struct.
    pub fields: Vec<PushConstantField>,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Type {
    Void,
    Bool,
    Int  { bytes: u32, signed: bool },
    Float { bytes: u32 },
    Vector { component_id: u32, count: u32 },
    Matrix { column_type_id: u32, count: u32 },
    Array  { element_id: u32, length_id: u32 },
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
    /// member_names[(struct_id, member_index)] = name
    member_names: BTreeMap<(u32, u32), String>,
    types: BTreeMap<u32, Type>,
    constants: BTreeMap<u32, u32>,
    member_layouts: BTreeMap<(u32, u32), MemberLayout>,
    push_constant_variables: Vec<(u32, u32)>,  // (pointee_type_id, variable_id)
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn reflect_spirv_push_constants(words: &[u32]) -> PushConstantReflection {
    let Some(module) = parse_module(words) else {
        return PushConstantReflection::default();
    };

    let mut reflection = PushConstantReflection::default();

    for (variable_type_id, variable_id) in module.push_constant_variables.iter().copied() {
        if let Some(name) = module.names.get(&variable_id) {
            reflection.names.insert(name.clone());
        }
        let block_size = type_size(&module, variable_type_id, None).unwrap_or(0);
        if block_size > reflection.bytes {
            reflection.bytes = block_size;

            // Extract per-field detail from the struct at the top of the block.
            let fields = extract_fields(&module, variable_type_id);
            reflection.fields = fields;
        }
    }
    reflection
}

// ── Field extraction ──────────────────────────────────────────────────────────

fn extract_fields(module: &Module, block_type_id: u32) -> Vec<PushConstantField> {
    // Dereference a pointer to get the struct.
    let struct_id = match module.types.get(&block_type_id) {
        Some(Type::Pointer { pointee_id }) => *pointee_id,
        _ => block_type_id,
    };

    let member_ids = match module.types.get(&struct_id) {
        Some(Type::Struct { member_ids }) => member_ids.clone(),
        _ => return Vec::new(),
    };

    let mut fields = Vec::new();
    for (idx, &member_type_id) in member_ids.iter().enumerate() {
        let idx_u32 = idx as u32;
        let name = module.member_names
            .get(&(struct_id, idx_u32))
            .cloned()
            .unwrap_or_else(|| format!("_field{idx}"));
        let layout = module.member_layouts
            .get(&(struct_id, idx_u32))
            .copied()
            .unwrap_or_default();
        let byte_offset = layout.offset.unwrap_or(0);
        let kind = classify_type(module, member_type_id);
        fields.push(PushConstantField { name, kind, byte_offset });
    }
    fields
}

fn classify_type(module: &Module, type_id: u32) -> PcFieldKind {
    match module.types.get(&type_id) {
        Some(Type::Bool) => PcFieldKind::Bool,
        Some(Type::Int { signed: true,  .. }) => PcFieldKind::Int,
        Some(Type::Int { signed: false, .. }) => PcFieldKind::Uint,
        Some(Type::Float { .. }) => PcFieldKind::Float,
        Some(Type::Vector { component_id, count }) => {
            let comp = *component_id;
            match (module.types.get(&comp), *count) {
                (Some(Type::Float { .. }), 2) => PcFieldKind::Float2,
                (Some(Type::Float { .. }), 3) => PcFieldKind::Float3,
                (Some(Type::Float { .. }), 4) => PcFieldKind::Float4,
                _ => PcFieldKind::Other { byte_size: type_size(module, type_id, None).unwrap_or(0) },
            }
        }
        Some(Type::Matrix { column_type_id, count }) => {
            // float4x4 — 4 columns each of float4
            let ct = *column_type_id;
            if *count == 4 {
                if let Some(Type::Vector { count: 4, .. }) = module.types.get(&ct) {
                    return PcFieldKind::Mat4;
                }
            }
            PcFieldKind::Other { byte_size: type_size(module, type_id, None).unwrap_or(0) }
        }
        _ => PcFieldKind::Other { byte_size: type_size(module, type_id, None).unwrap_or(0) },
    }
}

// ── SPIR-V parsing ────────────────────────────────────────────────────────────

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
            module.names.insert(operands[0], decode_spirv_string(&operands[1..]));
        }
        OP_MEMBER_NAME if operands.len() >= 3 => {
            // operands: [struct_id, member_index, ...string]
            let name = decode_spirv_string(&operands[2..]);
            module.member_names.insert((operands[0], operands[1]), name);
        }
        OP_MEMBER_DECORATE if operands.len() >= 4 && operands[2] == DECORATION_OFFSET => {
            module.member_layouts.entry((operands[0], operands[1])).or_default().offset = Some(operands[3]);
        }
        OP_MEMBER_DECORATE if operands.len() >= 4 && operands[2] == DECORATION_MATRIX_STRIDE => {
            module.member_layouts.entry((operands[0], operands[1])).or_default().matrix_stride = Some(operands[3]);
        }
        OP_TYPE_VOID if !operands.is_empty() => {
            module.types.insert(operands[0], Type::Void);
        }
        OP_TYPE_BOOL if !operands.is_empty() => {
            module.types.insert(operands[0], Type::Bool);
        }
        OP_TYPE_INT if operands.len() >= 3 => {
            // operands: [result_id, width, signedness]
            module.types.insert(operands[0], Type::Int {
                bytes: operands[1] / 8,
                signed: operands[2] != 0,
            });
        }
        OP_TYPE_FLOAT if operands.len() >= 2 => {
            module.types.insert(operands[0], Type::Float { bytes: operands[1] / 8 });
        }
        OP_TYPE_VECTOR if operands.len() >= 3 => {
            module.types.insert(operands[0], Type::Vector { component_id: operands[1], count: operands[2] });
        }
        OP_TYPE_MATRIX if operands.len() >= 3 => {
            module.types.insert(operands[0], Type::Matrix { column_type_id: operands[1], count: operands[2] });
        }
        OP_TYPE_ARRAY if operands.len() >= 3 => {
            module.types.insert(operands[0], Type::Array { element_id: operands[1], length_id: operands[2] });
        }
        OP_TYPE_RUNTIME_ARRAY if operands.len() >= 2 => {
            module.types.insert(operands[0], Type::RuntimeArray { element_id: operands[1] });
        }
        OP_TYPE_STRUCT if !operands.is_empty() => {
            module.types.insert(operands[0], Type::Struct { member_ids: operands[1..].to_vec() });
        }
        OP_TYPE_POINTER if operands.len() >= 3 => {
            module.types.insert(operands[0], Type::Pointer { pointee_id: operands[2] });
        }
        OP_CONSTANT if operands.len() >= 3 => {
            module.constants.insert(operands[1], operands[2]);
        }
        OP_VARIABLE if operands.len() >= 3 && operands[2] == STORAGE_CLASS_PUSH_CONSTANT => {
            module.push_constant_variables.push((operands[0], operands[1]));
        }
        _ => {}
    }
}

fn type_size(module: &Module, type_id: u32, member_layout: Option<MemberLayout>) -> Option<u32> {
    match module.types.get(&type_id)? {
        Type::Void | Type::Bool => Some(4),
        Type::Int   { bytes, .. } => Some(*bytes),
        Type::Float { bytes }     => Some(*bytes),
        Type::Vector { component_id, count } => {
            Some(type_size(module, *component_id, None)? * *count)
        }
        Type::Matrix { column_type_id, count } => {
            let col = type_size(module, *column_type_id, None)?;
            let stride = member_layout.and_then(|l| l.matrix_stride).unwrap_or(col);
            Some(stride * *count)
        }
        Type::Array { element_id, length_id } => {
            Some(type_size(module, *element_id, None)? * *module.constants.get(length_id)?)
        }
        Type::RuntimeArray { element_id } => type_size(module, *element_id, None),
        Type::Struct { member_ids } => {
            let mut size = 0;
            for (i, &mid) in member_ids.iter().enumerate() {
                let layout = module.member_layouts.get(&(type_id, i as u32)).copied().unwrap_or_default();
                let offset = layout.offset.unwrap_or(0);
                size = size.max(offset + type_size(module, mid, Some(layout))?);
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
            if byte == 0 { return String::from_utf8_lossy(&bytes).into_owned(); }
            bytes.push(byte);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
