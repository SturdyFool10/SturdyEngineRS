use super::push_vertex::PushVertex;

pub fn push_vertices() -> [PushVertex; 4] {
    [
        PushVertex {
            position: [-0.55, -0.45],
            color: [1.0, 0.2, 0.1],
        },
        PushVertex {
            position: [0.55, -0.45],
            color: [0.1, 0.85, 0.25],
        },
        PushVertex {
            position: [0.55, 0.45],
            color: [0.2, 0.35, 1.0],
        },
        PushVertex {
            position: [-0.55, 0.45],
            color: [1.0, 0.9, 0.2],
        },
    ]
}

pub fn push_indices() -> [u16; 6] {
    [0, 1, 2, 0, 2, 3]
}
