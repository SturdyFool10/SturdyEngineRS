#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct DeviceHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct ImageHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct BufferHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct SamplerHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct ShaderHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct FrameHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct PassHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct PipelineLayoutHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct PipelineHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct BindGroupHandle(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct SurfaceHandle(pub u64);

/// Opaque token returned by `flush`.  Callers can pass it to
/// `Device::wait_for_submission` to block until the GPU finishes that frame.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SubmissionHandle(pub u64);

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
