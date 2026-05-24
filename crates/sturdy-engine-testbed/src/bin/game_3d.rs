// 3D game sample — scene explorer with deferred PBR rendering.
//
// Demonstrates the full deferred pipeline using only the default game shell:
//   - DeferredPass with 4-cascade CSM shadows and BVH point lights
//   - UnifiedMaterial expressions (procedural emissive pulse, typed constants)
//   - Orbit camera with mouse drag and keyboard orbit/zoom
//   - 8 animated point lights orbiting the scene
//   - Procedural geometry only — no asset files required
//
// Run:  cargo run --bin game_3d
//
// Controls:  WASD = orbit camera  Q/E = zoom  R = reset camera
//            Hold LMB + drag = mouse orbit
//
// Roadmap: Track 1 — game loop shell (3D game sample).

use std::f32::consts::TAU;
use std::time::Duration;

use glam::{Mat4, Quat, Vec3};
use sturdy_engine::{
    DebugOverlay, DebugOverlayRenderer, DeferredPass, Engine, FixedUpdateContext, GameApp,
    GameConfig, GameContext, InputHub, KeyModifier, Keybind, MaterialExpr, Mesh, MeshProgram,
    ObjectKind, OrbitCamera, PointLight, RectLight, Scene, ShaderProgram, ShellFrame, SkyConfig,
    Surface, SurfaceImage, UnifiedMaterial, WindowConfig,
};

struct Game3d {
    engine: Engine,
    scene: Scene,
    deferred: DeferredPass,
    passthrough: ShaderProgram, // for blitting deferred output to swapchain
    overlay: DebugOverlayRenderer,
    camera: OrbitCamera,
    time: f32,
    orbit_n: usize,
}

impl GameApp for Game3d {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, _surface: &Surface) -> sturdy_engine::Result<Self> {
        let mut scene = Scene::new();

        // ── Geometry ──────────────────────────────────────────────────────────

        // Ground plane.
        let g = scene.add_mesh(Mesh::plane(engine, 20.0, 20.0)?, MeshProgram::lit(engine)?);
        scene.set_unified_material(
            g,
            UnifiedMaterial::pbr_metallic_roughness("ground")
                .base_color(MaterialExpr::Constant([0.22f32, 0.20, 0.18, 1.0]))
                .roughness(MaterialExpr::Constant(0.85f32))
                .metallic(MaterialExpr::Constant(0.0f32))
                .build(),
        );
        scene.add_object_at(g, Mat4::IDENTITY, ObjectKind::Static);

        // Pedestal.
        let p = scene.add_mesh(Mesh::cube(engine, 1.0)?, MeshProgram::lit(engine)?);
        scene.set_unified_material(
            p,
            UnifiedMaterial::pbr_metallic_roughness("pedestal")
                .base_color(MaterialExpr::Constant([0.38f32, 0.38, 0.42, 1.0]))
                .roughness(MaterialExpr::Constant(0.25f32))
                .metallic(MaterialExpr::Constant(0.85f32))
                .build(),
        );
        scene.add_object_at(
            p,
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.2, 0.3, 1.2),
                Quat::IDENTITY,
                Vec3::new(0.0, 0.15, 0.0),
            ),
            ObjectKind::Static,
        );

        // Sphere on pedestal — procedural pulsing emissive.
        let s = scene.add_mesh(
            Mesh::uv_sphere(engine, 0.45, 32, 32)?,
            MeshProgram::lit(engine)?,
        );
        scene.set_unified_material(
            s,
            UnifiedMaterial::pbr_metallic_roughness("sphere")
                .base_color(MaterialExpr::Constant([0.9f32, 0.7, 0.2, 1.0]))
                .roughness(MaterialExpr::Constant(0.04f32))
                .metallic(MaterialExpr::Constant(1.0f32))
                .emissive(MaterialExpr::procedural(
                    "float3(1.0, 0.4, 0.05) * (sin(cam.time * 2.2) * 0.45 + 0.6) * 2.5",
                ))
                .build(),
        );
        scene.add_object_at(
            s,
            Mat4::from_translation(Vec3::new(0.0, 0.75, 0.0)),
            ObjectKind::Static,
        );

        // Ring of 8 columns.
        for i in 0..8usize {
            let angle = i as f32 / 8.0 * TAU;
            let r = 4.2f32;
            let h = 0.5 + (i as f32 * 0.6).cos().abs() * 0.9;
            let pos = Vec3::new(r * angle.cos(), h * 0.5, r * angle.sin());
            let c = scene.add_mesh(Mesh::cube(engine, 1.0)?, MeshProgram::lit(engine)?);
            let (cr, cg, cb) = hsv(i as f32 / 8.0, 0.35, 0.55);
            scene.set_unified_material(
                c,
                UnifiedMaterial::pbr_metallic_roughness(format!("col_{i}"))
                    .base_color(MaterialExpr::Constant([cr, cg, cb, 1.0]))
                    .roughness(MaterialExpr::Constant(0.6f32 + i as f32 * 0.04))
                    .metallic(MaterialExpr::Constant(0.1f32))
                    .build(),
            );
            scene.add_object_at(
                c,
                Mat4::from_scale_rotation_translation(Vec3::new(0.4, h, 0.4), Quat::IDENTITY, pos),
                ObjectKind::Static,
            );
        }

        // ── Lighting ──────────────────────────────────────────────────────────
        scene.directional_light.direction = Vec3::new(0.4, -0.8, -0.5).normalize();
        scene.directional_light.color = Vec3::new(1.0, 0.92, 0.82);
        scene.directional_light.intensity = 1.1;
        scene.directional_light.ambient = Vec3::splat(0.025);

        let orbit_n = 8usize;
        for i in 0..orbit_n {
            let (r, g, b) = hsv(i as f32 / orbit_n as f32, 0.9, 1.0);
            scene.point_lights.push(PointLight {
                position: Vec3::ZERO,
                color: Vec3::new(r, g, b),
                intensity: 80.0,
                range: 7.0,
                ..Default::default()
            });
        }

        // Warm overhead rect light (like a studio light panel).
        scene.rect_lights.push(RectLight {
            position: Vec3::new(0.0, 6.0, 0.0),
            right: Vec3::X,
            up: Vec3::Z,
            half_extents: [2.5, 2.5],
            color: Vec3::new(1.0, 0.95, 0.88),
            intensity: 250.0,
            range: 10.0,
            two_sided: false,
            ..Default::default()
        });

        let mut deferred = DeferredPass::new(engine)?;
        deferred.sky = SkyConfig {
            turbidity: 3.0,
            exposure: 1.2,
            ..Default::default()
        };
        let passthrough = ShaderProgram::passthrough(engine)?;
        let camera = OrbitCamera::new(Vec3::new(0.0, 0.8, 0.0), 8.0);

        Ok(Self {
            engine: engine.clone(),
            scene,
            deferred,
            passthrough,
            overlay: DebugOverlayRenderer::new(engine)?,
            camera,
            time: 0.0,
            orbit_n,
        })
    }

    fn configure_input(&mut self, hub: &mut InputHub) {
        let m = hub.actions_mut();
        let no_mod = std::iter::empty::<KeyModifier>();
        m.bind("left", Keybind::new(no_mod.clone(), Some("KeyA".into())));
        m.bind("right", Keybind::new(no_mod.clone(), Some("KeyD".into())));
        m.bind("up", Keybind::new(no_mod.clone(), Some("KeyW".into())));
        m.bind("down", Keybind::new(no_mod.clone(), Some("KeyS".into())));
        m.bind("zin", Keybind::new(no_mod.clone(), Some("KeyE".into())));
        m.bind("zout", Keybind::new(no_mod.clone(), Some("KeyQ".into())));
        m.bind("reset", Keybind::new(no_mod.clone(), Some("KeyR".into())));
    }

    fn fixed_update(&mut self, ctx: &FixedUpdateContext<'_>) -> sturdy_engine::Result<()> {
        let dt = ctx.fixed_step_secs();
        self.time += dt;
        let t = self.time;
        let a = ctx.input.actions();

        let spd = 1.2f32;
        let zsz = 3.0f32;
        if a.is_held("left") {
            self.camera.yaw -= spd * dt;
        }
        if a.is_held("right") {
            self.camera.yaw += spd * dt;
        }
        if a.is_held("up") {
            self.camera.pitch = (self.camera.pitch + spd * 0.5 * dt).min(1.5);
        }
        if a.is_held("down") {
            self.camera.pitch = (self.camera.pitch - spd * 0.5 * dt).max(-0.1);
        }
        if a.is_held("zin") {
            self.camera.distance = (self.camera.distance - zsz * dt).max(2.0);
        }
        if a.is_held("zout") {
            self.camera.distance = (self.camera.distance + zsz * dt).min(22.0);
        }
        if a.just_pressed("reset") {
            self.camera = OrbitCamera::new(Vec3::new(0.0, 0.8, 0.0), 8.0);
        }

        if ctx.input.is_mouse_button_pressed(0) {
            let d = ctx.input.mouse_delta();
            self.camera.on_drag(d.x, d.y, 0.005);
        }

        for i in 0..self.orbit_n {
            let phase = i as f32 / self.orbit_n as f32 * TAU;
            let orbit_r = 3.5 + (t * 0.35 + phase).sin() * 0.6;
            let h = 0.9 + (t * 0.28 + phase * 1.3).sin() * 0.55;
            if let Some(pl) = self.scene.point_lights.get_mut(i) {
                let angle = phase + t * 0.55;
                pl.position = Vec3::new(orbit_r * angle.cos(), h, orbit_r * angle.sin());
            }
        }
        Ok(())
    }

    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
        ctx: &GameContext<'_>,
    ) -> sturdy_engine::Result<()> {
        let ext = surface_image.desc().extent;
        let aspect = ext.width as f32 / ext.height.max(1) as f32;
        let t =
            self.time + ctx.fixed_alpha * ctx.fixed_step.map(|s| s.as_secs_f32()).unwrap_or(0.0);

        let view = self.camera.view_matrix();
        let proj = self.camera.projection_matrix(aspect);
        let rf = frame.inner();

        self.scene.prepare(&self.engine)?;

        // Allocate scene HDR target, run deferred pass into it, present.
        let scene_target = frame.default_hdr_scene_target("scene_color", 1)?;
        let scene_color = frame.resolve_default_hdr_scene_target(&scene_target, "scene_color")?;

        self.deferred.draw(
            &mut self.scene,
            view,
            proj,
            &scene_color,
            rf,
            &self.engine,
            t,
        )?;

        // Blit HDR scene colour → swapchain (passthrough, no tone-mapping).
        let swapchain = rf.swapchain_image(surface_image)?;
        swapchain.blit_from(&scene_color, &self.passthrough)?;

        let lines = [
            "3D Sample — Orbit Scene".to_string(),
            format!("WASD orbit  Q/E zoom  R reset  LMB drag | t={t:.1}s"),
            format!("{} orbiting lights + 1 rect light", self.orbit_n),
        ];
        let mut overlay = DebugOverlay::new();
        overlay.rounded_rectangle_outline_screen(
            ext.width,
            ext.height,
            [16.0, 16.0],
            [650.0, 120.0],
            10.0,
            3.0,
            [1.0, 1.0, 1.0, 0.9],
        );
        for (index, line) in lines.iter().enumerate() {
            overlay.add_screen_text(line.as_str(), 24.0, 24.0 + index as f32 * 26.0);
        }
        frame.run_camera_locked_pass("game_3d_hud", &swapchain, |render_frame, target| {
            self.overlay
                .draw(render_frame, target, ext.width, ext.height, &overlay)
        })?;

        rf.present_image(&swapchain)?;
        Ok(())
    }
}

fn hsv(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h6 = h * 6.0;
    let i = h6.floor() as u32 % 6;
    let f = h6.fract();
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn main() {
    sturdy_engine::run_game::<Game3d>(
        GameConfig::new(
            WindowConfig::new("Orbit Scene — SturdyEngine 3D Sample", 1280, 720)
                .with_resizable(true),
        )
        .with_fixed_step(Duration::from_secs_f64(1.0 / 60.0)),
    );
}
