// AtomicMat4 — a 4×4 f32 matrix stored as 8 AtomicU64 values.
//
// Each AtomicU64 holds two consecutive f32 values (column-major layout):
//   slot 0 = (col0.x, col0.y), slot 1 = (col0.z, col0.w)
//   slot 2 = (col1.x, col1.y), slot 3 = (col1.z, col1.w)
//   ...
//
// Store uses Release ordering, load uses Acquire ordering. This guarantees
// that any thread that observes any of the written u64 values also sees all
// stores that happened before the Release store. For transforms written by one
// system thread and read by the render thread at prepare() time, this gives
// the correct sequencing without any locks.
//
// # Limitation
//
// An AtomicMat4 write is NOT a single atomic transaction across all 8 slots.
// If two threads write the SAME AtomicMat4 simultaneously, the result is an
// interleaved mix of the two matrices. The ECS scheduler prevents this for
// SceneLink-coupled objects by ensuring at most one system writes a given
// entity's transform per wave.

use glam::Mat4;
use std::sync::atomic::{AtomicU64, Ordering};

// ── AtomicMat4 ────────────────────────────────────────────────────────────────

pub struct AtomicMat4([AtomicU64; 8]);

impl AtomicMat4 {
    pub fn new(mat: Mat4) -> Self {
        let s = Self(std::array::from_fn(|_| AtomicU64::new(0)));
        s.store(mat);
        s
    }

    /// Store `mat` with Release semantics.
    ///
    /// After this returns, any thread that loads any slot of this AtomicMat4
    /// with Acquire will observe all stores that happened before this call.
    pub fn store(&self, mat: Mat4) {
        let cols = mat.to_cols_array(); // [f32; 16], column-major
        for (i, chunk) in cols.chunks_exact(2).enumerate() {
            let lo = chunk[0].to_bits() as u64;
            let hi = chunk[1].to_bits() as u64;
            self.0[i].store(lo | (hi << 32), Ordering::Release);
        }
    }

    /// Load the current matrix with Acquire semantics.
    pub fn load(&self) -> Mat4 {
        let mut cols = [0f32; 16];
        for i in 0..8 {
            let val = self.0[i].load(Ordering::Acquire);
            cols[i * 2] = f32::from_bits((val & 0xFFFF_FFFF) as u32);
            cols[i * 2 + 1] = f32::from_bits((val >> 32) as u32);
        }
        Mat4::from_cols_array(&cols)
    }
}

impl Default for AtomicMat4 {
    fn default() -> Self {
        Self::new(Mat4::IDENTITY)
    }
}

// AtomicU64 is Send+Sync, so AtomicMat4 is too.
// The derive would give this automatically since AtomicU64: Send+Sync.
