use sturdy_engine::{RuntimeController, RuntimeDiagnostics, RuntimeSettingKey, ShellFrame};

use super::{ShowcaseScene, Testbed, tone_mapping_label};

impl Testbed {
    pub(super) fn overlay_lines(
        &self,
        shell_frame: &ShellFrame<'_>,
        runtime_controller: &RuntimeController,
    ) -> Vec<String> {
        let diagnostics = shell_frame.runtime_diagnostics();
        let mut overlay_lines = vec![
            format!(
                "scene {}: {} — {}",
                self.selected_scene.number(),
                self.selected_scene.label(),
                self.selected_scene.summary()
            ),
            ShowcaseScene::picker_line(),
            self.global_tonemap_bloom_line(),
            self.global_aa_hdr_line(),
        ];

        if let Some(line) = self.demo_status_line(runtime_controller) {
            overlay_lines.push(line);
        }
        if let Some(line) = self.debug_status_line(shell_frame, runtime_controller) {
            overlay_lines.push(line);
        }
        if let Some(line) = self.window_status_line(runtime_controller) {
            overlay_lines.push(line);
        }

        overlay_lines.push(Self::compact_runtime_line(&diagnostics));
        if self.show_graph_inspector {
            overlay_lines.extend(shell_frame.runtime_graph_inspection_lines(6, 4));
        }

        overlay_lines.push(Self::global_keys_line());
        if let Some(line) = self.demo_keys_line() {
            overlay_lines.push(line);
        }

        for err in diagnostics.shader_compile_errors {
            overlay_lines.push(format!(
                "[shader error] {}: {}",
                err.path
                    .file_name()
                    .unwrap_or(err.path.as_os_str())
                    .to_string_lossy(),
                err.message.lines().next().unwrap_or("compile failed"),
            ));
        }

        overlay_lines
    }

    fn global_tonemap_bloom_line(&self) -> String {
        let bloom_state = if self.bloom_enabled {
            if self.bloom_only { "only" } else { "on" }
        } else {
            "off"
        };
        format!(
            "global post: tonemap={} {}={:.2} | bloom={}",
            tone_mapping_label(self.tone_mapping),
            self.selected_tonemap_dial.label(),
            self.tonemap_settings.get(self.selected_tonemap_dial),
            bloom_state,
        )
    }

    fn global_aa_hdr_line(&self) -> String {
        format!(
            "global image: aa={} {}={:.2} | hdr={}",
            self.aa.mode.label(),
            self.aa.selected_dial.label(),
            self.current_aa_dial_value(),
            Self::on_off(self.hdr_output),
        )
    }

    fn demo_status_line(&self, runtime_controller: &RuntimeController) -> Option<String> {
        match self.selected_scene {
            ShowcaseScene::RealtimeShadows => Some(
                "demo: shadow camera orbit; deferred path stays focused on PBR + CSM".to_string(),
            ),
            ShowcaseScene::TemporalAndPost => Some(format!(
                "demo setting: motion-vector debug={}",
                if self.show_motion_vectors {
                    "shown"
                } else {
                    "hidden"
                }
            )),
            ShowcaseScene::ProceduralTextures => Some(format!(
                "demo setting: procedural texture resolution={} ({}px)",
                self.texture_resolution.label(),
                self.texture_resolution.size(),
            )),
            ShowcaseScene::Bloom => {
                Some("demo: bright HDR emitters; use global bloom controls".to_string())
            }
            ShowcaseScene::CornellPathTracing => {
                let [x, y, z] = self.path_tracer_camera.position();
                let camera = if self.path_tracer_subscene
                    == super::path_tracer_subscene::PathTracerSubscene::Outdoor
                {
                    format!(" | camera=({x:.1}, {y:.1}, {z:.1})")
                } else {
                    String::new()
                };
                Some(format!(
                    "demo setting: RT subscene {} {}{} | accumulation={} denoise=SVGF display-only path={}",
                    self.path_tracer_subscene.slot(),
                    self.path_tracer_subscene.label(),
                    camera,
                    if self.cornell_rt_scene.is_some() {
                        "raw progressive"
                    } else {
                        "shader fallback"
                    },
                    if self.cornell_rt_scene.is_some() {
                        "hardware"
                    } else {
                        "fullscreen shader"
                    }
                ))
            }
            ShowcaseScene::DebugGraph => Some(format!(
                "demo: graph inspector={} selected_debug={}",
                Self::on_off(self.show_graph_inspector),
                self.debug_view_picker
                    .selected_name(runtime_controller)
                    .unwrap_or_else(|| "off".to_string())
            )),
            ShowcaseScene::Overview
            | ShowcaseScene::ProceduralSky
            | ShowcaseScene::ProceduralMaterials => None,
        }
    }

    fn debug_status_line(
        &self,
        shell_frame: &ShellFrame<'_>,
        runtime_controller: &RuntimeController,
    ) -> Option<String> {
        let selected_debug = self.debug_view_picker.selected_name(runtime_controller);
        let should_show = self.selected_scene == ShowcaseScene::DebugGraph
            || selected_debug.is_some()
            || self.show_graph_inspector
            || self.pending_debug_image_export;
        if !should_show {
            return None;
        }

        Some(format!(
            "debug: view={} | images={} | export={}",
            selected_debug.unwrap_or_else(|| "off".to_string()),
            shell_frame.debug_image_names().len(),
            if self.pending_debug_image_export {
                "queued"
            } else {
                "idle"
            }
        ))
    }

    fn window_status_line(&self, runtime_controller: &RuntimeController) -> Option<String> {
        let transparency = runtime_controller
            .bool_setting(RuntimeSettingKey::SurfaceTransparency)
            .unwrap_or(false);
        let effect = runtime_controller
            .text_setting(RuntimeSettingKey::WindowBackgroundEffect)
            .unwrap_or_else(|| "None".to_string());
        if !transparency && effect == "None" {
            None
        } else {
            Some(format!(
                "window: transparency={} effect={}",
                Self::on_off(transparency),
                effect
            ))
        }
    }

    fn compact_runtime_line(diagnostics: &RuntimeDiagnostics) -> String {
        format!(
            "runtime: {:?} | graph={}p/{}i w{} e{} | cpu={} gpu={}",
            diagnostics.backend,
            diagnostics.graph.pass_count,
            diagnostics.graph.image_count,
            diagnostics.graph.warning_count,
            diagnostics.graph.error_count,
            Self::timing_label(diagnostics.timings.cpu_frame_time_ms),
            Self::timing_label(diagnostics.timings.gpu_frame_time_ms),
        )
    }

    fn global_keys_line() -> String {
        "keys: 1-9 scenes | O overlay | T/P/[/]/R tonemap | B/b bloom | a/D/./,/U AA | H HDR"
            .to_string()
    }

    fn demo_keys_line(&self) -> Option<String> {
        match self.selected_scene {
            ShowcaseScene::RealtimeShadows => {
                Some("demo keys: arrows orbit | PgUp/PgDn zoom".to_string())
            }
            ShowcaseScene::TemporalAndPost => Some("demo keys: V motion-vector debug".to_string()),
            ShowcaseScene::ProceduralTextures => {
                Some("demo keys: F1/F2/F3 texture resolution".to_string())
            }
            ShowcaseScene::CornellPathTracing => Some(format!(
                "demo keys: {} | outdoor camera WASD move Q/E down/up arrows look Ctrl fast | Shift+A clear accumulation | N/M debug views | E export",
                super::path_tracer_subscene::PathTracerSubscene::picker_line()
            )),
            ShowcaseScene::DebugGraph => {
                Some("demo keys: N/M debug view | E export | I graph inspector".to_string())
            }
            ShowcaseScene::Overview
            | ShowcaseScene::ProceduralSky
            | ShowcaseScene::ProceduralMaterials
            | ShowcaseScene::Bloom => None,
        }
    }

    fn on_off(value: bool) -> &'static str {
        if value { "on" } else { "off" }
    }

    fn timing_label(value: Option<f32>) -> String {
        value
            .map(|ms| format!("{ms:.1}ms"))
            .unwrap_or_else(|| "pending".to_string())
    }
}
