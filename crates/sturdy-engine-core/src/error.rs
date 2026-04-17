use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Error {
    InvalidHandle,
    Unsupported(&'static str),
    CompileFailed(String),
    OutOfMemory,
    InvalidInput(String),
    Backend(String),
    Unknown(String),
}

impl Error {
    pub fn code(&self) -> i32 {
        match self {
            Self::InvalidHandle => 1,
            Self::Unsupported(_) => 2,
            Self::CompileFailed(_) => 3,
            Self::OutOfMemory => 4,
            Self::InvalidInput(_) => 5,
            Self::Backend(_) => 6,
            Self::Unknown(_) => 0x7fff_ffff,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHandle => write!(f, "invalid handle"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            Self::CompileFailed(msg) => write!(f, "shader compile failed: {msg}"),
            Self::OutOfMemory => write!(f, "out of memory"),
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
            Self::Unknown(msg) => write!(f, "unknown error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}
