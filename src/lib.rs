//! Spell-checking widgets for [`egui`].
//!
//! [`SpellCheckTextEdit`] mirrors `egui::TextEdit`'s builder-style API while
//! underlining misspelled words and offering suggestions in its context menu.
//! It uses embedded Hunspell dictionaries, so it does not require native FFI,
//! filesystem access, or network access at runtime.
//!
//! # Example
//!
//! ```no_run
//! use egui_spellcheck::SpellCheckTextEdit;
//!
//! fn note_editor(ui: &mut egui::Ui, note: &mut String) {
//!     egui::CentralPanel::default().show(ui, |ui| {
//!         SpellCheckTextEdit::multiline(note)
//!             .hint_text("Clinical note")
//!             .desired_width(f32::INFINITY)
//!             .desired_rows(8)
//!             .show(ui);
//!     });
//! }
//! ```
//!
//! Enable the default `grammar` feature for grammar hints, the optional
//! `medical` feature for bundled medical terminology, or use
//! `default-features = false` to build a spelling-only widget.

mod spell_check;

pub use spell_check::SpellCheckTextEdit;
