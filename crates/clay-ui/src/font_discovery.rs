use fontdb::{Database, Family, Query, Stretch, Style, Weight};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFaceSummary {
    pub family_names: Vec<String>,
    pub post_script_name: String,
    pub style: String,
    pub weight: u16,
    pub stretch: String,
    pub monospaced: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontSearchQuery {
    pub family: Option<String>,
    pub family_contains: Option<String>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub monospaced: Option<bool>,
}

#[derive(Debug)]
pub struct FontDiscovery {
    database: Database,
}

impl Default for FontDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl FontDiscovery {
    pub fn new() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();
        Self { database }
    }

    pub fn with_database(database: Database) -> Self {
        Self { database }
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn database_mut(&mut self) -> &mut Database {
        &mut self.database
    }

    pub fn load_system_fonts(&mut self) {
        self.database.load_system_fonts();
    }

    pub fn load_font_data(&mut self, bytes: Vec<u8>) {
        self.database.load_font_data(bytes);
    }

    pub fn search(&self, query: &FontSearchQuery) -> Vec<FontFaceSummary> {
        self.database
            .faces()
            .filter(|face| matches_query(face, query))
            .map(summary_for_face)
            .collect()
    }

    pub fn resolve_family(&self, family: &str) -> Option<FontFaceSummary> {
        let query = Query {
            families: &[Family::Name(family)],
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        };
        self.database
            .query(&query)
            .and_then(|id| self.database.face(id))
            .map(summary_for_face)
    }

    pub fn known_families(&self) -> Vec<String> {
        let mut families = self
            .database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect::<Vec<_>>();
        families.sort_unstable();
        families.dedup();
        families
    }

    pub fn fallback_family_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();
        for family in platform_fallback_families() {
            if self.resolve_family(family).is_some() {
                candidates.push(family.to_string());
            }
        }
        if candidates.is_empty() {
            candidates = self.known_families();
        }
        candidates
    }

    pub fn resolve_family_candidates<I, S>(&self, families: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut resolved = Vec::new();
        for family in families {
            let family = family.as_ref();
            if self.resolve_family(family).is_some() {
                resolved.push(family.to_string());
            }
        }
        if resolved.is_empty() {
            self.fallback_family_candidates()
        } else {
            resolved
        }
    }
}

fn summary_for_face(face: &fontdb::FaceInfo) -> FontFaceSummary {
    FontFaceSummary {
        family_names: face.families.iter().map(|(name, _)| name.clone()).collect(),
        post_script_name: face.post_script_name.clone(),
        style: format!("{:?}", face.style),
        weight: face.weight.0,
        stretch: format!("{:?}", face.stretch),
        monospaced: face.monospaced,
    }
}

fn matches_query(face: &fontdb::FaceInfo, query: &FontSearchQuery) -> bool {
    if let Some(monospaced) = query.monospaced
        && face.monospaced != monospaced
    {
        return false;
    }

    if let Some(weight) = query.weight
        && face.weight.0 != weight
    {
        return false;
    }

    if let Some(italic) = query.italic {
        let is_italic = matches!(face.style, Style::Italic | Style::Oblique);
        if is_italic != italic {
            return false;
        }
    }

    if let Some(family) = &query.family
        && !face
            .families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(family))
    {
        return false;
    }

    if let Some(substr) = &query.family_contains {
        let needle = substr.to_ascii_lowercase();
        if !face
            .families
            .iter()
            .any(|(name, _)| name.to_ascii_lowercase().contains(&needle))
        {
            return false;
        }
    }

    true
}

#[cfg(target_os = "macos")]
fn platform_fallback_families() -> &'static [&'static str] {
    &[
        "SF Pro Text",
        ".SF NS",
        "Helvetica Neue",
        "Menlo",
        "Monaco",
        "Times New Roman",
        "Times",
        "Apple Color Emoji",
    ]
}

#[cfg(target_os = "windows")]
fn platform_fallback_families() -> &'static [&'static str] {
    &[
        "Segoe UI Variable",
        "Segoe UI",
        "Arial",
        "Cascadia Mono",
        "Consolas",
        "Courier New",
        "Times New Roman",
        "Georgia",
    ]
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn platform_fallback_families() -> &'static [&'static str] {
    &[
        "Inter",
        "Noto Sans",
        "Cantarell",
        "Ubuntu",
        "DejaVu Sans",
        "Noto Sans Mono",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Serif",
        "DejaVu Serif",
        "Liberation Serif",
        "Noto Color Emoji",
    ]
}
