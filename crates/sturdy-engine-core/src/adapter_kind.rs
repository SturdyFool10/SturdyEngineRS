#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum AdapterKind {
    DiscreteGpu,
    IntegratedGpu,
    VirtualGpu,
    Cpu,
    #[default]
    Unknown,
}
