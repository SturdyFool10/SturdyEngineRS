use std::{
    collections::{HashMap, hash_map::Entry},
    ptr::NonNull,
    sync::{Mutex, OnceLock},
};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_compositor, wl_region, wl_registry, wl_surface},
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1, ext_background_effect_surface_v1,
};
use wayland_protocols_plasma::blur::client::{org_kde_kwin_blur, org_kde_kwin_blur_manager};

use crate::{NativeWindowAppearanceError, WindowAppearance, WindowBackdrop};

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
delegate_noop!(WaylandDispatchState: org_kde_kwin_blur_manager::OrgKdeKwinBlurManager);
delegate_noop!(WaylandDispatchState: org_kde_kwin_blur::OrgKdeKwinBlur);

struct WaylandBackdropState {
    connection: Connection,
    surface: wl_surface::WlSurface,
    compositor: wl_compositor::WlCompositor,
    effect: WaylandBackdropEffect,
}

enum WaylandBackdropEffect {
    ExtBackground(ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1),
    KdeBlur {
        manager: org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
        blur: org_kde_kwin_blur::OrgKdeKwinBlur,
    },
}

fn wayland_states() -> &'static Mutex<HashMap<usize, WaylandBackdropState>> {
    static STATES: OnceLock<Mutex<HashMap<usize, WaylandBackdropState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn apply(
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
        disable_blur(surface_key)?;
        return Ok(());
    }

    let (width, height) = size.unwrap_or((1, 1));
    let width = width.max(1) as i32;
    let height = height.max(1) as i32;

    let mut states = wayland_states().lock().map_err(|_| {
        NativeWindowAppearanceError::ApplyFailed("wayland blur state lock poisoned".into())
    })?;
    let state = match states.entry(surface_key) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => entry.insert(create_state(display.display, window.surface)?),
    };

    let region = state
        .compositor
        .create_region(&new_queue_handle(&state.connection), ());
    region.add(0, 0, width, height);
    match &state.effect {
        WaylandBackdropEffect::ExtBackground(effect) => {
            effect.set_blur_region(Some(&region));
            state.surface.commit();
        }
        WaylandBackdropEffect::KdeBlur { blur, .. } => {
            blur.set_region(Some(&region));
            blur.commit();
        }
    }
    state
        .connection
        .flush()
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    region.destroy();
    Ok(())
}

fn disable_blur(surface_key: usize) -> Result<(), NativeWindowAppearanceError> {
    let mut states = wayland_states().lock().map_err(|_| {
        NativeWindowAppearanceError::ApplyFailed("wayland blur state lock poisoned".into())
    })?;
    let Some(state) = states.remove(&surface_key) else {
        return Ok(());
    };
    match state.effect {
        WaylandBackdropEffect::ExtBackground(effect) => {
            effect.set_blur_region(None);
            effect.destroy();
            state.surface.commit();
        }
        WaylandBackdropEffect::KdeBlur { manager, blur } => {
            manager.unset(&state.surface);
            blur.release();
            state.surface.commit();
        }
    }
    state
        .connection
        .flush()
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    Ok(())
}

fn create_state(
    display: NonNull<std::ffi::c_void>,
    surface: NonNull<std::ffi::c_void>,
) -> Result<WaylandBackdropState, NativeWindowAppearanceError> {
    match create_ext_background_state(display, surface) {
        Ok(state) => Ok(state),
        Err(ext_error) if ext_error.is_degraded() => {
            match create_kde_blur_state(display, surface) {
                Ok(state) => Ok(state),
                Err(kde_error) if kde_error.is_degraded() => {
                    Err(NativeWindowAppearanceError::Degraded(format!(
                        "{ext_error}; KDE/KWin blur fallback also unavailable: {kde_error}"
                    )))
                }
                Err(kde_error) => Err(kde_error),
            }
        }
        Err(error) => Err(error),
    }
}

fn create_ext_background_state(
    display: NonNull<std::ffi::c_void>,
    surface: NonNull<std::ffi::c_void>,
) -> Result<WaylandBackdropState, NativeWindowAppearanceError> {
    let backend =
        unsafe { wayland_client::backend::Backend::from_foreign_display(display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let (globals, mut queue) = registry_queue_init::<WaylandDispatchState>(&connection)
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    let qh = queue.handle();
    let compositor = globals
        .bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=6, ())
        .map_err(|err| {
            NativeWindowAppearanceError::Degraded(format!(
                "Wayland compositor global is unavailable; falling back to basic transparency/no native blur: {err}"
            ))
        })?;
    let manager = globals
        .bind::<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _, _>(
            &qh,
            1..=1,
            (),
        )
        .map_err(|err| {
            NativeWindowAppearanceError::Degraded(format!(
                "Wayland compositor does not expose ext-background-effect-v1; falling back to basic transparency/no native blur: {err}",
            ))
        })?;

    let surface = wrap_foreign_surface(&connection, surface)?;
    let effect = manager.get_background_effect(&surface, &qh, ());
    queue
        .roundtrip(&mut WaylandDispatchState)
        .map_err(|err| {
            NativeWindowAppearanceError::Degraded(format!(
                "Wayland compositor refused ext-background-effect-v1 setup; falling back to basic transparency/no native blur: {err}"
            ))
        })?;

    Ok(WaylandBackdropState {
        connection,
        surface,
        compositor,
        effect: WaylandBackdropEffect::ExtBackground(effect),
    })
}

fn create_kde_blur_state(
    display: NonNull<std::ffi::c_void>,
    surface: NonNull<std::ffi::c_void>,
) -> Result<WaylandBackdropState, NativeWindowAppearanceError> {
    let backend =
        unsafe { wayland_client::backend::Backend::from_foreign_display(display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let (globals, mut queue) = registry_queue_init::<WaylandDispatchState>(&connection)
        .map_err(|err| NativeWindowAppearanceError::ApplyFailed(err.to_string()))?;
    let qh = queue.handle();
    let compositor = globals
        .bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=6, ())
        .map_err(|err| {
            NativeWindowAppearanceError::Degraded(format!(
                "Wayland compositor global is unavailable; falling back to basic transparency/no native blur: {err}"
            ))
        })?;
    let manager = globals
        .bind::<org_kde_kwin_blur_manager::OrgKdeKwinBlurManager, _, _>(&qh, 1..=1, ())
        .map_err(|err| {
            NativeWindowAppearanceError::Degraded(format!(
                "Wayland compositor does not expose KDE/KWin blur; falling back to basic transparency/no native blur: {err}"
            ))
        })?;
    let surface = wrap_foreign_surface(&connection, surface)?;
    let blur = manager.create(&surface, &qh, ());
    queue
        .roundtrip(&mut WaylandDispatchState)
        .map_err(|err| {
            NativeWindowAppearanceError::Degraded(format!(
                "Wayland compositor refused KDE/KWin blur setup; falling back to basic transparency/no native blur: {err}"
            ))
        })?;

    Ok(WaylandBackdropState {
        connection,
        surface,
        compositor,
        effect: WaylandBackdropEffect::KdeBlur { manager, blur },
    })
}

fn wrap_foreign_surface(
    connection: &Connection,
    surface: NonNull<std::ffi::c_void>,
) -> Result<wl_surface::WlSurface, NativeWindowAppearanceError> {
    let surface_id = unsafe {
        wayland_client::backend::ObjectId::from_ptr(
            wl_surface::WlSurface::interface(),
            surface.as_ptr().cast(),
        )
    }
    .map_err(|_| {
        NativeWindowAppearanceError::Degraded(
            "invalid foreign wl_surface; falling back to basic transparency/no native blur".into(),
        )
    })?;
    wl_surface::WlSurface::from_id(connection, surface_id).map_err(|_| {
        NativeWindowAppearanceError::Degraded(
            "failed to wrap foreign wl_surface; falling back to basic transparency/no native blur"
                .into(),
        )
    })
}

fn new_queue_handle(connection: &Connection) -> QueueHandle<WaylandDispatchState> {
    connection
        .new_event_queue::<WaylandDispatchState>()
        .handle()
}
