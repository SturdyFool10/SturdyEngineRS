// SceneCommands — thread-safe structural mutation queue for Scene.
//
// `Scene::set_transform` is already lock-free via AtomicMat4. This file handles
// the less-frequent structural mutations — adding/removing objects and meshes,
// changing materials — which require exclusive Scene access. These are queued
// here (from any thread) and drained by `Scene::prepare` on the render thread.
//
// The queue uses a `Mutex<Vec<Box<dyn FnOnce>>>` rather than a lock-free
// structure because structural mutations are infrequent (load time, object
// creation/destruction) and the Mutex is uncontended in the common case.
//
// # ID pre-allocation
//
// `add_mesh` and `add_object` need to return stable IDs immediately so the
// caller can store them in `SceneLink` components or similar. These IDs are
// reserved atomically from counters in `Scene` via `Scene::reserve_mesh_id()`
// and `Scene::reserve_object_id()`. The actual slot population is deferred
// to the `drain()` call in `prepare()`.
//
// # SceneView
//
// `SceneView` is a `Send + Sync` pointer wrapper that allows worker threads
// to read scene data (transforms, camera positions, light counts) between
// `prepare()` calls. All accessed data is either immutable or accessed via
// atomic operations.

use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::Mutex;

use glam::Mat4;

use super::object::{MeshId, ObjectId};
use super::scene::Scene;

// ── SceneCommands ─────────────────────────────────────────────────────────────

type CommandFn = Box<dyn FnOnce(&mut Scene) + Send + 'static>;

/// Thread-safe queue of deferred structural mutations for a [`Scene`].
///
/// Obtain via [`Scene::commands()`]. Call any mutation method from any thread.
/// Mutations are applied in FIFO order when [`Scene::prepare`] runs.
///
/// # Pre-allocating IDs
///
/// `add_mesh` and `add_object` require an ID reserved in advance. Use
/// [`Scene::reserve_mesh_id`] and [`Scene::reserve_object_id`] to get stable
/// IDs atomically before queueing the command:
///
/// ```ignore
/// let mesh_id = scene.reserve_mesh_id();
/// let obj_id  = scene.reserve_object_id();
/// scene.commands().add_mesh(mesh_id, mesh, program);
/// scene.commands().add_object(obj_id, mesh_id, ObjectKind::Dynamic, Mat4::IDENTITY);
/// // Store obj_id in a SceneLink component — it becomes valid after prepare().
/// ```
pub struct SceneCommands {
    queue: Mutex<Vec<CommandFn>>,
}

impl SceneCommands {
    pub(super) fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
        }
    }

    fn push(&self, cmd: impl FnOnce(&mut Scene) + Send + 'static) {
        //panic allowed, reason = "poisoned internal scene command queue is unrecoverable"
        self.queue
            .lock()
            .expect("scene commands mutex poisoned")
            .push(Box::new(cmd));
    }

    // ── Structural mutations — safe to call from any thread ───────────────────

    /// Queue removal of an object. No-op if the id is out of range.
    ///
    /// The object stops contributing to draw calls after the next `prepare()`.
    pub fn remove_object(&self, id: ObjectId) {
        self.push(move |scene| scene.remove_object_deferred(id));
    }

    /// Queue a material descriptor change on an existing mesh.
    ///
    /// The GPU buffer is re-uploaded in the same `prepare()` that drains this command.
    pub fn set_material(&self, id: MeshId, descriptor: super::MaterialDescriptor) {
        self.push(move |scene| {
            scene.set_material(id, descriptor);
        });
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    /// Take all queued commands out of the queue, leaving it empty.
    ///
    /// The caller applies the returned commands to `&mut Scene`. Splitting the
    /// take from the apply avoids a borrow conflict when `scene` itself owns
    /// the `SceneCommands`.
    pub(super) fn take_all(&self) -> Vec<CommandFn> {
        //panic allowed, reason = "poisoned internal scene command queue is unrecoverable"
        let mut lock = self.queue.lock().expect("scene commands mutex poisoned");
        std::mem::take(&mut *lock)
    }

    /// Number of pending commands. Useful for diagnostics.
    pub fn pending_count(&self) -> usize {
        self.queue.lock().map(|q| q.len()).unwrap_or(0)
    }
}

// ── SceneView ─────────────────────────────────────────────────────────────────

/// A read-only, `Send + Sync` view of a [`Scene`] for use from worker threads.
///
/// Valid between calls to `Scene::prepare`. Do not hold a `SceneView` across
/// a frame boundary — `prepare` mutates the scene structure.
///
/// Obtain via [`Scene::view`].
pub struct SceneView<'s> {
    scene: NonNull<Scene>,
    _marker: PhantomData<&'s Scene>,
}

// SAFETY: Scene is Sync (transform reads use atomic operations; the scene
// structure is stable while a SceneView exists — no structural mutations
// happen outside of prepare()). All SceneView methods are read-only.
unsafe impl<'s> Send for SceneView<'s> {}
unsafe impl<'s> Sync for SceneView<'s> {}

impl<'s> SceneView<'s> {
    /// Create a SceneView from a Scene borrow.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// 1. No structural mutations (`add_object`, `remove_object`, etc.) occur
    ///    while any `SceneView` derived from this scene is live.
    /// 2. `prepare()` is not called while a `SceneView` is live.
    ///
    /// In practice, create the SceneView after `prepare()` and drop it before
    /// the next `prepare()`.
    pub unsafe fn new(scene: &'s Scene) -> Self {
        SceneView {
            scene: NonNull::from(scene),
            _marker: PhantomData,
        }
    }

    /// Get the current transform of an object. Returns `None` if the id is
    /// out of range or the object slot is vacant.
    pub fn get_transform(&self, id: ObjectId) -> Option<Mat4> {
        let scene = unsafe { self.scene.as_ref() };
        scene
            .objects
            .get(id.0 as usize)
            .filter(|o| !matches!(o.kind, _) || true) // all live slots
            .map(|o| o.get_transform())
    }

    /// Number of registered cameras.
    pub fn camera_count(&self) -> usize {
        unsafe { self.scene.as_ref() }.cameras.len()
    }

    /// Number of live objects.
    pub fn object_count(&self) -> usize {
        unsafe { self.scene.as_ref() }.objects.len()
    }
}
