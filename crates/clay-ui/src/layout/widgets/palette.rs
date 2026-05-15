use crate::{
    Axis, ColorSpaceKind, Cx, Easing, ElementId, SliderConfig, UiColor, WidgetBehavior, WidgetState,
};

/// Configuration for toggle switch animations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToggleAnimConfig {
    /// Seconds elapsed since the last frame (pass 0.0 to snap with no animation).
    pub delta_time: f32,
    /// Duration of the full on->off or off->on transition in seconds.
    pub duration: f32,
    /// Easing function applied to the linear progress before use for rendering.
    pub easing: Easing,
    /// Color space used when interpolating the track background.
    pub color_space: ColorSpaceKind,
}

impl Default for ToggleAnimConfig {
    fn default() -> Self {
        Self {
            delta_time: 0.0,
            duration: 0.15,
            easing: Easing::EaseInOut,
            color_space: ColorSpaceKind::LinearSrgb,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetPalette {
    pub text: UiColor,
    pub muted_text: UiColor,
    pub surface: UiColor,
    pub surface_hovered: UiColor,
    pub surface_pressed: UiColor,
    pub surface_selected: UiColor,
    pub surface_disabled: UiColor,
    pub outline: UiColor,
    pub outline_focus: UiColor,
    pub outline_invalid: UiColor,
    pub accent: UiColor,
    pub accent_text: UiColor,
}

impl Default for WidgetPalette {
    fn default() -> Self {
        Self {
            text: UiColor::from_rgba8(226, 232, 240, 255),
            muted_text: UiColor::from_rgba8(148, 163, 184, 255),
            surface: UiColor::from_rgba8(15, 23, 42, 255),
            surface_hovered: UiColor::from_rgba8(30, 41, 59, 255),
            surface_pressed: UiColor::from_rgba8(51, 65, 85, 255),
            surface_selected: UiColor::from_rgba8(37, 99, 235, 255),
            surface_disabled: UiColor::from_rgba8(24, 31, 43, 180),
            outline: UiColor::from_rgba8(148, 163, 184, 80),
            outline_focus: UiColor::from_rgba8(96, 165, 250, 255),
            outline_invalid: UiColor::from_rgba8(248, 113, 113, 255),
            accent: UiColor::from_rgba8(59, 130, 246, 255),
            accent_text: UiColor::WHITE,
        }
    }
}

pub trait WidgetRenderContext {
    fn widget_state(&self, id: &ElementId) -> WidgetState;
    fn widget_palette(&self) -> WidgetPalette;

    fn register_widget_behavior(&self, _id: ElementId, _behavior: WidgetBehavior) {}

    fn register_slider_widget(&self, id: ElementId, axis: Axis, config: SliderConfig) {
        self.register_widget_behavior(id, WidgetBehavior::slider(axis).pointer_drag(true));
        let _ = config;
    }

    fn slider_display_value(&self, _id: &ElementId, config: SliderConfig) -> f32 {
        let range = (config.max - config.min).max(f32::EPSILON);
        ((config.initial - config.min) / range).clamp(0.0, 1.0)
    }

    fn register_text_input_widget(&self, id: ElementId) {
        self.register_widget_behavior(id, WidgetBehavior::text_input());
    }

    fn register_drag_bar_widget(&self, id: ElementId, axis: Axis) {
        self.register_widget_behavior(id, WidgetBehavior::drag_bar(axis));
    }

    /// Returns the eased animation progress (0.0=off, 1.0=on) for a toggle.
    /// Default implementation returns `target` instantly (no animation).
    fn advance_toggle_animation(
        &self,
        id: &ElementId,
        target: f32,
        _config: ToggleAnimConfig,
    ) -> f32 {
        let _ = id;
        target
    }
}

impl WidgetRenderContext for Cx<'_> {
    fn widget_state(&self, id: &ElementId) -> WidgetState {
        self.state(id)
    }

    fn widget_palette(&self) -> WidgetPalette {
        self.palette
    }

    fn register_widget_behavior(&self, id: ElementId, behavior: WidgetBehavior) {
        self.register_behavior(id, behavior);
    }

    fn register_slider_widget(&self, id: ElementId, axis: Axis, config: SliderConfig) {
        self.register_slider(id, axis, config);
    }

    fn slider_display_value(&self, id: &ElementId, _config: SliderConfig) -> f32 {
        self.slider_value_normalized(id)
    }

    fn register_text_input_widget(&self, id: ElementId) {
        self.register_text_input(id);
    }

    fn register_drag_bar_widget(&self, id: ElementId, axis: Axis) {
        self.register_drag_bar(id, axis);
    }

    fn advance_toggle_animation(
        &self,
        id: &ElementId,
        target: f32,
        config: ToggleAnimConfig,
    ) -> f32 {
        self.advance_toggle_animation(
            id,
            target,
            config.duration,
            config.delta_time,
            config.easing,
        )
    }
}

impl WidgetRenderContext for WidgetState {
    fn widget_state(&self, _id: &ElementId) -> WidgetState {
        self.clone()
    }

    fn widget_palette(&self) -> WidgetPalette {
        WidgetPalette::default()
    }
}

impl WidgetRenderContext for WidgetPalette {
    fn widget_state(&self, _id: &ElementId) -> WidgetState {
        WidgetState::default()
    }

    fn widget_palette(&self) -> WidgetPalette {
        *self
    }
}
