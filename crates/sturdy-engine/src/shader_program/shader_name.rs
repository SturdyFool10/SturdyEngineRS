/// An opaque shader program name for diagnostics and hot-reload tracking.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ShaderName(String);

impl ShaderName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
