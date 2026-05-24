use std::time::{SystemTime, UNIX_EPOCH};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Vec3};

pub const SCENE_CORNELL: u32 = 0;
pub const SCENE_OUTDOOR: u32 = 1;

const MATERIAL_DIFFUSE: u32 = 0;
const MATERIAL_LIGHT: u32 = 1;
const MATERIAL_GLASS: u32 = 2;

const SHAPE_FLAT: u32 = 0;
const SHAPE_SPHERE: u32 = 1;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct PathTracerVertex {
    pub position: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct PathTracerTriangleInfo {
    pub normal: [f32; 3],
    pub material: u32,
    pub albedo: [f32; 3],
    pub shape: u32,
    pub emission: [f32; 3],
    pub _pad0: u32,
    pub sphere_center: [f32; 3],
    pub _pad1: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathTracerSubscene {
    CornellBox,
    Outdoor,
}

impl PathTracerSubscene {
    pub const DEFAULT: Self = Self::CornellBox;

    pub fn from_slot(slot: u32) -> Option<Self> {
        match slot {
            1 => Some(Self::CornellBox),
            2 => Some(Self::Outdoor),
            _ => None,
        }
    }

    pub fn shifted_digit_slot(key: &str) -> Option<u32> {
        match key {
            "!" => Some(1),
            "@" => Some(2),
            "#" => Some(3),
            "$" => Some(4),
            "%" => Some(5),
            "^" => Some(6),
            "&" => Some(7),
            "*" => Some(8),
            "(" => Some(9),
            _ => None,
        }
    }

    pub fn slot(self) -> u32 {
        match self {
            Self::CornellBox => 1,
            Self::Outdoor => 2,
        }
    }

    pub fn scene_id(self) -> u32 {
        match self {
            Self::CornellBox => SCENE_CORNELL,
            Self::Outdoor => SCENE_OUTDOOR,
        }
    }

    pub fn cache_key(self) -> &'static str {
        match self {
            Self::CornellBox => "cornell",
            Self::Outdoor => "outdoor",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CornellBox => "Cornell box",
            Self::Outdoor => "random outdoor",
        }
    }

    pub fn picker_line() -> &'static str {
        "path tracer subscenes: Shift+1 Cornell  Shift+2 outdoor"
    }
}

pub struct PathTracerSceneGeometry {
    pub vertices: Vec<PathTracerVertex>,
    pub indices: Vec<u32>,
    pub triangle_infos: Vec<PathTracerTriangleInfo>,
    pub scene_id: u32,
}

pub fn geometry_for_subscene(subscene: PathTracerSubscene) -> PathTracerSceneGeometry {
    match subscene {
        PathTracerSubscene::CornellBox => cornell_geometry(),
        PathTracerSubscene::Outdoor => outdoor_geometry(),
    }
}

struct MeshBuilder {
    vertices: Vec<PathTracerVertex>,
    indices: Vec<u32>,
    triangle_infos: Vec<PathTracerTriangleInfo>,
}

impl MeshBuilder {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            triangle_infos: Vec::new(),
        }
    }

    fn finish(self, scene_id: u32) -> PathTracerSceneGeometry {
        PathTracerSceneGeometry {
            vertices: self.vertices,
            indices: self.indices,
            triangle_infos: self.triangle_infos,
            scene_id,
        }
    }

    fn push_triangle(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        normal: Vec3,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
        shape: u32,
        sphere_center: Vec3,
    ) {
        let base = self.vertices.len() as u32;
        self.vertices.extend([
            PathTracerVertex {
                position: a.to_array(),
            },
            PathTracerVertex {
                position: b.to_array(),
            },
            PathTracerVertex {
                position: c.to_array(),
            },
        ]);
        self.indices.extend([base, base + 1, base + 2]);
        self.triangle_infos.push(PathTracerTriangleInfo {
            normal: normalize_or(normal, Vec3::Y).to_array(),
            material,
            albedo: albedo.clamp(Vec3::ZERO, Vec3::splat(64.0)).to_array(),
            shape,
            emission: emission.max(Vec3::ZERO).to_array(),
            _pad0: 0,
            sphere_center: sphere_center.to_array(),
            _pad1: 0,
        });
    }

    fn push_triangle_outward(
        &mut self,
        a: Vec3,
        mut b: Vec3,
        mut c: Vec3,
        object_center: Vec3,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
    ) {
        let centroid = (a + b + c) / 3.0;
        let mut normal = (b - a).cross(c - a);
        if normal.dot(centroid - object_center) < 0.0 {
            std::mem::swap(&mut b, &mut c);
            normal = -normal;
        }
        self.push_triangle(
            a,
            b,
            c,
            normal,
            material,
            albedo,
            emission,
            SHAPE_FLAT,
            Vec3::ZERO,
        );
    }

    fn push_quad(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        normal: Vec3,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
    ) {
        self.push_triangle(
            a,
            b,
            c,
            normal,
            material,
            albedo,
            emission,
            SHAPE_FLAT,
            Vec3::ZERO,
        );
        self.push_triangle(
            a,
            c,
            d,
            normal,
            material,
            albedo,
            emission,
            SHAPE_FLAT,
            Vec3::ZERO,
        );
    }

    fn push_rotated_box(
        &mut self,
        original_center: Vec3,
        half_extent: Vec3,
        y_rotation: f32,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
    ) {
        let rot = Mat3::from_rotation_y(y_rotation);
        let lifted_center = Vec3::new(
            original_center.x,
            rotated_floor_lift(half_extent, rot),
            original_center.z,
        );
        let corner = |sx: f32, sy: f32, sz: f32| {
            lifted_center
                + rot * Vec3::new(sx * half_extent.x, sy * half_extent.y, sz * half_extent.z)
        };

        let nnn = corner(-1.0, -1.0, -1.0);
        let nnp = corner(-1.0, -1.0, 1.0);
        let npn = corner(-1.0, 1.0, -1.0);
        let npp = corner(-1.0, 1.0, 1.0);
        let pnn = corner(1.0, -1.0, -1.0);
        let pnp = corner(1.0, -1.0, 1.0);
        let ppn = corner(1.0, 1.0, -1.0);
        let ppp = corner(1.0, 1.0, 1.0);

        self.push_quad(
            nnp,
            nnn,
            npn,
            npp,
            rot * Vec3::NEG_X,
            material,
            albedo,
            emission,
        );
        self.push_quad(
            pnn,
            pnp,
            ppp,
            ppn,
            rot * Vec3::X,
            material,
            albedo,
            emission,
        );
        self.push_quad(
            nnn,
            pnn,
            ppn,
            npn,
            rot * Vec3::NEG_Z,
            material,
            albedo,
            emission,
        );
        self.push_quad(
            pnp,
            nnp,
            npp,
            ppp,
            rot * Vec3::Z,
            material,
            albedo,
            emission,
        );
        self.push_quad(nnn, nnp, pnp, pnn, Vec3::NEG_Y, material, albedo, emission);
        self.push_quad(npn, ppn, ppp, npp, Vec3::Y, material, albedo, emission);
    }

    fn push_tetrahedron(
        &mut self,
        base_center: Vec3,
        radius: f32,
        height: f32,
        y_rotation: f32,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
    ) {
        let rot = Mat3::from_rotation_y(y_rotation);
        let p0 = base_center + rot * Vec3::new(radius, 0.0, 0.0);
        let p1 = base_center + rot * Vec3::new(-0.5 * radius, 0.0, 0.8660254 * radius);
        let p2 = base_center + rot * Vec3::new(-0.5 * radius, 0.0, -0.8660254 * radius);
        let top = base_center + Vec3::Y * height;
        let object_center = (p0 + p1 + p2 + top) * 0.25;

        self.push_triangle_outward(p0, p1, p2, object_center, material, albedo, emission);
        self.push_triangle_outward(p0, top, p1, object_center, material, albedo, emission);
        self.push_triangle_outward(p1, top, p2, object_center, material, albedo, emission);
        self.push_triangle_outward(p2, top, p0, object_center, material, albedo, emission);
    }

    fn push_uv_sphere(
        &mut self,
        center: Vec3,
        radius: f32,
        slices: u32,
        stacks: u32,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
    ) {
        let slices = slices.max(3);
        let stacks = stacks.max(2);
        for stack in 0..stacks {
            let v0 = stack as f32 / stacks as f32;
            let v1 = (stack + 1) as f32 / stacks as f32;
            let theta0 = std::f32::consts::PI * v0;
            let theta1 = std::f32::consts::PI * v1;
            for slice in 0..slices {
                let u0 = slice as f32 / slices as f32;
                let u1 = (slice + 1) as f32 / slices as f32;
                let p00 = center + sphere_dir(theta0, u0) * radius;
                let p01 = center + sphere_dir(theta0, u1) * radius;
                let p10 = center + sphere_dir(theta1, u0) * radius;
                let p11 = center + sphere_dir(theta1, u1) * radius;

                if stack == 0 {
                    self.push_sphere_triangle(p00, p10, p11, center, material, albedo, emission);
                } else if stack + 1 == stacks {
                    self.push_sphere_triangle(p00, p10, p01, center, material, albedo, emission);
                } else {
                    self.push_sphere_triangle(p00, p10, p11, center, material, albedo, emission);
                    self.push_sphere_triangle(p00, p11, p01, center, material, albedo, emission);
                }
            }
        }
    }

    fn push_sphere_triangle(
        &mut self,
        a: Vec3,
        mut b: Vec3,
        mut c: Vec3,
        center: Vec3,
        material: u32,
        albedo: Vec3,
        emission: Vec3,
    ) {
        let centroid = (a + b + c) / 3.0;
        let mut normal = (b - a).cross(c - a);
        if normal.dot(centroid - center) < 0.0 {
            std::mem::swap(&mut b, &mut c);
            normal = -normal;
        }
        self.push_triangle(
            a,
            b,
            c,
            normal,
            material,
            albedo,
            emission,
            SHAPE_SPHERE,
            center,
        );
    }
}

fn cornell_geometry() -> PathTracerSceneGeometry {
    let mut mesh = MeshBuilder::new();
    let white = Vec3::new(0.78, 0.76, 0.70);
    let red = Vec3::new(0.86, 0.12, 0.08);
    let green = Vec3::new(0.10, 0.48, 0.16);
    let short_box = Vec3::new(0.76, 0.72, 0.64);
    let tall_box = Vec3::new(0.72, 0.72, 0.76);
    let light = Vec3::new(18.0, 14.0, 9.0);

    mesh.push_quad(
        v(-1.0, 0.0, 0.0),
        v(1.0, 0.0, 0.0),
        v(1.0, 0.0, 2.0),
        v(-1.0, 0.0, 2.0),
        Vec3::Y,
        MATERIAL_DIFFUSE,
        white,
        Vec3::ZERO,
    );
    mesh.push_quad(
        v(-1.0, 2.0, 2.0),
        v(1.0, 2.0, 2.0),
        v(1.0, 2.0, 0.0),
        v(-1.0, 2.0, 0.0),
        Vec3::NEG_Y,
        MATERIAL_DIFFUSE,
        white,
        Vec3::ZERO,
    );
    mesh.push_quad(
        v(-1.0, 0.0, 2.0),
        v(1.0, 0.0, 2.0),
        v(1.0, 2.0, 2.0),
        v(-1.0, 2.0, 2.0),
        Vec3::NEG_Z,
        MATERIAL_DIFFUSE,
        white,
        Vec3::ZERO,
    );
    mesh.push_quad(
        v(-1.0, 0.0, 0.0),
        v(-1.0, 0.0, 2.0),
        v(-1.0, 2.0, 2.0),
        v(-1.0, 2.0, 0.0),
        Vec3::X,
        MATERIAL_DIFFUSE,
        red,
        Vec3::ZERO,
    );
    mesh.push_quad(
        v(1.0, 0.0, 2.0),
        v(1.0, 0.0, 0.0),
        v(1.0, 2.0, 0.0),
        v(1.0, 2.0, 2.0),
        Vec3::NEG_X,
        MATERIAL_DIFFUSE,
        green,
        Vec3::ZERO,
    );
    mesh.push_quad(
        v(-0.32, 1.998, 0.78),
        v(0.32, 1.998, 0.78),
        v(0.32, 1.998, 1.36),
        v(-0.32, 1.998, 1.36),
        Vec3::NEG_Y,
        MATERIAL_LIGHT,
        Vec3::splat(1.0),
        light,
    );

    mesh.push_rotated_box(
        Vec3::new(-0.26, 0.36, 0.77),
        Vec3::new(0.21, 0.36, 0.21),
        45.0_f32.to_radians(),
        MATERIAL_DIFFUSE,
        short_box,
        Vec3::ZERO,
    );
    mesh.push_rotated_box(
        Vec3::new(0.445, 0.59, 1.06),
        Vec3::new(0.225, 0.59, 0.24),
        45.0_f32.to_radians(),
        MATERIAL_DIFFUSE,
        tall_box,
        Vec3::ZERO,
    );

    mesh.finish(SCENE_CORNELL)
}

fn outdoor_geometry() -> PathTracerSceneGeometry {
    let mut mesh = MeshBuilder::new();
    let mut rng = Rng64::new(outdoor_seed());

    mesh.push_quad(
        v(-28.0, 0.0, -16.0),
        v(28.0, 0.0, -16.0),
        v(28.0, 0.0, 30.0),
        v(-28.0, 0.0, 30.0),
        Vec3::Y,
        MATERIAL_DIFFUSE,
        Vec3::new(0.48, 0.56, 0.38),
        Vec3::ZERO,
    );

    const COLUMNS: u32 = 7;
    const OBJECT_COUNT: u32 = 30;
    const CELL_SIZE: f32 = 2.35;
    for index in 0..OBJECT_COUNT {
        let col = index % COLUMNS;
        let row = index / COLUMNS;
        let base_x = (col as f32 - (COLUMNS as f32 - 1.0) * 0.5) * CELL_SIZE;
        let base_z = row as f32 * CELL_SIZE + 0.4;
        let x = base_x + rng.range_f32(-0.62, 0.62);
        let z = base_z + rng.range_f32(-0.58, 0.58);
        let base_center = Vec3::new(x, 0.0, z);
        let rotation = rng.range_f32(0.0, std::f32::consts::TAU);
        let (material, albedo, emission) = random_material(&mut rng);

        match rng.next_u32() % 3 {
            0 => {
                let half_extent = Vec3::new(
                    rng.range_f32(0.22, 0.56),
                    rng.range_f32(0.26, 0.95),
                    rng.range_f32(0.22, 0.56),
                );
                mesh.push_rotated_box(
                    base_center + Vec3::Y * half_extent.y,
                    half_extent,
                    rotation,
                    material,
                    albedo,
                    emission,
                );
            }
            1 => {
                let radius = rng.range_f32(0.38, 0.72);
                let height = rng.range_f32(0.82, 1.55);
                mesh.push_tetrahedron(
                    base_center,
                    radius,
                    height,
                    rotation,
                    material,
                    albedo,
                    emission,
                );
            }
            _ => {
                let radius = rng.range_f32(0.30, 0.68);
                mesh.push_uv_sphere(
                    base_center + Vec3::Y * radius,
                    radius,
                    32,
                    16,
                    material,
                    albedo,
                    emission,
                );
            }
        }
    }

    mesh.finish(SCENE_OUTDOOR)
}

fn random_material(rng: &mut Rng64) -> (u32, Vec3, Vec3) {
    let roll = rng.next_f32();
    if roll < 0.62 {
        let color = random_color(rng, 0.18, 0.92);
        (MATERIAL_DIFFUSE, color, Vec3::ZERO)
    } else if roll < 0.84 {
        let tint = Vec3::splat(0.55) + random_color(rng, 0.08, 0.45);
        (MATERIAL_GLASS, tint.min(Vec3::splat(0.98)), Vec3::ZERO)
    } else {
        let color = random_color(rng, 0.35, 1.0);
        let intensity = rng.range_f32(3.0, 10.0);
        (MATERIAL_LIGHT, Vec3::splat(1.0), color * intensity)
    }
}

fn random_color(rng: &mut Rng64, min: f32, max: f32) -> Vec3 {
    Vec3::new(
        rng.range_f32(min, max),
        rng.range_f32(min, max),
        rng.range_f32(min, max),
    )
}

fn sphere_dir(theta: f32, u: f32) -> Vec3 {
    let phi = std::f32::consts::TAU * u;
    Vec3::new(
        theta.sin() * phi.cos(),
        theta.cos(),
        theta.sin() * phi.sin(),
    )
}

fn rotated_floor_lift(half_extent: Vec3, rot: Mat3) -> f32 {
    let mut min_y = f32::MAX;
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                min_y = min_y.min(
                    (rot * Vec3::new(sx * half_extent.x, sy * half_extent.y, sz * half_extent.z)).y,
                );
            }
        }
    }
    -min_y
}

fn normalize_or(v: Vec3, fallback: Vec3) -> Vec3 {
    if v.length_squared() > 1e-8 {
        v.normalize()
    } else {
        fallback
    }
}

fn outdoor_seed() -> u64 {
    let time_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x8b5a_d4c3_92e7_1531);
    time_seed ^ 0xa24b_aed4_963e_e407
}

struct Rng64 {
    state: u64,
}

impl Rng64 {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        (x >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() & 0x00ff_ffff) as f32 / 16_777_216.0
    }

    fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
