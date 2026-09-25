use eframe::egui::{self, Color32, ViewportCommand};

// Alacritty palette
const BG: [u8; 3] = [0x18, 0x18, 0x18];
const RAISED: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
const FG: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
const MUTED: Color32 = Color32::from_rgb(0x7f, 0x7f, 0x7f);

const TITLE_BAR_HEIGHT: f32 = 28.0;

struct Jot {
    opacity: f32,
}

impl Default for Jot {
    fn default() -> Self {
        let opacity = std::env::var("JOT_OPACITY")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .map_or(0.92, |v| v.clamp(0.3, 1.0));
        Self { opacity }
    }
}

impl eframe::App for Jot {
    // Fully transparent clear color so the window alpha comes from our own fills.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let alpha = (self.opacity * 255.0) as u8;
        let bg = Color32::from_rgba_unmultiplied(BG[0], BG[1], BG[2], alpha);

        egui::Panel::top("title_bar")
            .exact_size(TITLE_BAR_HEIGHT)
            .frame(egui::Frame::NONE.fill(bg))
            .show(ui, |ui| title_bar(ui, &ctx));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(bg))
            .show(ui, |_ui| {});
    }
}

fn title_bar(ui: &mut egui::Ui, ctx: &egui::Context) {
    let rect = ui.max_rect();

    // Dragging the bar moves the window; double click toggles maximize.
    let drag = ui.interact(rect, ui.id().with("drag"), egui::Sense::click_and_drag());
    if drag.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    ui.painter().text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "jot",
        egui::FontId::proportional(13.0),
        MUTED,
    );

    // Window buttons, right-aligned.
    let button_w = 40.0;
    let buttons = [("–", 0), ("□", 1), ("×", 2)];
    for (i, (label, action)) in buttons.iter().enumerate() {
        let x = rect.right() - button_w * (buttons.len() - i) as f32;
        let r = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(button_w, TITLE_BAR_HEIGHT),
        );
        let resp = ui.interact(r, ui.id().with(("btn", i)), egui::Sense::click());
        if resp.hovered() {
            let hover = if *action == 2 {
                Color32::from_rgb(0xac, 0x42, 0x42)
            } else {
                RAISED
            };
            ui.painter().rect_filled(r, 0.0, hover);
        }
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(14.0),
            FG,
        );
        if resp.clicked() {
            match action {
                0 => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                1 => {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                }
                _ => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("jot")
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([320.0, 200.0])
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "jot",
        options,
        Box::new(|_cc| Ok(Box::new(Jot::default()))),
    )
}
