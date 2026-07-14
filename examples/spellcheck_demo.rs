use eframe::egui;
use egui::TextEdit;
use egui_spellcheck::SpellCheckTextEdit;

fn main() -> eframe::Result<()> {
    // Build the spelling/grammar engines on a background thread up front so the
    // first check doesn't freeze the UI while harper's grammar dictionary and
    // the spellbook word lists are constructed.
    egui_spellcheck::prewarm();

    eframe::run_native(
        "egui_spellcheck demo",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(SpellcheckDemo::default()))),
    )
}

struct SpellcheckDemo {
    title: String,
    note: String,
}

impl Default for SpellcheckDemo {
    fn default() -> Self {
        Self {
            title: "Follow-up note".to_owned(),
            note: "They is feeling nausia after taking metformin. Reviewed labs, Quantaferon negative.\n\nRight-click an underlined word to see spelling or grammar suggestions.".to_owned(),
        }
    }
}

impl eframe::App for SpellcheckDemo {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("egui_spellcheck");
            ui.label("Edit the sample note, then right-click an underlined word for suggestions.");

            #[cfg(feature = "medical-en")]
            ui.small("Medical terminology is enabled for this build.");
            #[cfg(not(feature = "medical-en"))]
            ui.small(
                "Medical terminology is disabled. Run with `--features medical-en` to enable it.",
            );

            ui.add_space(12.0);
            SpellCheckTextEdit::singleline(&mut self.title)
                .hint_text("Note title")
                .desired_width(f32::INFINITY)
                .id_salt("demo-title")
                .show(ui);

            ui.add_space(8.0);
            SpellCheckTextEdit::multiline(&mut self.note)
                .hint_text("Write a note")
                .desired_width(f32::INFINITY)
                .desired_rows(8)
                .id_salt("demo-note")
                .show(ui);

            ui.heading("Without Spellcheck (normal TextEdit::Multiline)");
            ui.add_space(8.0);
            TextEdit::multiline(&mut self.note)
                .hint_text("Write a note")
                .desired_width(f32::INFINITY)
                .desired_rows(8)
                .id_salt("demo-note")
                .show(ui);

            ui.add_space(8.0);
            if ui.button("Restore sample text").clicked() {
                *self = Self::default();
            }
        });
    }
}
