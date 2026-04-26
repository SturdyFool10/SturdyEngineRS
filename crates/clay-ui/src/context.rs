use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    ElementId, FontDiscovery, GpuWorkQueue, InputEvent, InputSimulator, LayoutCache,
    LayoutTextCacheStats, LayoutTree, OffscreenTarget, RenderCommandKind, RenderCommandList, Size,
    TextStyle, TextWrap, UiTree,
};
use sturdy_engine_core::{Format, ImageDesc, ImageRole, ImageUsage, Limits};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TextSceneKey {
    pub tree: String,
    pub element: u64,
    pub pass: crate::TextPass,
}

#[derive(Default)]
pub struct UiFrameOutput {
    pub trees: HashMap<String, UiTreeFrameOutput>,
    pub text_scenes: HashMap<TextSceneKey, Arc<textui::TextGpuScene>>,
    pub text_surface_plans: HashMap<TextSceneKey, crate::UiSurfacePlan>,
    pub text_tile_image_descs: HashMap<TextSceneKey, Vec<ImageDesc>>,
    pub text_stats: UiTextFrameStats,
}

#[derive(Clone, Debug)]
pub struct UiTreeFrameOutput {
    pub queue: GpuWorkQueue,
    pub surface_plan: crate::ImageTilingPlan,
    pub tile_image_descs: Vec<ImageDesc>,
    pub text_scenes: HashMap<TextSceneKey, Arc<textui::TextGpuScene>>,
    pub text_surface_plans: HashMap<TextSceneKey, crate::UiSurfacePlan>,
    pub text_tile_image_descs: HashMap<TextSceneKey, Vec<ImageDesc>>,
    pub text_stats: UiTextFrameStats,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UiTextFrameStats {
    pub scene_count: usize,
    pub glyph_quad_count: usize,
    pub atlas_page_count: usize,
    pub atlas_bytes: usize,
    pub text_command_count: usize,
    pub batch_count: usize,
    pub layout_cache_hits: usize,
    pub layout_cache_misses: usize,
}

impl UiTextFrameStats {
    pub fn add_scene(&mut self, scene: &textui::TextGpuScene) {
        self.scene_count += 1;
        self.glyph_quad_count += scene.quads.len();
        self.atlas_page_count += scene.atlas_pages.len();
        self.atlas_bytes += scene
            .atlas_pages
            .iter()
            .map(|page| page.rgba8.len())
            .sum::<usize>();
    }

    pub fn merge(&mut self, other: Self) {
        self.scene_count += other.scene_count;
        self.glyph_quad_count += other.glyph_quad_count;
        self.atlas_page_count += other.atlas_page_count;
        self.atlas_bytes += other.atlas_bytes;
        self.text_command_count += other.text_command_count;
        self.batch_count += other.batch_count;
        self.layout_cache_hits += other.layout_cache_hits;
        self.layout_cache_misses += other.layout_cache_misses;
    }

    pub fn add_layout_cache_stats(&mut self, stats: LayoutTextCacheStats) {
        self.layout_cache_hits += stats.hits;
        self.layout_cache_misses += stats.misses;
    }
}

pub struct UiTreeInstance {
    pub name: String,
    pub tree: UiTree,
    pub target: OffscreenTarget,
    pub layout_cache: LayoutCache,
    pub input: InputSimulator,
}

impl UiTreeInstance {
    pub fn new(name: impl Into<String>, tree: UiTree, target: OffscreenTarget) -> Self {
        Self {
            name: name.into(),
            tree,
            target,
            layout_cache: LayoutCache::default(),
            input: InputSimulator::default(),
        }
    }
}

pub struct UiContext {
    text: textui::TextUi,
    font_discovery: FontDiscovery,
    cached_text_scenes: HashMap<u64, Arc<textui::TextGpuScene>>,
    trees: HashMap<String, UiTreeInstance>,
}

impl Default for UiContext {
    fn default() -> Self {
        Self::new()
    }
}

impl UiContext {
    pub fn new() -> Self {
        Self {
            text: textui::TextUi::new(),
            font_discovery: FontDiscovery::new(),
            cached_text_scenes: HashMap::new(),
            trees: HashMap::new(),
        }
    }

    pub fn font_discovery(&self) -> &FontDiscovery {
        &self.font_discovery
    }

    pub fn font_discovery_mut(&mut self) -> &mut FontDiscovery {
        &mut self.font_discovery
    }

    pub fn text_system(&self) -> &textui::TextUi {
        &self.text
    }

    pub fn text_system_mut(&mut self) -> &mut textui::TextUi {
        &mut self.text
    }

    pub fn register_tree(
        &mut self,
        name: impl Into<String>,
        tree: UiTree,
        target: OffscreenTarget,
    ) -> Option<UiTreeInstance> {
        let name = name.into();
        self.trees
            .insert(name.clone(), UiTreeInstance::new(name, tree, target))
    }

    pub fn remove_tree(&mut self, name: &str) -> Option<UiTreeInstance> {
        self.trees.remove(name)
    }

    pub fn tree_mut(&mut self, name: &str) -> Option<&mut UiTreeInstance> {
        self.trees.get_mut(name)
    }

    pub fn queue_input(&mut self, tree: &str, event: InputEvent) {
        if let Some(tree) = self.trees.get_mut(tree) {
            tree.input.queue(event);
        }
    }

    pub fn simulate_activate(&mut self, tree: &str, id: ElementId) {
        self.queue_input(tree, InputEvent::Activate(id));
    }

    pub fn build_frame(
        &mut self,
        viewport: Size,
        frame_info: textui::TextFrameInfo,
        text_scale: f32,
    ) -> UiFrameOutput {
        self.text.begin_frame_info(frame_info);
        self.build_frame_inner(viewport, frame_info, text_scale, None)
    }

    pub fn build_frame_with_limits(
        &mut self,
        viewport: Size,
        frame_number: u64,
        limits: &Limits,
        text_scale: f32,
    ) -> UiFrameOutput {
        let max_texture_side = limits
            .max_image_dimension_2d
            .min(limits.max_texture_2d_size)
            .max(1) as usize;
        let frame_info = textui::TextFrameInfo::new(frame_number, max_texture_side);
        self.text.begin_frame_info(frame_info);
        self.build_frame_inner(viewport, frame_info, text_scale, Some(limits))
    }

    fn build_frame_inner(
        &mut self,
        viewport: Size,
        frame_info: textui::TextFrameInfo,
        text_scale: f32,
        limits: Option<&Limits>,
    ) -> UiFrameOutput {
        let mut output = UiFrameOutput::default();
        let text_ui = &mut self.text;
        let font_discovery = &self.font_discovery;
        let cached_text_scenes = &mut self.cached_text_scenes;
        for tree in self.trees.values_mut() {
            tree.layout_cache.reset_text_stats();
            let mut layout = LayoutTree::default();
            let mut commands = RenderCommandList::default();
            for root in &tree.tree.roots {
                let mut text_measurer =
                    |id: &ElementId, text: &str, style: &TextStyle, width: Option<f32>| {
                        measure_text_for_layout(
                            text_ui,
                            font_discovery,
                            id,
                            text,
                            style,
                            width,
                            text_scale.max(0.001),
                        )
                    };
                if let Ok(root_layout) = LayoutTree::compute_with_text_measurer(
                    root,
                    viewport,
                    &mut tree.layout_cache,
                    &mut text_measurer,
                ) {
                    layout.nodes.extend(root_layout.nodes);
                    let mut root_commands = RenderCommandList::from_element_tree(root, &layout);
                    commands.commands.append(&mut root_commands.commands);
                }
            }
            commands.sort_for_rendering();
            let _ = tree.input.update(&layout);

            let mut queue = GpuWorkQueue::new(tree.name.clone(), tree.target.clone());
            queue.commands = commands.commands;
            queue.rebuild_batches();
            let mut tree_text_stats = UiTextFrameStats::default();
            tree_text_stats.add_layout_cache_stats(tree.layout_cache.text_stats());
            tree_text_stats.text_command_count = queue
                .commands
                .iter()
                .filter(|command| command.kind == RenderCommandKind::Text)
                .count();
            tree_text_stats.batch_count = queue.batches.len();
            let (target_width, target_height) = tree.target.surface_extent(
                frame_info.max_texture_side_px as u32,
                frame_info.max_texture_side_px as u32,
            );
            let surface_plan = if let Some(limits) = limits {
                crate::image_tiling::UiSurfacePlan::from_limits(
                    frame_info.frame_number,
                    target_width,
                    target_height,
                    limits,
                )
            } else {
                crate::image_tiling::UiSurfacePlan {
                    text_frame_info: frame_info,
                    image_tiling_plan: crate::ImageTilingPlan::new_2d(
                        target_width,
                        target_height,
                        frame_info.max_texture_side_px as u32,
                    ),
                }
            };
            let tile_image_descs = surface_plan.image_tiling_plan.to_image_descs(
                Format::Rgba16Float,
                ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                ImageRole::ColorAttachment,
                false,
                Some("clay-ui-tile"),
            );
            let mut tree_text_scenes = HashMap::new();
            let mut tree_text_surface_plans = HashMap::new();
            let mut tree_text_tile_image_descs = HashMap::new();
            gather_text_scenes(
                &tree.name,
                &queue,
                text_ui,
                font_discovery,
                cached_text_scenes,
                viewport,
                text_scale,
                &mut output.text_scenes,
                &mut tree_text_scenes,
                &mut tree_text_surface_plans,
                &mut tree_text_tile_image_descs,
                &mut tree_text_stats,
                limits,
                frame_info.frame_number,
            );
            output.text_stats.merge(tree_text_stats);
            output.trees.insert(
                tree.name.clone(),
                UiTreeFrameOutput {
                    queue,
                    surface_plan: surface_plan.image_tiling_plan,
                    tile_image_descs,
                    text_scenes: tree_text_scenes,
                    text_surface_plans: tree_text_surface_plans,
                    text_tile_image_descs: tree_text_tile_image_descs,
                    text_stats: tree_text_stats,
                },
            );
        }
        output
    }
}

fn measure_text_for_layout(
    text_ui: &mut textui::TextUi,
    font_discovery: &FontDiscovery,
    _id: &ElementId,
    text: &str,
    style: &TextStyle,
    width: Option<f32>,
    text_scale: f32,
) -> Size {
    let resolved_style = style.resolved_with_fonts(font_discovery);
    let size = text_ui.measure_label_size_at_scale(
        text,
        &resolved_style.to_textui_options(),
        width,
        text_scale,
    );
    Size::new(
        size[0].ceil().max(1.0),
        size[1].ceil().max(resolved_style.line_height),
    )
}

fn gather_text_scenes(
    tree: &str,
    queue: &GpuWorkQueue,
    text_ui: &mut textui::TextUi,
    font_discovery: &FontDiscovery,
    cache: &mut HashMap<u64, Arc<textui::TextGpuScene>>,
    viewport: Size,
    text_scale: f32,
    out: &mut HashMap<TextSceneKey, Arc<textui::TextGpuScene>>,
    tree_out: &mut HashMap<TextSceneKey, Arc<textui::TextGpuScene>>,
    tree_surface_plans: &mut HashMap<TextSceneKey, crate::UiSurfacePlan>,
    tree_tile_image_descs: &mut HashMap<TextSceneKey, Vec<ImageDesc>>,
    tree_text_stats: &mut UiTextFrameStats,
    limits: Option<&Limits>,
    frame_number: u64,
) {
    for command in &queue.commands {
        if let crate::render_command::RenderData::Text(text) = &command.data {
            let resolved_style = text.style.resolved_with_fonts(font_discovery);
            let width = if resolved_style.wrap == TextWrap::Words {
                Some(command.rect.size.width.min(viewport.width).max(1.0))
            } else {
                None
            };
            let cache_key =
                resolved_style.cache_fingerprint(&text.text, width, text_scale.max(0.001));
            let scene = if let Some(scene) = cache.get(&cache_key) {
                Arc::clone(scene)
            } else {
                let scene = text_ui.prepare_label_gpu_scene_at_scale(
                    cache_key,
                    &text.text,
                    &resolved_style.to_textui_options(),
                    width,
                    text_scale.max(0.001),
                );
                cache.insert(cache_key, Arc::clone(&scene));
                scene
            };
            let key = TextSceneKey {
                tree: tree.to_string(),
                element: command.id.hash,
                pass: text.pass,
            };
            let target_width = scene.size_points[0].ceil().max(1.0) as u32;
            let target_height = scene.size_points[1].ceil().max(1.0) as u32;
            let plan = if let Some(limits) = limits {
                crate::UiSurfacePlan::from_limits(frame_number, target_width, target_height, limits)
            } else {
                crate::UiSurfacePlan {
                    text_frame_info: textui::TextFrameInfo::new(
                        frame_number,
                        viewport.width.max(viewport.height).max(1.0) as usize,
                    ),
                    image_tiling_plan: crate::ImageTilingPlan::new_2d(
                        target_width,
                        target_height,
                        viewport.width.max(viewport.height).max(1.0) as u32,
                    ),
                }
            };
            tree_surface_plans.insert(
                TextSceneKey {
                    tree: tree.to_string(),
                    element: command.id.hash,
                    pass: text.pass,
                },
                plan.clone(),
            );
            tree_tile_image_descs.insert(
                TextSceneKey {
                    tree: tree.to_string(),
                    element: command.id.hash,
                    pass: text.pass,
                },
                plan.image_tiling_plan.to_image_descs(
                    Format::Rgba16Float,
                    ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                    ImageRole::ColorAttachment,
                    false,
                    Some("clay-ui-text-tile"),
                ),
            );
            out.insert(key.clone(), Arc::clone(&scene));
            tree_text_stats.add_scene(&scene);
            tree_out.insert(key, scene);
        }
    }
}
