use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ElementId {
    pub hash: u64,
    pub label: String,
    pub index: u32,
    pub parent: u64,
}

impl ElementId {
    pub fn new(label: impl Into<String>) -> Self {
        Self::indexed(label, 0)
    }

    pub fn indexed(label: impl Into<String>, index: u32) -> Self {
        let label = label.into();
        Self::with_parent(label, index, 0)
    }

    pub fn local(label: impl Into<String>, index: u32, parent: &ElementId) -> Self {
        Self::with_parent(label.into(), index, parent.hash)
    }

    pub fn anonymous(parent_hash: u64, sibling_index: usize) -> Self {
        Self::with_parent(
            format!("anon:{sibling_index}"),
            sibling_index as u32,
            parent_hash,
        )
    }

    fn with_parent(label: String, index: u32, parent: u64) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        label.hash(&mut hasher);
        index.hash(&mut hasher);
        parent.hash(&mut hasher);
        let hash = hasher.finish();
        Self {
            hash,
            label,
            index,
            parent,
        }
    }
}

impl Hash for ElementId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}
