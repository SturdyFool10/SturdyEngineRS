use glam::Vec2;

use crate::{ColorSpaceKind, UiColor, shader::ShaderRef};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    SmoothStep,
    Custom(u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub offset: f32,
    pub color: UiColor,
    pub easing_to_next: Easing,
}

impl ColorStop {
    pub fn new(offset: f32, color: UiColor) -> Self {
        Self {
            offset: offset.clamp(0.0, 1.0),
            color,
            easing_to_next: Easing::Linear,
        }
    }

    pub fn with_easing(mut self, easing: Easing) -> Self {
        self.easing_to_next = easing;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear { start: Vec2, end: Vec2 },
    Radial { center: Vec2, radius: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<ColorStop>,
    pub shader: ShaderRef,
    pub interpolation_space: ColorSpaceKind,
}

impl Gradient {
    pub fn linear(start: Vec2, end: Vec2, shader: ShaderRef) -> Self {
        Self {
            kind: GradientKind::Linear { start, end },
            stops: Vec::new(),
            shader,
            interpolation_space: ColorSpaceKind::LinearSrgb,
        }
    }

    pub fn radial(center: Vec2, radius: f32, shader: ShaderRef) -> Self {
        Self {
            kind: GradientKind::Radial { center, radius },
            stops: Vec::new(),
            shader,
            interpolation_space: ColorSpaceKind::LinearSrgb,
        }
    }

    pub fn with_stop(mut self, stop: ColorStop) -> Self {
        self.stops.push(stop);
        self.sort_and_dedup();
        self
    }

    pub fn sort_and_dedup(&mut self) {
        self.stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        self.stops
            .dedup_by(|a, b| (a.offset - b.offset).abs() <= f32::EPSILON);
    }

    pub fn with_interpolation_space(mut self, space: ColorSpaceKind) -> Self {
        self.interpolation_space = space;
        self
    }

    pub fn requires_gpu(&self) -> bool {
        true
    }

    pub fn sample_at(&self, t: f32) -> Option<UiColor> {
        let t = t.clamp(0.0, 1.0);
        let first = self.stops.first()?;
        if self.stops.len() == 1 {
            return Some(first.color);
        }

        let mut left = first;
        let mut right = first;
        for stop in &self.stops {
            if stop.offset <= t {
                left = stop;
            }
            if stop.offset >= t {
                right = stop;
                break;
            }
        }

        if (right.offset - left.offset).abs() <= f32::EPSILON {
            return Some(left.color);
        }

        let span = (t - left.offset) / (right.offset - left.offset);
        let eased = EasingRegistry::default().evaluate(left.easing_to_next, span);
        Some(
            left.color
                .mix_in_space(right.color, eased as f64, self.interpolation_space),
        )
    }

    pub fn sample_with_easing(&self, t: f32, easing_registry: &EasingRegistry) -> Option<UiColor> {
        let t = t.clamp(0.0, 1.0);
        let first = self.stops.first()?;
        if self.stops.len() == 1 {
            return Some(first.color);
        }

        let mut left = first;
        let mut right = first;
        for stop in &self.stops {
            if stop.offset <= t {
                left = stop;
            }
            if stop.offset >= t {
                right = stop;
                break;
            }
        }

        if (right.offset - left.offset).abs() <= f32::EPSILON {
            return Some(left.color);
        }

        let span = (t - left.offset) / (right.offset - left.offset);
        let eased = easing_registry.evaluate(left.easing_to_next, span);
        Some(
            left.color
                .mix_in_space(right.color, eased as f64, self.interpolation_space),
        )
    }
}

#[derive(Default)]
pub struct EasingRegistry {
    custom: std::collections::HashMap<u32, Box<dyn Fn(f32) -> f32 + Send + Sync>>,
}

impl EasingRegistry {
    pub fn register(
        &mut self,
        id: u32,
        easing: impl Fn(f32) -> f32 + Send + Sync + 'static,
    ) -> Option<Box<dyn Fn(f32) -> f32 + Send + Sync>> {
        self.custom.insert(id, Box::new(easing))
    }

    pub fn evaluate(&self, easing: Easing, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match easing {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - ((-2.0 * t + 2.0).powi(2) * 0.5)
                }
            }
            Easing::SmoothStep => t * t * (3.0 - 2.0 * t),
            Easing::Custom(id) => self.custom.get(&id).map(|f| f(t)).unwrap_or(t),
        }
    }
}
