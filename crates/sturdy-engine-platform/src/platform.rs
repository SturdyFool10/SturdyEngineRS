#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PlatformKind {
    Windows,
    Macos,
    Linux,
    Unknown,
}

pub fn current_platform() -> PlatformKind {
    #[cfg(target_os = "windows")]
    {
        return PlatformKind::Windows;
    }

    #[cfg(target_os = "macos")]
    {
        return PlatformKind::Macos;
    }

    #[cfg(target_os = "linux")]
    {
        return PlatformKind::Linux;
    }

    #[allow(unreachable_code)]
    PlatformKind::Unknown
}
