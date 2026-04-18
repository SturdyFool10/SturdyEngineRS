#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum GpuCaptureTool {
    RenderDoc,
    Pix,
    Xcode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuCaptureDesc {
    pub tool: GpuCaptureTool,
    pub label: String,
}

impl GpuCaptureDesc {
    pub fn new(tool: GpuCaptureTool, label: impl Into<String>) -> Self {
        Self {
            tool,
            label: label.into(),
        }
    }
}
