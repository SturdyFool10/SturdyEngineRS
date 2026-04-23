#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowEffectRegion {
    FullWindow,
    ClientArea,
    Titlebar,
}
