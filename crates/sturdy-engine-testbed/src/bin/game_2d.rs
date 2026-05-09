// 2D game sample — "Dodge" — asteroid avoidance arcade game.
//
// A complete, playable 2D game using only the default game shell:
//   - Player ship (quad) controlled by WASD / Arrow keys
//   - Asteroids spawn at arena edges and drift inward
//   - Axis-aligned bounding box collision detection
//   - Score increments each second survived; three lives
//   - Text HUD overlay; colour-coded asteroids
//   - Procedural geometry only — no assets required
//
// Run:  cargo run --bin game_2d
//
// Controls:  WASD / Arrow keys = move  R or Space = restart after game over
//
// Roadmap: Track 1 — game loop shell (2D game sample).

use std::time::Duration;

use glam::{Mat4, Quat, Vec2, Vec3};
use sturdy_engine::{
    Engine, FixedUpdateContext, GameApp, GameConfig, GameContext, InputHub, Keybind,
    KeyModifier, Mesh, MeshId, MeshProgram, ObjectId, ObjectKind, Scene, ShellFrame,
    Surface, SurfaceImage, Vertex2d, WindowConfig,
};

// ── Constants ─────────────────────────────────────────────────────────────────

const W: f32 = 18.0;  // arena half-width in world units
const H: f32 = 11.0;  // arena half-height

const PLAYER_SPD:   f32 = 9.0;
const PLAYER_SZ:    f32 = 0.55;
const ASTEROID_SPD: f32 = 2.8;
const ASTEROID_SZ:  f32 = 0.72;
const SPAWN_BASE:   f32 = 1.1;  // initial spawn interval in seconds
const MAX_ROCKS:    usize = 48;

// ── Asteroid ──────────────────────────────────────────────────────────────────

struct Rock {
    pos:    Vec2,
    vel:    Vec2,
    size:   f32,
    color:  [f32; 4],
    alive:  bool,
    obj:    ObjectId,
}

// ── Game ──────────────────────────────────────────────────────────────────────

struct Dodge {
    engine:     Engine,
    scene:      Scene,
    quad_id:    MeshId,
    player_obj: ObjectId,
    player_pos: Vec2,
    lives:      u32,
    invincible: f32,  // post-hit grace period countdown
    rocks:      Vec<Rock>,
    spawn_t:    f32,  // time until next spawn
    rng:        u64,  // xorshift64
    score:      u32,
    score_t:    f32,
    game_over:  bool,
    restart_t:  f32,
}

impl Dodge {
    fn rnd(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f32 / (1u64 << 53) as f32
    }

    fn rnd_range(&mut self, lo: f32, hi: f32) -> f32 { lo + self.rnd() * (hi - lo) }

    fn spawn_rock(&mut self) {
        if self.rocks.iter().filter(|r| r.alive).count() >= MAX_ROCKS { return; }

        let edge = (self.rnd() * 4.0) as u32;
        let (x, y) = match edge {
            0 => (self.rnd_range(-W, W),  H + 1.5),
            1 => (self.rnd_range(-W, W), -H - 1.5),
            2 => (-W - 1.5, self.rnd_range(-H, H)),
            _ => ( W + 1.5, self.rnd_range(-H, H)),
        };
        let pos = Vec2::new(x, y);
        let target = Vec2::new(self.rnd_range(-W * 0.4, W * 0.4),
                               self.rnd_range(-H * 0.4, H * 0.4));
        let dir   = (target - pos).normalize_or_zero();
        let speed = ASTEROID_SPD * self.rnd_range(0.6, 1.6);
        let hue   = self.rnd();
        let size  = ASTEROID_SZ * self.rnd_range(0.55, 1.45);

        let (r, g, b) = hsv(hue, 0.7, 0.9);
        let color = [r, g, b, 1.0];

        let obj = self.scene.add_object(self.quad_id, ObjectKind::Dynamic);

        if let Some(slot) = self.rocks.iter_mut().find(|r| !r.alive) {
            slot.pos   = pos;
            slot.vel   = dir * speed;
            slot.size  = size;
            slot.color = color;
            slot.alive = true;
            slot.obj   = obj;
        } else {
            self.rocks.push(Rock { pos, vel: dir * speed, size, color, alive: true, obj });
        }
    }

    fn overlap(a: Vec2, sa: f32, b: Vec2, sb: f32) -> bool {
        let d = (a - b).abs();
        let r = (sa + sb) * 0.5;
        d.x < r && d.y < r
    }

    fn restart(&mut self) {
        self.player_pos = Vec2::ZERO;
        self.lives      = 3;
        self.score      = 0;
        self.score_t    = 0.0;
        self.spawn_t    = 0.0;
        self.invincible = 0.0;
        self.game_over  = false;
        self.restart_t  = 0.0;
        for r in &mut self.rocks { r.alive = false; }
    }
}

impl GameApp for Dodge {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, _surface: &Surface) -> sturdy_engine::Result<Self> {
        let mut scene = Scene::new();

        // One shared quad mesh — every sprite is the same geometry, scaled per-instance.
        let verts: &[Vertex2d] = &[
            Vertex2d { position: [-0.5, -0.5], uv: [0., 1.], color: [1.; 4] },
            Vertex2d { position: [ 0.5, -0.5], uv: [1., 1.], color: [1.; 4] },
            Vertex2d { position: [ 0.5,  0.5], uv: [1., 0.], color: [1.; 4] },
            Vertex2d { position: [-0.5,  0.5], uv: [0., 0.], color: [1.; 4] },
        ];
        let idx: &[u32] = &[0, 1, 2, 0, 2, 3];
        let mesh    = Mesh::indexed_2d(engine, verts, idx)?;
        let program = MeshProgram::unlit(engine)?;
        let quad_id = scene.add_mesh(mesh, program);

        let player_obj = scene.add_object(quad_id, ObjectKind::Dynamic);

        Ok(Self {
            engine: engine.clone(),
            scene,
            quad_id,
            player_obj,
            player_pos: Vec2::ZERO,
            lives:      3,
            invincible: 0.0,
            rocks:      Vec::new(),
            spawn_t:    0.0,
            rng:        0xDEADBEEFCAFEBABE,
            score:      0,
            score_t:    0.0,
            game_over:  false,
            restart_t:  0.0,
        })
    }

    fn configure_input(&mut self, hub: &mut InputHub) {
        let m = hub.actions_mut();
        let nm = || std::iter::empty::<KeyModifier>();
        m.bind("left",    Keybind::new(nm(), Some("KeyA".into())));
        m.bind("right",   Keybind::new(nm(), Some("KeyD".into())));
        m.bind("up",      Keybind::new(nm(), Some("KeyW".into())));
        m.bind("down",    Keybind::new(nm(), Some("KeyS".into())));
        m.bind("left",    Keybind::new(nm(), Some("ArrowLeft".into())));
        m.bind("right",   Keybind::new(nm(), Some("ArrowRight".into())));
        m.bind("up",      Keybind::new(nm(), Some("ArrowUp".into())));
        m.bind("down",    Keybind::new(nm(), Some("ArrowDown".into())));
        m.bind("restart", Keybind::new(nm(), Some("KeyR".into())));
        m.bind("restart", Keybind::new(nm(), Some("Space".into())));
    }

    fn fixed_update(&mut self, ctx: &FixedUpdateContext<'_>) -> sturdy_engine::Result<()> {
        let dt = ctx.fixed_step_secs();
        let a  = ctx.input.actions();

        if self.game_over {
            self.restart_t += dt;
            if self.restart_t > 1.5 && a.just_pressed("restart") { self.restart(); }
            return Ok(());
        }

        // ── Player ────────────────────────────────────────────────────────────
        let mut mv = Vec2::ZERO;
        if a.is_held("left")  { mv.x -= 1.0; }
        if a.is_held("right") { mv.x += 1.0; }
        if a.is_held("up")    { mv.y += 1.0; }
        if a.is_held("down")  { mv.y -= 1.0; }
        if mv.length_squared() > 0.0 {
            self.player_pos += mv.normalize() * PLAYER_SPD * dt;
        }
        self.player_pos.x = self.player_pos.x.clamp(-W + PLAYER_SZ, W - PLAYER_SZ);
        self.player_pos.y = self.player_pos.y.clamp(-H + PLAYER_SZ, H - PLAYER_SZ);

        // ── Score ─────────────────────────────────────────────────────────────
        self.score_t += dt;
        if self.score_t >= 1.0 { self.score += 1; self.score_t -= 1.0; }

        // ── Spawn ─────────────────────────────────────────────────────────────
        self.spawn_t -= dt;
        if self.spawn_t <= 0.0 {
            self.spawn_rock();
            self.spawn_t = (SPAWN_BASE - self.score as f32 * 0.018).max(0.25);
        }

        // ── Move rocks ────────────────────────────────────────────────────────
        for r in &mut self.rocks {
            if !r.alive { continue; }
            r.pos += r.vel * dt;
            if r.pos.x.abs() > W + 4.0 || r.pos.y.abs() > H + 4.0 { r.alive = false; }
        }

        // ── Collision ─────────────────────────────────────────────────────────
        self.invincible = (self.invincible - dt).max(0.0);
        if self.invincible <= 0.0 {
            for r in &mut self.rocks {
                if !r.alive { continue; }
                if Self::overlap(self.player_pos, PLAYER_SZ, r.pos, r.size) {
                    r.alive       = false;
                    self.lives    = self.lives.saturating_sub(1);
                    self.invincible = 2.0;
                    if self.lives == 0 { self.game_over = true; }
                    break;
                }
            }
        }

        Ok(())
    }

    fn render(
        &mut self,
        frame: &mut ShellFrame<'_>,
        surface_image: &SurfaceImage,
        _ctx: &GameContext<'_>,
    ) -> sturdy_engine::Result<()> {
        let ext    = surface_image.desc().extent;
        let aspect = ext.width as f32 / ext.height.max(1) as f32;

        // Orthographic projection matching the world coordinate system.
        let half_h = H;
        let half_w = half_h * aspect;
        let proj   = Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, -1.0, 1.0);
        let view   = Mat4::IDENTITY;

        // ── Set instance transforms ───────────────────────────────────────────
        let blink = self.invincible > 0.0 && (self.invincible * 8.0) as u32 % 2 == 0;
        let p = self.player_pos;
        let ps = if blink { 0.001 } else { PLAYER_SZ };
        self.scene.set_transform(
            self.player_obj,
            Mat4::from_scale_rotation_translation(
                Vec3::new(ps, ps, 1.0), Quat::IDENTITY, Vec3::new(p.x, p.y, 0.0)),
        );
        for r in &self.rocks {
            if !r.alive { continue; }
            self.scene.set_transform(r.obj,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(r.size, r.size, 1.0),
                    Quat::IDENTITY,
                    Vec3::new(r.pos.x, r.pos.y, 0.0)));
        }

        self.scene.prepare(&self.engine)?;

        let rf        = frame.inner();
        let swapchain = rf.swapchain_image(surface_image)?;

        // Draw all quads directly to the swapchain.
        self.scene.draw(view, proj, &swapchain, rf, &self.engine)?;

        rf.present_image(&swapchain)?;

        // ── HUD ───────────────────────────────────────────────────────────────
        let lines: Vec<String> = if self.game_over {
            vec![
                "DODGE — 2D Game Sample".into(),
                "GAME  OVER".into(),
                format!("Final score: {}", self.score),
                "R / Space to restart".into(),
            ]
        } else {
            vec![
                "DODGE — 2D Game Sample".into(),
                format!("Score: {}    Lives: {}", self.score, "♥".repeat(self.lives as usize)),
                "WASD / Arrow keys = move".into(),
            ]
        };
        frame.set_runtime_overlay_lines(lines);

        Ok(())
    }
}

fn hsv(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h6 = h * 6.0;
    let i  = h6.floor() as u32 % 6;
    let f  = h6.fract();
    let p  = v * (1.0 - s);
    let q  = v * (1.0 - f * s);
    let t  = v * (1.0 - (1.0 - f) * s);
    match i { 0=>(v,t,p), 1=>(q,v,p), 2=>(p,v,t), 3=>(p,q,v), 4=>(t,p,v), _=>(v,p,q) }
}

fn main() {
    sturdy_engine::run_game::<Dodge>(
        GameConfig::new(
            WindowConfig::new("Dodge — SturdyEngine 2D Sample", 1280, 720)
                .with_resizable(true),
        )
        .with_fixed_step(Duration::from_secs_f64(1.0 / 60.0)),
    );
}
