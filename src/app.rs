// Define the panes
#[derive(serde::Deserialize, serde::Serialize)]
enum Pane {
    Left,
    TopRight,
    BottomRight,
}

// Implement the Behavior trait to control what renders in each pane
struct TreeBehavior;

impl egui_tiles::Behavior<Pane> for TreeBehavior {
    
    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match pane {
            Pane::Left => "Left".into(),
            Pane::TopRight => "Top Right".into(),
            Pane::BottomRight => "Bottom Right".into(),
        }
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        match pane {
            Pane::Left => {
                ui.heading("Left Panel");
            }
            Pane::TopRight => {
                ui.heading("Top Right Panel");
            }
            Pane::BottomRight => {
                ui.heading("Bottom Right Panel");
            }
        }
        egui_tiles::UiResponse::None
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct TemplateApp {
    label: String,
    #[serde(skip)]
    value: f32,
    tree: egui_tiles::Tree<Pane>,  // add the tree
}


impl Default for TemplateApp {
    fn default() -> Self {
        let mut tiles = egui_tiles::Tiles::default();

        let left = tiles.insert_pane(Pane::Left);
        let top_right = tiles.insert_pane(Pane::TopRight);
        let bottom_right = tiles.insert_pane(Pane::BottomRight);

        // TopRight and BottomRight stacked vertically in a tab container
        let right_tab_top = tiles.insert_tab_tile(vec![top_right]);
        let right_tab_bottom = tiles.insert_tab_tile(vec![bottom_right]);
        let right_split = tiles.insert_vertical_tile(vec![right_tab_top, right_tab_bottom]);

        // Left panel in its own tab container
        let left_tab = tiles.insert_tab_tile(vec![left]);

        // Left and right side by side
        let root = tiles.insert_horizontal_tile(vec![left_tab, right_split]);

        // Set left panel to 20% and right to 80%
        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(linear))) =
            tiles.get_mut(root)
        {
            linear.shares.set_share(left_tab, 20.0);
            linear.shares.set_share(right_split, 80.0);
        }

        let tree = egui_tiles::Tree::new("main_tree", root, tiles);

        Self {
            label: "Hello World!".to_owned(),
            value: 2.7,
            tree,
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
        // Floating top-right theme toggle
        let screen_rect = ui.ctx().screen_rect();
        egui::Area::new(egui::Id::new("theme_toggle"))
            .fixed_pos(egui::pos2(screen_rect.right() - 36.0, 7.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::none()
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        let is_dark = ui.visuals().dark_mode;
                        let icon = if is_dark { "☀" } else { "🌙" };
                        if ui.button(icon).clicked() {
                            let new_theme = if is_dark {
                                egui::Theme::Light
                            } else {
                                egui::Theme::Dark
                            };
                            ui.ctx().set_theme(new_theme);
                        }
                    });
            });

        // CentralPanel now takes the full window
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let mut behavior = TreeBehavior;
            self.tree.ui(&mut behavior, ui);
        });
    }
}