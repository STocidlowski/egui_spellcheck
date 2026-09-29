use egui_spellcheck::{SpellCheckTextEdit, suggestions};

#[test]
fn public_widget_api_supports_the_documented_builder_configuration() {
    let mut note = String::from("Patient reports nausia.");

    let _edit = SpellCheckTextEdit::multiline(&mut note)
        .hint_text("Clinical note")
        .desired_width(f32::INFINITY)
        .desired_rows(4)
        .id_salt("clinical-note")
        .interactive(true);
}

#[test]
fn public_suggestions_api_returns_ranked_corrections() {
    let corrections = suggestions("recieve");

    assert!(corrections.iter().any(|word| word == "receive"));
}

#[test]
fn public_widget_renders_in_singleline_and_multiline_modes() {
    let mut title = String::from("A short title");
    let mut note = String::from("Patient reports nausia.");

    egui::__run_test_ui(|ui| {
        let _singleline = SpellCheckTextEdit::singleline(&mut title).show(ui);
        let _multiline = SpellCheckTextEdit::multiline(&mut note).show(ui);
    });
}
