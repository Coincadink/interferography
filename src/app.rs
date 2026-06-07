use std::sync::{Arc, Mutex};
use crate::simulation::{FieldCache, SimParams};

// Define the panes
#[derive(serde::Deserialize, serde::Serialize)]
enum Pane {
    TopLeft,
    BottomLeft,
    Right,
}

pub struct SimState {
    pub wavelength: f32,
    pub delta: f32,        // extra path length in arm B
    pub arm_length: f32,   // nominal arm length
    pub beam_width: f32,   // Gaussian beam 1-σ width
    pub speed: f32,
    pub paused: bool,
    pub time: f32,
    pub cache: Option<Arc<FieldCache>>,
    pub dirty: bool,
    pub computing: bool,
    pub pending: Arc<Mutex<Option<Arc<FieldCache>>>>,
    pub texture: Option<egui::TextureHandle>,
    pub pixel_buf: Vec<egui::Color32>,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            wavelength: 20.0,
            delta: 0.0,
            arm_length: 120.0,
            beam_width: 30.0,
            speed: 1.0,
            paused: false,
            time: 0.0,
            cache: None,
            dirty: true,
            computing: false,
            pending: Arc::new(Mutex::new(None)),
            texture: None,
            pixel_buf: Vec::new(),
        }
    }
}

struct TreeBehavior<'a> {
    sim: &'a mut SimState,
}

impl<'a> egui_tiles::Behavior<Pane> for TreeBehavior<'a> {
    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match pane {
            Pane::TopLeft => "Controls".into(),
            Pane::BottomLeft => "Info".into(),
            Pane::Right => "Simulation".into(),
        }
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        match pane {
            Pane::TopLeft => {
                let sim = &mut self.sim;

                ui.heading("Interferometer");
                ui.add_space(8.0);

                let mut changed = false;

                ui.label("wavelength (λ)");
                let r = ui.add(egui::Slider::new(&mut sim.wavelength, 5.0..=50.0).suffix(" px"));
                changed |= r.drag_stopped() || r.lost_focus();

                ui.add_space(4.0);
                ui.label("path length difference (ΔL)");
                ui.small("Arm B extra length — controls interference");
                // Range ±3λ expressed in sim units
                let r = ui.add(
                    egui::Slider::new(&mut sim.delta, -80.0..=80.0)
                        .suffix(" px")
                        .smart_aim(false),
                );
                changed |= r.drag_stopped() || r.lost_focus();

                // Helper: show how many λ the delta corresponds to
                let frac = sim.delta / sim.wavelength;
                ui.small(format!("= {:.2} λ", frac));

                ui.add_space(4.0);
                ui.label("arm length");
                let r = ui.add(egui::Slider::new(&mut sim.arm_length, 40.0..=300.0).suffix(" px"));
                changed |= r.drag_stopped() || r.lost_focus();

                ui.add_space(4.0);
                ui.label("beam width (σ)");
                let r = ui.add(egui::Slider::new(&mut sim.beam_width, 5.0..=80.0).suffix(" px"));
                changed |= r.drag_stopped() || r.lost_focus();

                ui.add_space(8.0);
                ui.label("animation speed");
                ui.add(egui::Slider::new(&mut sim.speed, 0.1..=5.0));

                ui.add_space(4.0);
                if ui.checkbox(&mut sim.paused, "pause").changed() {}

                if changed {
                    sim.dirty = true;
                    sim.cache = None;
                }

                // Quick-set buttons for common path differences
                ui.add_space(8.0);
                ui.label("Preset ΔL:");
                ui.horizontal(|ui| {
                    if ui.small_button("0 (bright)").clicked() {
                        sim.delta = 0.0;
                        sim.dirty = true;
                        sim.cache = None;
                    }
                    if ui.small_button("λ/2 (dark)").clicked() {
                        sim.delta = sim.wavelength / 2.0;
                        sim.dirty = true;
                        sim.cache = None;
                    }
                    if ui.small_button("λ (bright)").clicked() {
                        sim.delta = sim.wavelength;
                        sim.dirty = true;
                        sim.cache = None;
                    }
                });
            }

            Pane::BottomLeft => {
                ui.heading("About");
                ui.add_space(4.0);
                ui.label("Mach-Zehnder interferometer simulation.");
                ui.add_space(6.0);

                // Colour-coded legend using coloured rectangles
                let row_h = 14.0;

                let mut paint_legend = |color: egui::Color32, text: &str| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(12.0, row_h),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 2.0, color);
                        ui.label(text);
                    });
                };

                paint_legend(egui::Color32::from_rgb(60, 80, 180), "Input beam (cyan)");
                paint_legend(egui::Color32::from_rgb(200, 140, 20),  "Arm A — transmitted");
                paint_legend(egui::Color32::from_rgb(20, 160, 190),  "Arm B — reflected (+ ΔL)");
                paint_legend(egui::Color32::from_rgb(180, 40, 180),  "Recombination");
                paint_legend(egui::Color32::from_rgb(60, 140, 230),  "Output / fringes");

                ui.add_space(8.0);
                ui.label("ΔL = 0  → constructive (bright)");
                ui.label("ΔL = λ/2 → destructive (dark)");
                ui.label("ΔL = nλ  → constructive again");
            }

            Pane::Right => {
                ui.ctx().request_repaint();
                let sim = &mut self.sim;

                // Kick off background recompute if needed
                if sim.dirty && !sim.computing {
                    sim.dirty = false;
                    sim.computing = true;
                    let rect = ui.available_rect_before_wrap();

                    #[cfg(target_arch = "wasm32")]
                    let w = (rect.width() as usize).max(32).min(128);
                    #[cfg(target_arch = "wasm32")]
                    let h = (rect.height() as usize).max(32).min(128);

                    #[cfg(not(target_arch = "wasm32"))]
                    let w = (rect.width() as usize).max(64).min(512);
                    #[cfg(not(target_arch = "wasm32"))]
                    let h = (rect.height() as usize).max(64).min(512);

                    let p = SimParams {
                        wavelength: sim.wavelength,
                        delta: sim.delta,
                        arm_length: sim.arm_length,
                        beam_width: sim.beam_width,
                        width: w,
                        height: h,
                    };

                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let mailbox = Arc::clone(&sim.pending);
                        std::thread::spawn(move || {
                            let cache = Arc::new(FieldCache::compute(&p));
                            *mailbox.lock().unwrap() = Some(cache);
                        });
                    }

                    #[cfg(target_arch = "wasm32")]
                    {
                        let cache = Arc::new(FieldCache::compute(&p));
                        sim.cache = Some(cache);
                        sim.computing = false;
                    }
                }

                // Collect finished result (native only)
                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(mut guard) = sim.pending.try_lock() {
                    if let Some(new_cache) = guard.take() {
                        sim.cache = Some(new_cache);
                        sim.computing = false;
                    }
                }

                // Advance time
                if !sim.paused {
                    sim.time += ui.input(|i| i.stable_dt) * sim.speed * 2.0;
                }

                let rect = ui.available_rect_before_wrap();

                if let Some(cache) = sim.cache.clone() {
                    let w = cache.params.width;
                    let h = cache.params.height;
                    cache.render(&mut sim.pixel_buf, sim.time);

                    let img = egui::ColorImage {
                        size: [w, h],
                        pixels: sim.pixel_buf.clone(),
                        source_size: egui::vec2(w as f32, h as f32),
                    };

                    match &mut sim.texture {
                        Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                        None => {
                            sim.texture = Some(ui.ctx().load_texture(
                                "sim_field",
                                img,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                    }

                    if let Some(tex) = &sim.texture {
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(1.0, 1.0),
                        );
                        ui.painter().image(tex.id(), rect, uv, egui::Color32::WHITE);
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(if sim.computing {
                            "computing field…"
                        } else {
                            "initialising…"
                        });
                    });
                }
            }
        }
        egui_tiles::UiResponse::None
    }
}


#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct TemplateApp {
    label: String,
    #[serde(skip)]
    value: f32,
    tree: egui_tiles::Tree<Pane>,
    #[serde(skip)]
    sim: SimState,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let mut tiles = egui_tiles::Tiles::default();

        let top_left = tiles.insert_pane(Pane::TopLeft);
        let bottom_left = tiles.insert_pane(Pane::BottomLeft);
        let right = tiles.insert_pane(Pane::Right);

        let left_tab_top = tiles.insert_tab_tile(vec![top_left]);
        let left_tab_bottom = tiles.insert_tab_tile(vec![bottom_left]);
        let left_split = tiles.insert_vertical_tile(vec![left_tab_top, left_tab_bottom]);
        let right_tab = tiles.insert_tab_tile(vec![right]);
        let root = tiles.insert_horizontal_tile(vec![left_split, right_tab]);

        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(linear))) =
            tiles.get_mut(root)
        {
            linear.shares.set_share(left_split, 25.0);
            linear.shares.set_share(right_tab, 75.0);
        }

        let tree = egui_tiles::Tree::new("main_tree", root, tiles);

        Self {
            label: "Hello World!".to_owned(),
            value: 2.7,
            tree,
            sim: SimState::default(),
        }
    }
}

impl TemplateApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }
}

impl eframe::App for TemplateApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let content_rect = ui.ctx().content_rect();
        egui::Area::new(egui::Id::new("theme_toggle"))
            .fixed_pos(egui::pos2(content_rect.right() - 36.0, 7.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::default().inner_margin(4.0).show(ui, |ui| {
                    let is_dark = ui.visuals().dark_mode;
                    let icon = if is_dark { "☀" } else { "🌙" };
                    if ui.button(icon).clicked() {
                        let new_theme = if is_dark { egui::Theme::Light } else { egui::Theme::Dark };
                        ui.ctx().set_theme(new_theme);
                    }
                });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let mut behavior = TreeBehavior { sim: &mut self.sim };
            self.tree.ui(&mut behavior, ui);
        });
    }
}