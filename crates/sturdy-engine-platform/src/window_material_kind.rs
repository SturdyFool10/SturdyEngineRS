#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowMaterialKind {
    Auto,
    ThinTranslucent,
    ThickTranslucent,
    NoiseTranslucent,
    TitlebarTranslucent,
    Hud,
}
