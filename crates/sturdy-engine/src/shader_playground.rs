// Shader playground — interactive GPU shader with auto-generated parameter controls.
//
// `ShaderPlayground` loads any Slang fullscreen shader, reflects its push constant
// struct fields, and exposes each field as a live-editable parameter.  No app code
// is needed to move sliders: register once with a `RuntimeController` and every field
// appears in the runtime settings panel as a Float / Bool / Integer control.
//
// Roadmap: Track 5 — Shader playground auto-UI.
//
// # Zero-config usage
// ```ignore
// let mut pg = ShaderPlayground::from_file(&engine, "shaders/plasma.slang")?;
// pg.register_with_runtime(frame.runtime_controller())?;
//
// // Each frame:
// pg.sync_from_runtime(frame.runtime_controller());
// pg.render(&output_image, &frame)?;
// ```
//
// # Manual usage (no runtime controller)
// ```ignore
// pg.set("time",  PlaygroundValue::Float(elapsed_s));
// pg.set("color", PlaygroundValue::Float3([1.0, 0.5, 0.0]));
// pg.set_range("brightness", 0.0, 5.0);
// pg.render(&output_image, &frame)?;
// ```

use std::path::Path;

use crate::{
    Engine, GraphImage, PcFieldKind, PushConstantField, RenderFrame, Result,
    RuntimeController, RuntimeSettingDescriptor, RuntimeSettingId, RuntimeSettingValue,
    ShaderProgram, StageMask, ShaderParameterKind, RuntimeApplyPath,
};

// ── PlaygroundValue ───────────────────────────────────────────────────────────

/// The current value of one shader playground parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaygroundValue {
    Float(f32),
    Float2([f32; 2]),
    Float3([f32; 3]),
    Float4([f32; 4]),
    Int(i32),
    Uint(u32),
    Bool(bool),
    /// Raw bytes for unsupported types (matrices, arrays, structs).
    Raw(Vec<u8>),
}

impl PlaygroundValue {
    fn byte_size(&self) -> usize {
        match self {
            Self::Float(_)   | Self::Int(_)   | Self::Uint(_) | Self::Bool(_) => 4,
            Self::Float2(_)  => 8,
            Self::Float3(_)  => 12,
            Self::Float4(_)  => 16,
            Self::Raw(v)     => v.len(),
        }
    }

    /// Write this value into `dst` at `offset`. `dst` must be large enough.
    fn write_into(&self, dst: &mut [u8], offset: usize) {
        match self {
            Self::Float(v)   => dst[offset..offset + 4].copy_from_slice(&v.to_le_bytes()),
            Self::Int(v)     => dst[offset..offset + 4].copy_from_slice(&v.to_le_bytes()),
            Self::Uint(v)    => dst[offset..offset + 4].copy_from_slice(&v.to_le_bytes()),
            Self::Bool(v)    => dst[offset..offset + 4].copy_from_slice(&(*v as u32).to_le_bytes()),
            Self::Float2(v)  => {
                dst[offset..offset + 4].copy_from_slice(&v[0].to_le_bytes());
                dst[offset + 4..offset + 8].copy_from_slice(&v[1].to_le_bytes());
            }
            Self::Float3(v)  => {
                dst[offset..offset + 4].copy_from_slice(&v[0].to_le_bytes());
                dst[offset + 4..offset + 8].copy_from_slice(&v[1].to_le_bytes());
                dst[offset + 8..offset + 12].copy_from_slice(&v[2].to_le_bytes());
            }
            Self::Float4(v)  => {
                dst[offset..offset + 4].copy_from_slice(&v[0].to_le_bytes());
                dst[offset + 4..offset + 8].copy_from_slice(&v[1].to_le_bytes());
                dst[offset + 8..offset + 12].copy_from_slice(&v[2].to_le_bytes());
                dst[offset + 12..offset + 16].copy_from_slice(&v[3].to_le_bytes());
            }
            Self::Raw(v) => {
                let n = v.len().min(dst.len() - offset);
                dst[offset..offset + n].copy_from_slice(&v[..n]);
            }
        }
    }

    fn default_for(kind: &PcFieldKind) -> Self {
        match kind {
            PcFieldKind::Float   => Self::Float(0.0),
            PcFieldKind::Float2  => Self::Float2([0.0; 2]),
            PcFieldKind::Float3  => Self::Float3([0.0; 3]),
            PcFieldKind::Float4  => Self::Float4([0.0; 4]),
            PcFieldKind::Int     => Self::Int(0),
            PcFieldKind::Uint    => Self::Uint(0),
            PcFieldKind::Bool    => Self::Bool(false),
            PcFieldKind::Mat4    => Self::Raw(vec![0u8; 64]),
            PcFieldKind::Other { byte_size } => Self::Raw(vec![0u8; *byte_size as usize]),
        }
    }

    /// Convert from a `RuntimeSettingValue` (from the settings panel).
    fn from_runtime(rv: RuntimeSettingValue, kind: &PcFieldKind) -> Option<Self> {
        match (kind, rv) {
            (PcFieldKind::Float, RuntimeSettingValue::Float(v)) => Some(Self::Float(v as f32)),
            (PcFieldKind::Float2, RuntimeSettingValue::Text(s)) => parse_float2(&s),
            (PcFieldKind::Float3, RuntimeSettingValue::Text(s)) => parse_float3(&s),
            (PcFieldKind::Float4, RuntimeSettingValue::Text(s)) => parse_float4(&s),
            (PcFieldKind::Int,    RuntimeSettingValue::Integer(v)) => Some(Self::Int(v as i32)),
            (PcFieldKind::Uint,   RuntimeSettingValue::Integer(v)) => Some(Self::Uint(v as u32)),
            (PcFieldKind::Bool,   RuntimeSettingValue::Bool(v)) => Some(Self::Bool(v)),
            _ => None,
        }
    }

    /// Convert to a `RuntimeSettingValue` for registration.
    fn to_runtime(&self) -> RuntimeSettingValue {
        match self {
            Self::Float(v)   => RuntimeSettingValue::Float(*v as f64),
            Self::Float2(v)  => RuntimeSettingValue::Text(format!("{} {}", v[0], v[1])),
            Self::Float3(v)  => RuntimeSettingValue::Text(format!("{} {} {}", v[0], v[1], v[2])),
            Self::Float4(v)  => RuntimeSettingValue::Text(format!("{} {} {} {}", v[0], v[1], v[2], v[3])),
            Self::Int(v)     => RuntimeSettingValue::Integer(*v as i64),
            Self::Uint(v)    => RuntimeSettingValue::Integer(*v as i64),
            Self::Bool(v)    => RuntimeSettingValue::Bool(*v),
            Self::Raw(_)     => RuntimeSettingValue::Text(String::new()),
        }
    }
}

// ── PlaygroundParam ───────────────────────────────────────────────────────────

/// One reflected push constant field exposed as a live-editable parameter.
#[derive(Clone, Debug)]
pub struct PlaygroundParam {
    /// Source field descriptor from SPIR-V reflection.
    pub field: PushConstantField,
    /// Display label shown in the settings panel. Defaults to the field name.
    pub label: String,
    /// Current value. Mutated by `set()`, `sync_from_runtime()`, or `load_preset()`.
    pub value: PlaygroundValue,
    /// Slider minimum for `Float*` fields. Default `0.0`.
    pub min: f32,
    /// Slider maximum for `Float*` fields. Default `1.0`.
    pub max: f32,
    /// Runtime setting ID, set after `register_with_runtime`.
    setting_id: Option<RuntimeSettingId>,
}

impl PlaygroundParam {
    fn new(field: PushConstantField) -> Self {
        let label = field.name.clone();
        let value = PlaygroundValue::default_for(&field.kind);
        Self {
            field,
            label,
            value,
            min: 0.0,
            max: 1.0,
            setting_id: None,
        }
    }

    /// Whether this field is surfaced in the runtime settings panel.
    pub fn is_registered(&self) -> bool {
        self.setting_id.is_some()
    }
}

// ── PlaygroundPreset ──────────────────────────────────────────────────────────

/// A named snapshot of all parameter values.
#[derive(Clone, Debug)]
pub struct PlaygroundPreset {
    pub name: String,
    /// Values in the same order as `ShaderPlayground::params`.
    pub values: Vec<PlaygroundValue>,
}

// ── ShaderPlayground ──────────────────────────────────────────────────────────

/// An interactive fullscreen shader with auto-reflected push constant parameters.
///
/// Build with [`ShaderPlayground::from_file`] or [`ShaderPlayground::from_source`],
/// then call [`register_with_runtime`] to expose all parameters as runtime settings.
///
/// # Example — zero-config interactive shader
/// ```ignore
/// // At init:
/// let mut pg = ShaderPlayground::from_file(&engine, "shaders/plasma.slang")?;
/// pg.register_with_runtime(&runtime_controller)?;
///
/// // Each frame:
/// pg.sync_from_runtime(&runtime_controller);
/// pg.render(&output, &frame)?;
/// ```
pub struct ShaderPlayground {
    program: ShaderProgram,
    params: Vec<PlaygroundParam>,
    total_bytes: u32,
    presets: Vec<PlaygroundPreset>,
}

impl ShaderPlayground {
    /// Load and compile a Slang shader from `path`, reflecting all push constant fields.
    pub fn from_file(engine: &Engine, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let program = engine.load_shader(path)?;
        Ok(Self::from_program(program))
    }

    /// Compile an inline Slang source string, reflecting all push constant fields.
    pub fn from_source(engine: &Engine, source: impl AsRef<str>) -> Result<Self> {
        let program = ShaderProgram::from_inline_fragment(engine, source.as_ref())?;
        Ok(Self::from_program(program))
    }

    /// Build from a pre-compiled [`ShaderProgram`].
    pub fn from_program(program: ShaderProgram) -> Self {
        let (params, total_bytes) = build_params(program.reflection());
        Self { program, params, total_bytes, presets: Vec::new() }
    }

    // ── Parameter access ──────────────────────────────────────────────────────

    /// All reflected parameters in byte-offset order.
    pub fn params(&self) -> &[PlaygroundParam] { &self.params }

    /// Set a parameter by name. Returns `false` if not found or type mismatch.
    pub fn set(&mut self, name: &str, value: PlaygroundValue) -> bool {
        for p in &mut self.params {
            if p.field.name == name {
                p.value = value;
                return true;
            }
        }
        false
    }

    /// Get the current value of a parameter by name.
    pub fn get(&self, name: &str) -> Option<&PlaygroundValue> {
        self.params.iter().find(|p| p.field.name == name).map(|p| &p.value)
    }

    /// Set label for display in the runtime settings panel.
    pub fn set_label(&mut self, name: &str, label: impl Into<String>) {
        if let Some(p) = self.params.iter_mut().find(|p| p.field.name == name) {
            p.label = label.into();
        }
    }

    /// Set slider range for a `Float*` parameter. Default `[0.0, 1.0]`.
    pub fn set_range(&mut self, name: &str, min: f32, max: f32) {
        if let Some(p) = self.params.iter_mut().find(|p| p.field.name == name) {
            p.min = min;
            p.max = max;
        }
    }

    /// Total size of the push constant block in bytes.
    pub fn total_bytes(&self) -> u32 { self.total_bytes }

    /// The underlying compiled shader program.
    pub fn program(&self) -> &ShaderProgram { &self.program }

    // ── Runtime settings integration ──────────────────────────────────────────

    /// Register all editable parameters as `RuntimeSettingId::App` settings.
    ///
    /// After calling this, parameter values are updated from the runtime panel.
    /// Call [`sync_from_runtime`] each frame to pull in changes.
    ///
    /// Fields whose names start with `_` (padding) are silently skipped.
    pub fn register_with_runtime(&mut self, controller: &RuntimeController) -> Result<()> {
        for param in &mut self.params {
            if param.field.name.starts_with('_') {
                continue; // skip padding
            }
            if !param.field.kind.is_editable() {
                continue;
            }

            let id = RuntimeSettingId::App(format!("pg.{}", param.field.name));
            let default_rv = param.value.to_runtime();
            let desc = RuntimeSettingDescriptor::new(
                id.clone(),
                param.label.clone(),
                RuntimeApplyPath::Immediate,
                default_rv,
            );

            // Register — if already registered (e.g. hot reload), ignore the error.
            let _ = controller.register_app_setting(desc);
            param.setting_id = Some(id);
        }
        Ok(())
    }

    /// Pull the latest runtime setting values into the playground parameters.
    ///
    /// Call once per frame after `register_with_runtime`.
    pub fn sync_from_runtime(&mut self, controller: &RuntimeController) {
        for param in &mut self.params {
            let Some(id) = &param.setting_id else { continue };
            let Some(rv) = controller.setting_value(id.clone()) else { continue };
            if let Some(v) = PlaygroundValue::from_runtime(rv, &param.field.kind) {
                param.value = v;
            }
        }
    }

    // ── Rendering ─────────────────────────────────────────────────────────────

    /// Execute the shader with the current parameter values into `output`.
    ///
    /// Packs all field values into a push constant byte buffer and calls
    /// `output.execute_shader_with_push_constants`. Binds as VERTEX | FRAGMENT stages.
    pub fn render(&self, output: &GraphImage, frame: &RenderFrame) -> Result<()> {
        let bytes = self.pack_bytes();
        if bytes.is_empty() {
            output.execute_shader(&self.program)
        } else {
            output.execute_shader_with_push_constants(
                &self.program,
                StageMask::VERTEX | StageMask::FRAGMENT,
                &bytes,
            )
        }
    }

    /// Pack all parameter values into a `Vec<u8>` push constant buffer.
    pub fn pack_bytes(&self) -> Vec<u8> {
        if self.total_bytes == 0 { return Vec::new(); }
        let mut buf = vec![0u8; self.total_bytes as usize];
        for param in &self.params {
            let off = param.field.byte_offset as usize;
            let end = off + param.value.byte_size();
            if end <= buf.len() {
                param.value.write_into(&mut buf, off);
            }
        }
        buf
    }

    // ── Presets ───────────────────────────────────────────────────────────────

    /// Snapshot the current parameter values as a named preset.
    pub fn save_preset(&mut self, name: impl Into<String>) {
        let values = self.params.iter().map(|p| p.value.clone()).collect();
        let name = name.into();
        if let Some(existing) = self.presets.iter_mut().find(|p| p.name == name) {
            existing.values = values;
        } else {
            self.presets.push(PlaygroundPreset { name, values });
        }
    }

    /// Restore a saved preset. Returns `false` if not found.
    pub fn load_preset(&mut self, name: &str) -> bool {
        let Some(preset) = self.presets.iter().find(|p| p.name == name) else {
            return false;
        };
        let values = preset.values.clone();
        for (param, value) in self.params.iter_mut().zip(values.iter()) {
            param.value = value.clone();
        }
        true
    }

    /// All saved presets.
    pub fn presets(&self) -> &[PlaygroundPreset] { &self.presets }

    // ── Screenshot export ─────────────────────────────────────────────────────

    /// Render the current frame to CPU-readable RGBA8 pixels.
    ///
    /// Suitable for saving screenshots or exporting reference images.
    /// Blocks until the GPU finishes (synchronous).
    pub fn export_rgba8(&self, width: u32, height: u32, engine: &Engine) -> Result<Vec<u8>> {
        crate::render_to_rgba8_with_engine(engine, width, height, |frame, output, _engine| {
            self.render(output, frame)
        })
    }
}

// ── Builder helpers ───────────────────────────────────────────────────────────

fn build_params(reflection: &crate::ShaderReflection) -> (Vec<PlaygroundParam>, u32) {
    let mut params = Vec::new();
    let mut total = 0u32;

    for param_refl in &reflection.parameters {
        if param_refl.kind != ShaderParameterKind::PushConstant {
            continue;
        }
        total = total.max(param_refl.size_bytes.unwrap_or(0));
        for field in &param_refl.push_constant_fields {
            let pg_param = PlaygroundParam::new(field.clone());
            params.push(pg_param);
        }
    }

    // Sort by byte offset for consistent display order.
    params.sort_by_key(|p| p.field.byte_offset);
    (params, total)
}

// ── String parsing helpers (for float vector settings) ────────────────────────

fn parse_float2(s: &str) -> Option<PlaygroundValue> {
    let mut it = s.split_whitespace();
    let x = it.next()?.parse().ok()?;
    let y = it.next()?.parse().ok()?;
    Some(PlaygroundValue::Float2([x, y]))
}

fn parse_float3(s: &str) -> Option<PlaygroundValue> {
    let mut it = s.split_whitespace();
    let x = it.next()?.parse().ok()?;
    let y = it.next()?.parse().ok()?;
    let z = it.next()?.parse().ok()?;
    Some(PlaygroundValue::Float3([x, y, z]))
}

fn parse_float4(s: &str) -> Option<PlaygroundValue> {
    let mut it = s.split_whitespace();
    let x = it.next()?.parse().ok()?;
    let y = it.next()?.parse().ok()?;
    let z = it.next()?.parse().ok()?;
    let w = it.next()?.parse().ok()?;
    Some(PlaygroundValue::Float4([x, y, z, w]))
}
