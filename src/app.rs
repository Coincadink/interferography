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
    pub slit_sep: f32,
    pub slit_width: f32,
    pub wavelength: f32,
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
            slit_sep: 60.0,
            slit_width: 12.0,
            wavelength: 20.0,
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

// Implement the Behavior trait to control what renders in each pane
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
                
                ui.heading("Fourier optics");
                ui.add_space(8.0);

                let mut changed = false;

                ui.label("slit separation");
                let r = ui.add(egui::Slider::new(&mut sim.slit_sep, 10.0..=200.0).suffix(" px"));
                changed |= r.drag_stopped() || r.lost_focus();

                ui.label("slit width");
                let r = ui.add(egui::Slider::new(&mut sim.slit_width, 2.0..=40.0).suffix(" px"));
                changed |= r.drag_stopped() || r.lost_focus();

                ui.label("wavelength (λ)");
                let r = ui.add(egui::Slider::new(&mut sim.wavelength, 5.0..=50.0).suffix(" px"));
                changed |= r.drag_stopped() || r.lost_focus();

                ui.label("animation speed");
                ui.add(egui::Slider::new(&mut sim.speed, 0.1..=5.0));

                if changed {
                    sim.dirty = true;
                    sim.cache = None;
                }
            }

            Pane::BottomLeft => {
                ui.heading("About");
                ui.add_space(4.0);
                ui.label("Fresnel propagation of a double slit.");
                ui.label("Vertical axis = z (propagation depth).");
                ui.label("Horizontal axis = x (transverse position).");
                ui.add_space(8.0);
                ui.label("Near field (top): Huygens wavelets still separate.");
                ui.label("Far field (bottom): classic sinc² pattern.");
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
                        slit_sep: sim.slit_sep,
                        slit_width: sim.slit_width,
                        wavelength: sim.wavelength,
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
                        // No threads on WASM — compute synchronously
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

        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(linear))) = tiles.get_mut(root) {
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