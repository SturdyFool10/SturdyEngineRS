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

        let ground = scene.add_mesh(Mesh::plane(engine, 18.0, 18.0)?, MeshProgram::lit(engine)?);
        scene.set_unified_material(
            ground,
            UnifiedMaterial::pbr_metallic_roughness("shadow_demo_matte_plane")
                .base_color(MaterialExpr::Constant([0.34, 0.37, 0.33, 1.0]))
                .roughness(MaterialExpr::Constant(0.94))
                .metallic(MaterialExpr::Constant(0.0))
                .build(),
        );
        scene.add_object_at(ground, Mat4::IDENTITY, ObjectKind::Static);

        let cylinder_mesh = scene.add_mesh(
            Mesh::cylinder(engine, 1.0, 1.0, 64)?,
            MeshProgram::lit(engine)?,
        );
        scene.set_unified_material(
            cylinder_mesh,
            UnifiedMaterial::pbr_metallic_roughness("shadow_demo_cylinders")
                .base_color(MaterialExpr::Constant([0.82, 0.46, 0.23, 1.0]))
                .roughness(MaterialExpr::Constant(0.58))
                .metallic(MaterialExpr::Constant(0.0))
                .build(),
        );

        for cylinder in [
            CylinderPlacement {
                position: Vec3::new(-2.25, 0.0, 0.6),
                radius: 0.62,
                height: 2.4,
                yaw: 0.0,
            },
            CylinderPlacement {
                position: Vec3::new(0.25, 0.0, -1.15),
                radius: 0.82,
                height: 1.35,
                yaw: 0.45,
            },
            CylinderPlacement {
                position: Vec3::new(2.35, 0.0, 0.95),
                radius: 0.48,
                height: 3.15,
                yaw: -0.28,
            },
        ] {
            scene.add_object_at(
                cylinder_mesh,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(cylinder.radius, cylinder.height, cylinder.radius),
                    Quat::from_rotation_y(cylinder.yaw),
                    cylinder.position + Vec3::Y * (cylinder.height * 0.5),
                ),
                ObjectKind::Static,
            );
        }

        scene.directional_light.direction = Vec3::new(-0.45, -0.62, -0.64).normalize();
        scene.directional_light.color = Vec3::new(1.0, 0.93, 0.78);
        scene.directional_light.intensity = 2.7;
        scene.directional_light.ambient = Vec3::splat(0.035);

        let mut deferred = DeferredPass::new(engine)?;
        deferred.sky = SkyConfig {
            turbidity: 3.6,
            exposure: 1.05,
            sun_size_deg: 1.35,
            ..Default::default()
        };
        deferred.csm_config_mut().cascade_count = 3;
        deferred.csm_config_mut().resolution = 1024;
        deferred.csm_config_mut().depth_bias = 0.0025;
        deferred.csm_config_mut().lambda = 0.80;
        deferred.csm_config_mut().blend_range = 0.22;
        deferred.csm_config_mut().pcss = false;
        deferred.csm_config_mut().pcss_light_size = 3.5;

        let mut camera = OrbitCamera::new(Vec3::new(0.0, 1.15, 0.0), 8.5);
        camera.yaw = 0.72;
        camera.pitch = 0.36;

        Ok(Self {
            scene,
            deferred,
            camera,
            time: 0.0,
        })
    }

    pub(crate) fn advance(&mut self, elapsed: f32) {
        self.time = elapsed;

        let azimuth = 3.85 + elapsed * 0.035;
        let elevation = 0.54 + (elapsed * 0.08).sin() * 0.05;
        self.scene.directional_light.direction = Vec3::new(
            azimuth.cos() * elevation.cos(),
            -elevation.sin(),
            azimuth.sin() * elevation.cos(),
        )
        .normalize();
        self.scene.directional_light.color = Vec3::new(1.0, 0.92, 0.76);
        self.scene.directional_light.intensity = 2.45 + elevation.sin() * 0.45;
        self.deferred.sky.turbidity = 3.4 + (elapsed * 0.05).sin() * 0.35;
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

struct CylinderPlacement {
    position: Vec3,
    radius: f32,
    height: f32,
    yaw: f32,
}
