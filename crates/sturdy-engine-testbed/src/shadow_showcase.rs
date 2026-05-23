use glam::{Mat4, Quat, Vec3};
use sturdy_engine::{
    DeferredPass, Engine, GraphImage, MaterialExpr, Mesh, MeshProgram, ObjectKind, OrbitCamera,
    RenderFrame, Result as EngineResult, Scene, SkyConfig, UnifiedMaterial,
};

pub(crate) struct ShadowShowcase {
    scene: Scene,
    deferred: DeferredPass,
    camera: OrbitCamera,
    time: f32,
}

impl ShadowShowcase {
    pub(crate) fn new(engine: &Engine) -> EngineResult<Self> {
        let mut scene = Scene::new();

        // XZ ground plane at y=0. `Mesh::plane` extends along X and Z by design.
        let ground = scene.add_mesh(Mesh::plane(engine, 22.0, 22.0)?, MeshProgram::lit(engine)?);
        scene.set_unified_material(
            ground,
            UnifiedMaterial::pbr_metallic_roughness("csm_demo_xz_plane")
                .base_color(MaterialExpr::Constant([0.36, 0.39, 0.35, 1.0]))
                .roughness(MaterialExpr::Constant(0.92))
                .metallic(MaterialExpr::Constant(0.0))
                .build(),
        );
        scene.add_object_at(ground, Mat4::IDENTITY, ObjectKind::Static);

        let cube_mesh = scene.add_mesh(Mesh::cube(engine, 1.0)?, MeshProgram::lit(engine)?);
        scene.set_unified_material(
            cube_mesh,
            UnifiedMaterial::pbr_metallic_roughness("csm_demo_shadow_cubes")
                .base_color(MaterialExpr::Constant([0.82, 0.47, 0.24, 1.0]))
                .roughness(MaterialExpr::Constant(0.62))
                .metallic(MaterialExpr::Constant(0.0))
                .build(),
        );

        for cube in [
            CubePlacement {
                center_xz: Vec3::new(-2.0, 0.0, 0.8),
                scale: Vec3::splat(0.95),
                yaw: 0.25,
            },
            CubePlacement {
                center_xz: Vec3::new(0.15, 0.0, -1.05),
                scale: Vec3::new(1.05, 0.75, 0.9),
                yaw: -0.42,
            },
            CubePlacement {
                center_xz: Vec3::new(2.0, 0.0, 0.85),
                scale: Vec3::new(0.8, 1.35, 0.8),
                yaw: 0.72,
            },
        ] {
            scene.add_object_at(
                cube_mesh,
                Mat4::from_scale_rotation_translation(
                    cube.scale,
                    Quat::from_rotation_y(cube.yaw),
                    cube.center_xz + Vec3::Y * (cube.scale.y * 0.5),
                ),
                ObjectKind::Static,
            );
        }

        let sun_toward = Vec3::new(0.45, 0.62, 0.64).normalize();
        scene.directional_light.direction = -sun_toward;
        scene.directional_light.color = Vec3::new(1.0, 0.93, 0.78);
        scene.directional_light.intensity = 3.0;
        scene.directional_light.ambient = Vec3::splat(0.015);

        let mut deferred = DeferredPass::new(engine)?;
        deferred.sky = SkyConfig {
            enabled: true,
            turbidity: 3.4,
            exposure: 1.08,
            sun_size_deg: 1.25,
            ..Default::default()
        };
        deferred.csm_config_mut().cascade_count = 3;
        deferred.csm_config_mut().resolution = 1024;
        deferred.csm_config_mut().depth_bias = 0.0025;
        deferred.csm_config_mut().lambda = 0.80;
        deferred.csm_config_mut().blend_range = 0.22;
        deferred.csm_config_mut().pcss = false;
        deferred.csm_config_mut().pcss_light_size = 3.5;

        // OrbitCamera builds its view with world up = (0, 1, 0).
        let mut camera = OrbitCamera::new(Vec3::new(0.0, 0.65, 0.0), 15.0);
        camera.yaw = 0.78;
        camera.pitch = 0.68;
        camera.fov_y = 45.0_f32.to_radians();

        Ok(Self {
            scene,
            deferred,
            camera,
            time: 0.0,
        })
    }

    pub(crate) fn advance(&mut self, elapsed: f32) {
        self.time = elapsed;

        let azimuth = 0.78 + elapsed * 0.025;
        let elevation = 0.58 + (elapsed * 0.06).sin() * 0.04;
        let sun_toward = Vec3::new(
            azimuth.cos() * elevation.cos(),
            elevation.sin(),
            azimuth.sin() * elevation.cos(),
        )
        .normalize();
        self.scene.directional_light.direction = -sun_toward;
        self.scene.directional_light.color = Vec3::new(1.0, 0.92, 0.76);
        self.scene.directional_light.intensity = 2.7 + elevation.sin() * 0.35;
        self.deferred.sky.turbidity = 3.4 + (elapsed * 0.05).sin() * 0.25;
    }

    pub(crate) fn draw(
        &mut self,
        frame: &RenderFrame,
        output: &GraphImage,
        engine: &Engine,
        aspect: f32,
    ) -> EngineResult<()> {
        self.scene.prepare(engine)?;
        let view = self.camera.view_matrix();
        let proj = self.camera.projection_matrix(aspect);
        self.deferred.draw(
            &mut self.scene,
            view,
            proj,
            output,
            frame,
            engine,
            self.time,
        )
    }

    pub(crate) fn on_key(&mut self, key: &str) {
        const YAW_STEP: f32 = 0.12;
        const PITCH_STEP: f32 = 0.08;
        const ZOOM_STEP: f32 = 0.5;
        match key {
            "ArrowLeft" => self.camera.yaw -= YAW_STEP,
            "ArrowRight" => self.camera.yaw += YAW_STEP,
            "ArrowUp" => self.camera.pitch = (self.camera.pitch + PITCH_STEP).min(1.5),
            "ArrowDown" => self.camera.pitch = (self.camera.pitch - PITCH_STEP).max(-0.1),
            "PageUp" => self.camera.distance = (self.camera.distance - ZOOM_STEP).max(3.0),
            "PageDown" => self.camera.distance = (self.camera.distance + ZOOM_STEP).min(16.0),
            _ => {}
        }
    }
}

struct CubePlacement {
    center_xz: Vec3,
    scale: Vec3,
    yaw: f32,
}
