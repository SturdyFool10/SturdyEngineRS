/// Summary returned by [`RenderWorld::extract_from_world`](super::RenderWorld::extract_from_world).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderExtractionStats {
    /// Entities that received a new [`LocalToWorld`](super::LocalToWorld) link.
    pub allocated_objects: usize,
    /// ECS entities with a render-world link that were visited.
    pub extracted_entities: usize,
    /// Commands drained into the render-world staging state.
    pub applied_commands: usize,
}
