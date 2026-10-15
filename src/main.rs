use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("project-consoles")
            .with_inner_size([640.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "project-consoles",
        options,
        Box::new(|_cc| Ok(Box::new(WelcomeApp))),
    )
}

struct WelcomeApp;

impl eframe::App for WelcomeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading(egui::RichText::new("Seja bem-vindo ao project-consoles").size(32.0));
            });
        });
    }
}
