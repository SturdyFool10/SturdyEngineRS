#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum SurfaceColorSpace {
    #[default]
    Unknown,
    SrgbNonlinear,
    DisplayP3Nonlinear,
    ExtendedSrgbLinear,
    Hdr10St2084,
    Hdr10Hlg,
}
