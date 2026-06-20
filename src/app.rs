// app.rs — two-pane sine-wave viewer using egui_tiles

use crate::simulation::WaveParams;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------
struct AppState {
    params: WaveParams,
    time:   f32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            params: WaveParams::default(),
            time:   0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Panes
// ---------------------------------------------------------------------------
#[derive(serde::Deserialize, serde::Serialize)]
enum Pane {
    Controls,
    Wave,
}

impl Pane {
    fn title(&self) -> &str {
        match self {
            Pane::Controls => "Controls",
            Pane::Wave     => "Wave",
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> egui_tiles::UiResponse {
        match self {
            Pane::Controls => controls_ui(ui, state),
            Pane::Wave     => wave_ui(ui, state),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-pane rendering functions
// ---------------------------------------------------------------------------
fn controls_ui(ui: &mut egui::Ui, state: &mut AppState) -> egui_tiles::UiResponse {
    let p = &mut state.params;

    ui.add_space(8.0);
    ui.heading("Controls");
    ui.add_space(10.0);

    ui.label("Amplitude");
    ui.add(egui::Slider::new(&mut p.amplitude, 0.05..=1.0).step_by(0.01));

    ui.add_space(6.0);
    ui.label("Frequency (cycles)");
    ui.add(egui::Slider::new(&mut p.frequency, 0.5..=10.0).step_by(0.1));

    ui.add_space(6.0);
    ui.label("Phase (rad)");
    ui.add(
        egui::Slider::new(&mut p.phase, 0.0..=std::f32::consts::TAU)
            .step_by(0.01)
            .suffix(" rad"),
    );

    ui.add_space(6.0);
    ui.label("Animation speed");
    ui.add(egui::Slider::new(&mut p.speed, 0.0..=5.0).step_by(0.1));

    ui.add_space(12.0);
    if ui.button("Reset").clicked() {
        *p = WaveParams::default();
        state.time = 0.0;
    }

    egui_tiles::UiResponse::None
}

fn wave_ui(ui: &mut egui::Ui, state: &mut AppState) -> egui_tiles::UiResponse {
    ui.ctx().request_repaint();

    let draw_rect = ui.available_rect_before_wrap();
    let painter   = ui.painter_at(draw_rect);

    painter.rect_filled(draw_rect, 0.0, ui.visuals().extreme_bg_color);

    let w  = draw_rect.width();
    let h  = draw_rect.height();
    let cx = draw_rect.left();
    let cy = draw_rect.center().y;

    // Axes
    let axis_color = ui.visuals().widgets.noninteractive.fg_stroke.color
        .linear_multiply(0.35);
    painter.line_segment(
        [egui::pos2(cx, cy), egui::pos2(cx + w, cy)],
        egui::Stroke::new(1.0, axis_color),
    );
    painter.line_segment(
        [egui::pos2(cx, draw_rect.top()), egui::pos2(cx, draw_rect.bottom())],
        egui::Stroke::new(1.0, axis_color),
    );

    // Sine curve
    let n = (w as usize).max(2);
    let p = &state.params;
    let points: Vec<egui::Pos2> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let x = cx + t * w;
            let y = cy
                - p.amplitude
                * (h * 0.45)
                * (std::f32::consts::TAU * p.frequency * t + p.phase + state.time).sin();
            egui::pos2(x, y)
        })
        .collect();

    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 180, 255)),
    ));

    // Legend
    painter.text(
        egui::pos2(draw_rect.left() + 8.0, draw_rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        format!("A={:.2}  f={:.1}  φ={:.2}", p.amplitude, p.frequency, p.phase),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );

    egui_tiles::UiResponse::None
}

// ---------------------------------------------------------------------------
// TreeBehavior — thin dispatch layer, nothing more
// ---------------------------------------------------------------------------
struct TreeBehavior<'a> {
    state: &'a mut AppState,
}

impl<'a> egui_tiles::Behavior<Pane> for TreeBehavior<'a> {
    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        pane.ui(ui, self.state)
    }
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Simulation {
    tree: egui_tiles::Tree<Pane>,
    #[serde(skip)]
    state: AppState,
}

impl Default for Simulation {
    fn default() -> Self {
        Self {
            tree:  build_tree(),
            state: AppState::default(),
        }
    }
}

fn build_tree() -> egui_tiles::Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();

    let panes: Vec<egui_tiles::TileId> = [Pane::Controls, Pane::Wave]
        .into_iter()
        .map(|p| {
            let pane = tiles.insert_pane(p);
            tiles.insert_tab_tile(vec![pane])
        })
        .collect();

    let [left, right] = panes.as_slice() else { unreachable!() };

    let root = tiles.insert_horizontal_tile(vec![*left, *right]);
    if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(lin))) =
        tiles.get_mut(root)
    {
        lin.shares.set_share(*left,  28.0);
        lin.shares.set_share(*right, 72.0);
    }

    egui_tiles::Tree::new("main_tree", root, tiles)
}

impl Simulation {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }
}

impl eframe::App for Simulation {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.state.time += dt * self.state.params.speed;

        // Theme toggle in top-right corner
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
            let mut behavior = TreeBehavior { state: &mut self.state };
            self.tree.ui(&mut behavior, ui);
        });
    }
}