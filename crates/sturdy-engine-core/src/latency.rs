#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ReflexMode {
    #[default]
    Off,
    On,
    OnPlusBoost,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum AntiLagMode {
    #[default]
    Off,
    On,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LatencyMode {
    Reflex(ReflexMode),
    AntiLag(AntiLagMode),
}
