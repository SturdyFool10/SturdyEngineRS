#[path = "input/helpers.rs"]
mod helpers;
#[path = "input/simulator.rs"]
mod simulator;
#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
#[path = "input/types.rs"]
mod types;

pub use simulator::InputSimulator;
pub use types::{
    Cx, EventContext, EventPhase, FocusScope, Hit, InputEvent, InteractionPhase, ModifierKeys,
    PendingRegistrations, PointerButton, PointerState, ScrollAxis, ScrollConfig, ScrollState,
    SliderConfig, UiActivationEvent, UiEventResult, UiKeyEvent, UiMode, UiPointerEvent,
    UiTextEvent, WidgetBehavior, WidgetConfig, WidgetEventCallbacks, WidgetKind, WidgetState,
};

use helpers::{
    focus_scope_contains, focus_scope_contains_with_parent_map, layout_parent_map,
    scroll_key_delta, slider_key_delta, slider_normalized_from_rect,
};
