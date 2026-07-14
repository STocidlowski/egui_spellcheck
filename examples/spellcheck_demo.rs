use std::time::{Duration, Instant};

use eframe::egui;
use egui::TextEdit;
use egui_spellcheck::SpellCheckTextEdit;

fn main() -> eframe::Result<()> {
    // Timing experiment ----------------------------------------------------
    //
    // `prewarm()` builds the spelling/grammar engines up front (on a background
    // thread natively) so the first check in the UI doesn't freeze while
    // harper's grammar dictionary and the spellbook word lists are constructed.
    //
    // To *see* whether it actually helps, this demo is instrumented:
    //   * we time how long the `prewarm()` call itself takes to return, and
    //   * the app records the slowest single-frame spell/grammar check cost and
    //     how long after startup the first check completed (see `SpellcheckDemo`).
    //
    // Run it both ways and compare the numbers printed to stdout:
    //   cargo run --release --example spellcheck_demo                 # prewarm ON
    //   $env:SPELLCHECK_NO_PREWARM=1; cargo run --release --example spellcheck_demo   # prewarm OFF (PowerShell)
    //   SPELLCHECK_NO_PREWARM=1 cargo run --release --example spellcheck_demo         # prewarm OFF (bash)
    //
    // With prewarm ON the first check should be near-instant (the engines were
    // already built off-thread). With it OFF the first check pays the full
    // build cost on the UI thread -- you'll see a large "slowest frame" number.
    let prewarm_enabled = std::env::var_os("SPELLCHECK_NO_PREWARM").is_none();
    let app_start = Instant::now();

    if prewarm_enabled {
        let t = Instant::now();
        egui_spellcheck::prewarm();
        println!(
            "[timing] prewarm() ENABLED; call returned in {:?} (engines build on a background thread)",
            t.elapsed()
        );
    } else {
        println!(
            "[timing] prewarm() DISABLED (SPELLCHECK_NO_PREWARM set); engines build lazily on the first check"
        );
    }
    println!("[timing] release build: {}", !cfg!(debug_assertions));

    eframe::run_native(
        "egui_spellcheck demo",
        eframe::NativeOptions::default(),
        Box::new(move |_| Ok(Box::new(SpellcheckDemo::new(app_start, prewarm_enabled)))),
    )
}

struct SpellcheckDemo {
    title: String,
    note: String,
    // Timing instrumentation.
    app_start: Instant,
    prewarm_enabled: bool,
    slowest_frame: Duration,
    first_check_reported: bool,
}

impl SpellcheckDemo {
    fn new(app_start: Instant, prewarm_enabled: bool) -> Self {
        Self {
            app_start,
            prewarm_enabled,
            slowest_frame: Duration::ZERO,
            first_check_reported: false,
            ..Self::sample()
        }
    }

    /// The editable sample text (also used by the "Restore sample text" button).
    fn sample() -> Self {
        Self {
            title: "Follow-up note".to_owned(),
            note: "They is feeling nausia after taking metformin. Reviewed labs, Quantaferon negative.\n\nRight-click an underlined word to see spelling or grammar suggestions.".to_owned(),
            app_start: Instant::now(),
            prewarm_enabled: false,
            slowest_frame: Duration::ZERO,
            first_check_reported: true,
        }
    }
}

impl Default for SpellcheckDemo {
    fn default() -> Self {
        Self::sample()
    }
}

impl eframe::App for SpellcheckDemo {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        // Keep repainting for the first few seconds so the debounced spell/grammar
        // check fires (and its cost is measured) without needing user input.
        if self.app_start.elapsed() < Duration::from_secs(6) {
            ui.ctx().request_repaint();
        }

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
            // Time the (heaviest) spell/grammar-checked field. The frame on which
            // the debounced check first runs pays the one-time engine-build cost
            // unless `prewarm()` already built them on a background thread.
            let check_start = Instant::now();
            SpellCheckTextEdit::multiline(&mut self.note)
                .hint_text("Write a note")
                .desired_width(f32::INFINITY)
                .desired_rows(8)
                .id_salt("demo-note")
                .show(ui);
            let frame_check = check_start.elapsed();
            self.slowest_frame = self.slowest_frame.max(frame_check);

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
                self.title = Self::sample().title;
                self.note = Self::sample().note;
            }

            ui.add_space(12.0);
            ui.separator();
            ui.label(format!(
                "prewarm: {}  |  slowest checked frame so far: {:?}",
                if self.prewarm_enabled { "ON" } else { "OFF" },
                self.slowest_frame
            ));
        });

        // Once the warm-up window has elapsed, print a one-time summary of the
        // slowest single-frame check cost. A large number here means the first
        // check blocked the UI thread (prewarm OFF or not finished in time); a
        // small number means the engines were already warm.
        if !self.first_check_reported && self.app_start.elapsed() >= Duration::from_secs(5) {
            self.first_check_reported = true;
            println!(
                "[timing] prewarm {}: slowest single-frame spell/grammar check in the first 5s = {:?}",
                if self.prewarm_enabled { "ON" } else { "OFF" },
                self.slowest_frame
            );
        }
    }
}
