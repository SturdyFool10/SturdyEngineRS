/// Entry point specification for `Engine::load_slang_source`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlangEntryPoints {
    Graphics { vertex: String, fragment: String },
    Fragment { fragment: String },
    Compute { compute: String },
}

impl SlangEntryPoints {
    pub fn graphics(vertex: impl Into<String>, fragment: impl Into<String>) -> Self {
        Self::Graphics {
            vertex: vertex.into(),
            fragment: fragment.into(),
        }
    }

    pub fn fragment(fragment: impl Into<String>) -> Self {
        Self::Fragment {
            fragment: fragment.into(),
        }
    }

    pub fn compute(compute: impl Into<String>) -> Self {
        Self::Compute {
            compute: compute.into(),
        }
    }
}
