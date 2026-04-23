use std::hash::{Hash, Hasher};

use crate::{ShaderRef, UiColor, font_discovery::FontDiscovery};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextWrap {
    Words,
    Newlines,
    None,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FontFeatures {
    pub standard_ligatures: bool,
    pub contextual_alternates: bool,
    pub discretionary_ligatures: bool,
    pub historical_ligatures: bool,
    pub custom: Vec<textui::TextFeatureSetting>,
}

impl Default for FontFeatures {
    fn default() -> Self {
        Self {
            standard_ligatures: true,
            contextual_alternates: true,
            discretionary_ligatures: false,
            historical_ligatures: false,
            custom: Vec::new(),
        }
    }
}

impl FontFeatures {
    pub fn enable_tag(&mut self, tag: &str) -> Result<(), InvalidOpenTypeTag> {
        self.set_tag(tag, 1)
    }

    pub fn enable_tags<I, S>(&mut self, tags: I) -> Result<(), InvalidOpenTypeTag>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for tag in tags {
            self.enable_tag(tag.as_ref())?;
        }
        Ok(())
    }

    pub fn enable_tags_csv(&mut self, tags: &str) -> Result<(), InvalidOpenTypeTag> {
        self.enable_tags(tags.split(',').map(str::trim).filter(|tag| !tag.is_empty()))
    }

    pub fn disable_tag(&mut self, tag: &str) -> Result<(), InvalidOpenTypeTag> {
        self.set_tag(tag, 0)
    }

    pub fn set_tag(&mut self, tag: &str, value: u16) -> Result<(), InvalidOpenTypeTag> {
        let bytes = parse_opentype_tag(tag)?;
        if let Some(existing) = self.custom.iter_mut().find(|feature| feature.tag == bytes) {
            existing.value = value;
        } else {
            self.custom
                .push(textui::TextFeatureSetting::new(bytes, value));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InvalidOpenTypeTag;

impl std::fmt::Display for InvalidOpenTypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpenType feature tag must be exactly 4 ASCII bytes")
    }
}

impl std::error::Error for InvalidOpenTypeTag {}

fn parse_opentype_tag(tag: &str) -> Result<[u8; 4], InvalidOpenTypeTag> {
    let bytes = tag.as_bytes();
    if bytes.len() != 4 || !bytes.iter().all(|byte| byte.is_ascii()) {
        return Err(InvalidOpenTypeTag);
    }
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

impl From<&FontFeatures> for textui::TextFundamentals {
    fn from(value: &FontFeatures) -> Self {
        Self {
            kerning: textui::TextKerning::Auto,
            standard_ligatures: value.standard_ligatures,
            contextual_alternates: value.contextual_alternates,
            discretionary_ligatures: value.discretionary_ligatures,
            historical_ligatures: value.historical_ligatures,
            feature_settings: value.custom.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextOutline {
    pub width_px: f32,
    pub color: UiColor,
    pub shader: ShaderRef,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font_size: f32,
    pub line_height: f32,
    pub color: UiColor,
    pub fill_shader: ShaderRef,
    pub outline: Option<TextOutline>,
    pub wrap: TextWrap,
    pub align: TextAlign,
    pub features: FontFeatures,
    pub family_candidates: Vec<String>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 18.0,
            line_height: 27.0,
            color: UiColor::WHITE,
            fill_shader: ShaderRef::SOLID_COLOR,
            outline: None,
            wrap: TextWrap::Words,
            align: TextAlign::Left,
            features: FontFeatures::default(),
            family_candidates: Vec::new(),
        }
    }
}

impl TextStyle {
    pub fn to_textui_options(&self) -> textui::TextLabelOptions {
        self.to_textui_options_with_families(self.family_candidates.clone())
    }

    pub fn to_textui_options_with_families(
        &self,
        family_candidates: Vec<String>,
    ) -> textui::TextLabelOptions {
        let rgba = self.color.to_f32_array();
        textui::TextLabelOptions {
            font_size: self.font_size,
            line_height: self.line_height,
            color: textui::TextColor::from_rgba8(
                (rgba[0].clamp(0.0, 1.0) * 255.0) as u8,
                (rgba[1].clamp(0.0, 1.0) * 255.0) as u8,
                (rgba[2].clamp(0.0, 1.0) * 255.0) as u8,
                (rgba[3].clamp(0.0, 1.0) * 255.0) as u8,
            ),
            wrap: self.wrap != TextWrap::None,
            monospace: false,
            weight: 400,
            italic: false,
            family_candidates,
            fundamentals: (&self.features).into(),
        }
    }

    pub fn resolved_with_fonts(&self, discovery: &FontDiscovery) -> Self {
        let mut resolved = self.clone();
        resolved.family_candidates = discovery.resolve_family_candidates(&self.family_candidates);
        resolved
    }

    pub fn cache_fingerprint(&self, text: &str, width_points_opt: Option<f32>, scale: f32) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        self.font_size.to_bits().hash(&mut hasher);
        self.line_height.to_bits().hash(&mut hasher);
        self.color.color.r.to_bits().hash(&mut hasher);
        self.color.color.g.to_bits().hash(&mut hasher);
        self.color.color.b.to_bits().hash(&mut hasher);
        self.color.color.a.to_bits().hash(&mut hasher);
        self.wrap.hash(&mut hasher);
        self.align.hash(&mut hasher);
        self.features.hash(&mut hasher);
        self.family_candidates.hash(&mut hasher);
        width_points_opt.map(f32::to_bits).hash(&mut hasher);
        scale.to_bits().hash(&mut hasher);
        hasher.finish()
    }
}
