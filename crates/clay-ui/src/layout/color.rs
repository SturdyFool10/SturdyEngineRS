use colorlab::colorspaces::colorspace::ColorSpace;
use colorlab::{
    AdobeRgb, Color, DisplayP3, Hsl, Hsv, Hwb, Lab, Lch, Luv, Oklab, Oklch, Rec2020, Srgb, Xyz,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ColorSpaceKind {
    LinearSrgb,
    Srgb,
    DisplayP3,
    AdobeRgb,
    Rec2020,
    Xyz,
    Lab,
    Lch,
    Luv,
    Oklab,
    Oklch,
    Hsl,
    Hsv,
    Hwb,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorSpaceSample {
    pub space: ColorSpaceKind,
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ColorSpaceSample {
    pub const fn new(space: ColorSpaceKind, r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { space, r, g, b, a }
    }

    pub fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            space: self.space,
            r: lerp(self.r, other.r, t),
            g: lerp(self.g, other.g, t),
            b: lerp(self.b, other.b, t),
            a: lerp(self.a, other.a, t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiColor {
    pub color: Color,
    pub source_space: ColorSpaceKind,
    pub transform_space: ColorSpaceKind,
}

impl UiColor {
    pub const TRANSPARENT: Self = Self {
        color: Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        },
        source_space: ColorSpaceKind::LinearSrgb,
        transform_space: ColorSpaceKind::LinearSrgb,
    };

    pub const WHITE: Self = Self {
        color: Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        source_space: ColorSpaceKind::LinearSrgb,
        transform_space: ColorSpaceKind::LinearSrgb,
    };

    pub fn linear_rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self {
            color: Color::new(r, g, b, a),
            source_space: ColorSpaceKind::LinearSrgb,
            transform_space: ColorSpaceKind::LinearSrgb,
        }
    }

    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::from_space(
            Srgb {
                r: f64::from(r) / 255.0,
                g: f64::from(g) / 255.0,
                b: f64::from(b) / 255.0,
                a: f64::from(a) / 255.0,
            },
            ColorSpaceKind::Srgb,
        )
    }

    pub fn from_space<T: ColorSpace>(space_color: T, source_space: ColorSpaceKind) -> Self {
        Self {
            color: space_color.to_color(),
            source_space,
            transform_space: ColorSpaceKind::LinearSrgb,
        }
    }

    pub fn from_srgb(color: Srgb) -> Self {
        Self::from_space(color, ColorSpaceKind::Srgb)
    }

    pub fn from_display_p3(color: DisplayP3) -> Self {
        Self::from_space(color, ColorSpaceKind::DisplayP3)
    }

    pub fn from_adobe_rgb(color: AdobeRgb) -> Self {
        Self::from_space(color, ColorSpaceKind::AdobeRgb)
    }

    pub fn from_rec2020(color: Rec2020) -> Self {
        Self::from_space(color, ColorSpaceKind::Rec2020)
    }

    pub fn from_xyz(color: Xyz) -> Self {
        Self::from_space(color, ColorSpaceKind::Xyz)
    }

    pub fn from_lab(color: Lab) -> Self {
        Self::from_space(color, ColorSpaceKind::Lab)
    }

    pub fn from_lch(color: Lch) -> Self {
        Self::from_space(color, ColorSpaceKind::Lch)
    }

    pub fn from_luv(color: Luv) -> Self {
        Self::from_space(color, ColorSpaceKind::Luv)
    }

    pub fn from_oklab(color: Oklab) -> Self {
        Self::from_space(color, ColorSpaceKind::Oklab)
    }

    pub fn from_oklch(color: Oklch) -> Self {
        Self::from_space(color, ColorSpaceKind::Oklch)
    }

    pub fn from_hsl(color: Hsl) -> Self {
        Self::from_space(color, ColorSpaceKind::Hsl)
    }

    pub fn from_hsv(color: Hsv) -> Self {
        Self::from_space(color, ColorSpaceKind::Hsv)
    }

    pub fn from_hwb(color: Hwb) -> Self {
        Self::from_space(color, ColorSpaceKind::Hwb)
    }

    pub fn with_source_space(mut self, source_space: ColorSpaceKind) -> Self {
        self.source_space = source_space;
        self
    }

    pub fn with_transform_space(mut self, transform_space: ColorSpaceKind) -> Self {
        self.transform_space = transform_space;
        self
    }

    pub fn sample_in_space(self, space: ColorSpaceKind) -> ColorSpaceSample {
        space.sample_color(self.color)
    }

    pub fn transformed_in_space(
        self,
        space: ColorSpaceKind,
        transform: impl FnOnce(ColorSpaceSample) -> ColorSpaceSample,
    ) -> Self {
        let transformed = transform(self.sample_in_space(space));
        Self {
            color: space.compose_sample(transformed),
            source_space: self.source_space,
            transform_space: space,
        }
    }

    pub fn mix_in_space(self, other: Self, t: f64, space: ColorSpaceKind) -> Self {
        let mixed = space
            .sample_color(self.color)
            .lerp(space.sample_color(other.color), t);
        Self {
            color: space.compose_sample(mixed),
            source_space: self.source_space,
            transform_space: space,
        }
    }

    pub fn is_visible(self) -> bool {
        self.color.a > 0.0
    }

    pub fn to_f32_array(self) -> [f32; 4] {
        [
            self.color.r as f32,
            self.color.g as f32,
            self.color.b as f32,
            self.color.a as f32,
        ]
    }

    pub fn with_alpha(self, alpha: f64) -> Self {
        Self {
            color: Color::new(self.color.r, self.color.g, self.color.b, alpha),
            source_space: self.source_space,
            transform_space: self.transform_space,
        }
    }

    pub fn premultiply(self) -> Self {
        Self::linear_rgba(
            self.color.r * self.color.a,
            self.color.g * self.color.a,
            self.color.b * self.color.a,
            self.color.a,
        )
        .with_source_space(self.source_space)
        .with_transform_space(self.transform_space)
    }
}

impl ColorSpaceKind {
    pub fn sample_color(self, color: Color) -> ColorSpaceSample {
        match self {
            Self::LinearSrgb => ColorSpaceSample::new(self, color.r, color.g, color.b, color.a),
            Self::Srgb => {
                let value = Srgb::from_color(&color);
                ColorSpaceSample::new(self, value.r, value.g, value.b, value.a)
            }
            Self::DisplayP3 => {
                let value = DisplayP3::from_color(&color);
                ColorSpaceSample::new(self, value.r, value.g, value.b, value.a)
            }
            Self::AdobeRgb => {
                let value = AdobeRgb::from_color(&color);
                ColorSpaceSample::new(self, value.r, value.g, value.b, value.a)
            }
            Self::Rec2020 => {
                let value = Rec2020::from_color(&color);
                ColorSpaceSample::new(self, value.r, value.g, value.b, value.a)
            }
            Self::Xyz => {
                let value = Xyz::from_color(&color);
                ColorSpaceSample::new(self, value.x, value.y, value.z, value.alpha)
            }
            Self::Lab => {
                let value = Lab::from_color(&color);
                ColorSpaceSample::new(self, value.l, value.a, value.b, value.alpha)
            }
            Self::Lch => {
                let value = Lch::from_color(&color);
                ColorSpaceSample::new(self, value.l, value.c, value.h, value.a)
            }
            Self::Luv => {
                let value = Luv::from_color(&color);
                ColorSpaceSample::new(self, value.l, value.u, value.v, value.alpha)
            }
            Self::Oklab => {
                let value = Oklab::from_color(&color);
                ColorSpaceSample::new(self, value.l, value.a, value.b, value.alpha)
            }
            Self::Oklch => {
                let value = Oklch::from_color(&color);
                ColorSpaceSample::new(self, value.l, value.c, value.h, value.alpha)
            }
            Self::Hsl => {
                let value = Hsl::from_color(&color);
                ColorSpaceSample::new(self, value.h, value.s, value.l, value.a)
            }
            Self::Hsv => {
                let value = Hsv::from_color(&color);
                ColorSpaceSample::new(self, value.h, value.s, value.v, value.a)
            }
            Self::Hwb => {
                let value = Hwb::from_color(&color);
                ColorSpaceSample::new(self, value.h, value.w, value.b, value.a)
            }
        }
    }

    pub fn compose_sample(self, sample: ColorSpaceSample) -> Color {
        debug_assert_eq!(self, sample.space);
        match self {
            Self::LinearSrgb => Color::new(sample.r, sample.g, sample.b, sample.a),
            Self::Srgb => Srgb {
                r: sample.r,
                g: sample.g,
                b: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::DisplayP3 => DisplayP3 {
                r: sample.r,
                g: sample.g,
                b: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::AdobeRgb => AdobeRgb {
                r: sample.r,
                g: sample.g,
                b: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::Rec2020 => Rec2020 {
                r: sample.r,
                g: sample.g,
                b: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::Xyz => Xyz {
                x: sample.r,
                y: sample.g,
                z: sample.b,
                alpha: sample.a,
            }
            .to_color(),
            Self::Lab => Lab {
                l: sample.r,
                a: sample.g,
                b: sample.b,
                alpha: sample.a,
            }
            .to_color(),
            Self::Lch => Lch {
                l: sample.r,
                c: sample.g,
                h: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::Luv => Luv {
                l: sample.r,
                u: sample.g,
                v: sample.b,
                alpha: sample.a,
            }
            .to_color(),
            Self::Oklab => Oklab {
                l: sample.r,
                a: sample.g,
                b: sample.b,
                alpha: sample.a,
            }
            .to_color(),
            Self::Oklch => Oklch {
                l: sample.r,
                c: sample.g,
                h: sample.b,
                alpha: sample.a,
            }
            .to_color(),
            Self::Hsl => Hsl {
                h: sample.r,
                s: sample.g,
                l: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::Hsv => Hsv {
                h: sample.r,
                s: sample.g,
                v: sample.b,
                a: sample.a,
            }
            .to_color(),
            Self::Hwb => Hwb {
                h: sample.r,
                w: sample.g,
                b: sample.b,
                a: sample.a,
            }
            .to_color(),
        }
    }

    pub fn blend(self, left: Color, right: Color, t: f64) -> Color {
        self.compose_sample(self.sample_color(left).lerp(self.sample_color(right), t))
    }
}

impl Default for UiColor {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuColorTransform {
    None,
    PremultiplyAlpha,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorComputationMode {
    Cpu,
    Gpu,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorWorkload {
    /// Single output color math for an element/property.
    SingleColorTransform,
    /// Per-pixel gradient evaluation.
    GradientPerPixel,
    /// Any effect that evaluates multiple colors per pixel.
    MultiColorPerPixel,
}

pub fn color_computation_mode(workload: ColorWorkload) -> ColorComputationMode {
    match workload {
        ColorWorkload::SingleColorTransform => ColorComputationMode::Cpu,
        ColorWorkload::GradientPerPixel | ColorWorkload::MultiColorPerPixel => {
            ColorComputationMode::Gpu
        }
    }
}

impl CpuColorTransform {
    pub fn apply(self, color: UiColor) -> UiColor {
        match self {
            Self::None => color,
            Self::PremultiplyAlpha => color.premultiply(),
        }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}
