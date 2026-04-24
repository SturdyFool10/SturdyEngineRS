use std::{collections::HashMap, ptr::NonNull, sync::{Mutex, OnceLock}};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_compositor, wl_region, wl_registry, wl_surface},
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1, ext_background_effect_surface_v1,
};

use crate::{
    NativeWindowAppearanceError, PlatformCapabilityState, PlatformKind, WindowAppearance,
    WindowAppearanceCaps, WindowBackdrop,
};

/// Linux platform adapter entry point.
///
/// Planned backdrop/effect support:
/// - Wayland `ext-background-effect-v1` as the primary protocol path
/// - older compositor-specific blur protocols only as compatibility fallbacks
/// - clean fallback to transparency/no backdrop effect when no supported
///   compositor protocol is available
pub fn platform_kind() -> PlatformKind {
    PlatformKind::Linux
}

pub fn window_appearance_caps() -> WindowAppearanceCaps {
    WindowAppearanceCaps {
        transparency: Some(PlatformCapabilityState::Supported),
        blur: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
        materials: Vec::new(),
        custom_regions: Some(PlatformCapabilityState::Unsupported),
        live_reconfiguration: Some(PlatformCapabilityState::RuntimeReconfigureSupported),
    }
}

#[derive(Default)]
struct WaylandDispatchState;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandDispatchState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(WaylandDispatchState: wl_compositor::WlCompositor);
delegate_noop!(WaylandDispatchState: wl_region::WlRegion);
delegate_noop!(WaylandDispatchState: wl_surface::WlSurface);
delegate_noop!(WaylandDispatchState: ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1);
delegate_noop!(WaylandDispatchState: ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1);

struct WaylandBackdropState {
    connection: Connection,
    surface: wl_surface::WlSurface,
    effect: ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
    compositor: wl_compositor::WlCompositor,
}

fn wayland_states() -> &'static Mutex<HashMap<usize, WaylandBackdropState>> {
    static STATES: OnceLock<Mutex<HashMap<usize, WaylandBackdropState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn apply_native_window_appearance(
    display: RawDisplayHandle,
    window: RawWindowHandle,
    size: Option<(u32, u32)>,
    appearance: WindowAppearance,
) -> Result<(), NativeWindowAppearanceError> {
    let RawDisplayHandle::Wayland(display) = display else {
        return Err(NativeWindowAppearanceError::UnsupportedDisplayHandle);
    };
    let RawWindowHandle::Wayland(window) = window else {
        return Err(NativeWindowAppearanceError::UnsupportedWindowHandle);
    };

    let surface_key = window.surface.as_ptr() as usize;
    let wants_blur = matches!(
        appearance.backdrop,
        WindowBackdrop::Blurred(_) | WindowBackdrop::Material(_)
    );

    if !wants_blur {
        disable_wayland_blur(surface_key)?;
        return Ok(());
    }

    let (width, height) = size.unwrap_or((1, 1));
    let width = width.max(1) as i32;
    let height = height.max(1) as i32;

    let mut states = wayland_states()
        .lock()
        .map_err(|_| NativeWindowAppearanceError::ApplyFailed("wayland blur state lock poisoned".into()))?;
    let state = if let Some(state) = states.get_mut(&surface_key) {
        state
    } else {
        states.insert(
            surface_key,
            create_wayland_state(display.display, window.surface)?,
        );
        states.get_mut(&surface_key).expect("inserted state disappeared")
    };

    let region = state.compositor.create_region(&new_queue_handle(&state.connection), ());
    region.add(0, 0, width, height);
    state.effect.set_blur_region(Some(&region));
    state.surface.commit();
    state.connection
        .flush()
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    region.destroy();
    Ok(())
}

fn disable_wayland_blur(surface_key: usize) -> Result<(), NativeWindowAppearanceError> {
    let mut states = wayland_states()
        .lock()
        .map_err(|_| NativeWindowAppearanceError::ApplyFailed("wayland blur state lock poisoned".into()))?;
    let Some(state) = states.remove(&surface_key) else {
        return Ok(());
    };
    state.effect.set_blur_region(None);
    state.effect.destroy();
    state.surface.commit();
    state.connection
        .flush()
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    Ok(())
}

fn create_wayland_state(
    display: NonNull<std::ffi::c_void>,
    surface: NonNull<std::ffi::c_void>,
) -> Result<WaylandBackdropState, NativeWindowAppearanceError> {
    let backend = unsafe {
        wayland_client::backend::Backend::from_foreign_display(display.as_ptr().cast())
    };
    let connection = Connection::from_backend(backend);
    let (globals, mut queue) = registry_queue_init::<WaylandDispatchState>(&connection)
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    let qh = queue.handle();
    let compositor = globals
        .bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=6, ())
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    let manager = globals
        .bind::<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _, _>(&qh, 1..=1, ())
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(format!(
            "Wayland compositor does not expose ext-background-effect-v1: {err}",
        )))?;

    let surface_id = unsafe {
        wayland_client::backend::ObjectId::from_ptr(
            wl_surface::WlSurface::interface(),
            surface.as_ptr().cast(),
        )
    }
    .map_err(|_| NativeWindowAppearanceError::ApplyFailed("invalid foreign wl_surface".into()))?;
    let surface = wl_surface::WlSurface::from_id(&connection, surface_id)
        .map_err(|_| NativeWindowAppearanceError::ApplyFailed("failed to wrap foreign wl_surface".into()))?;
    let effect = manager.get_background_effect(&surface, &qh, ());
    queue
        .roundtrip(&mut WaylandDispatchState)
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;

    Ok(WaylandBackdropState {
        connection,
        surface,
        effect,
        compositor,
    })
}

fn new_queue_handle(connection: &Connection) -> QueueHandle<WaylandDispatchState> {
    connection.new_event_queue::<WaylandDispatchState>().handle()
}
