macro_rules! define_handle {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
        pub struct $name(pub(crate) u64);

        impl $name {
            /// Return the raw numeric handle value for diagnostics, FFI bridging, or serialization.
            pub const fn as_raw(self) -> u64 {
                self.0
            }

            /// Reconstruct a handle from a raw numeric value.
            ///
            /// # Safety
            ///
            /// The value must have been produced by this engine instance for the same
            /// handle type, and the referenced resource/token must still be valid for
            /// the API call that receives it. Prefer safe engine APIs that return typed
            /// resource wrappers whenever possible.
            pub const unsafe fn from_raw(raw: u64) -> Self {
                Self(raw)
            }
        }
    };
}

macro_rules! define_ordered_handle {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub(crate) u64);

        impl $name {
            /// Return the raw numeric handle value for diagnostics, FFI bridging, or serialization.
            pub const fn as_raw(self) -> u64 {
                self.0
            }

            /// Reconstruct a handle from a raw numeric value.
            ///
            /// # Safety
            ///
            /// The value must have been produced by this engine instance for the same
            /// handle type, and the referenced resource/token must still be valid for
            /// the API call that receives it. Prefer safe engine APIs that return typed
            /// resource wrappers whenever possible.
            pub const unsafe fn from_raw(raw: u64) -> Self {
                Self(raw)
            }
        }
    };
}

define_handle!(DeviceHandle);
define_handle!(ImageHandle);
define_handle!(BufferHandle);
define_handle!(SamplerHandle);
define_handle!(ShaderHandle);
define_handle!(FrameHandle);
define_handle!(PassHandle);
define_handle!(PipelineLayoutHandle);
define_handle!(PipelineHandle);
define_handle!(AccelerationStructureHandle);
define_handle!(BindGroupHandle);
define_handle!(SurfaceHandle);

define_ordered_handle!(
    #[doc = "Opaque token returned by `flush`. Callers can pass it to"]
    #[doc = "`Device::wait_for_submission` to block until the GPU finishes that frame."]
    SubmissionHandle
);

define_ordered_handle!(VideoSessionHandle);
define_ordered_handle!(IndirectCommandLayoutHandle);
define_ordered_handle!(OpticalFlowSessionHandle);
define_handle!(SemaphoreHandle);

define_handle!(
    #[doc = "Handle for an externally-exportable Vulkan fence."]
    #[doc = "Created by `Device::create_exportable_fence`. Requires `BackendFeatures::external_fence_fd`."]
    FenceHandle
);

#[derive(Debug, Default)]
pub(crate) struct HandleAllocator {
    next: u64,
}

impl HandleAllocator {
    pub(crate) fn alloc(&mut self) -> u64 {
        self.next = self.next.max(1);
        let handle = self.next;
        self.next += 1;
        handle
    }
}
