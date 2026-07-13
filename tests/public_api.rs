use egui_spellcheck::SpellCheckTextEdit;

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
