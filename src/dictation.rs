//! Optional hold-to-dictate chrome. Capture and networking belong to the host.

use crate::SpellCheckTextEdit;
use egui::{Response, Sense, Ui, Vec2, text_edit::TextEditOutput};

/// The host's current state for this field's microphone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DictationUiState {
    /// No capture or request is pending for this field.
    #[default]
    Idle,
    /// Waiting for the microphone route to become ready.
    Starting,
    /// Capturing audio while held.
    Recording,
    /// Transcribing a released recording.
    Processing,
}

/// Host-provided configuration. Ordinary editors show no microphone by default.
#[derive(Clone, Copy, Debug)]
pub struct DictationControl<'a> {
    /// Capture/request feedback for this field.
    pub state: DictationUiState,
    /// Whether a new hold is permitted (editability, mode and permissions).
    pub enabled: bool,
    /// Explanation shown when the host disables capture.
    pub disabled_reason: Option<&'a str>,
}

impl Default for DictationControl<'_> {
    fn default() -> Self {
        Self {
            state: DictationUiState::Idle,
            enabled: true,
            disabled_reason: None,
        }
    }
}

/// An interaction intent. The host binds Start to a field and owns the resulting request ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictationIntent {
    /// Primary pointer or Space was pressed on the microphone.
    Start,
    /// Released, including outside the button. Finish exactly once.
    Finish,
    /// Escape, lost application focus or disabled capture cancelled the hold.
    Cancel,
}

/// Full editor output, preserving caret and galley access for collaborative hosts.
pub struct DictationTextEditOutput {
    /// The original spelling editor's output.
    pub editor: TextEditOutput,
    /// Optional intent for the host to process after releasing the text borrow.
    pub intent: Option<DictationIntent>,
    /// The microphone's response, including its fixed-size rectangle.
    pub microphone: Response,
}

/// An explicitly opted-in editor. Its fixed microphone gutter never changes on hover.
#[must_use = "Call show to draw the editor"]
pub struct DictationTextEdit<'t, 'c> {
    editor: SpellCheckTextEdit<'t>,
    control: DictationControl<'c>,
}

impl<'t> SpellCheckTextEdit<'t> {
    /// Add an optional hold-to-dictate control without coupling the editor to an audio engine.
    /// The default `show` and `Widget` APIs keep their existing appearance and behavior.
    pub fn with_dictation(self, control: DictationControl<'_>) -> DictationTextEdit<'t, '_> {
        DictationTextEdit {
            editor: self,
            control,
        }
    }
}

impl DictationTextEdit<'_, '_> {
    /// Draw the field and microphone, returning start/release/cancel intents to the host.
    /// Hosts must also cancel their hold if this editor disappears from the UI.
    pub fn show(self, ui: &mut Ui) -> DictationTextEditOutput {
        let control = self.control;
        let available = (ui.available_width() - 28.0 - ui.spacing().item_spacing.x).max(1.0);
        let width = self
            .editor
            .desired_width_hint()
            .unwrap_or(ui.spacing().text_edit_width)
            .min(available)
            .max(1.0);
        ui.horizontal_top(|ui| {
            let mut editor = self.editor.desired_width(width).show(ui);
            let (rect, mut microphone) =
                ui.allocate_exact_size(Vec2::new(28.0, 24.0), Sense::click_and_drag());
            let held_id = editor.response.id.with("dictation-held");
            let frame = ui.ctx().cumulative_frame_nr();
            let (mut held, last_frame, mut keyboard_hold) = ui
                .data(|data| data.get_temp::<(bool, u64, bool)>(held_id))
                .unwrap_or((false, frame, false));
            if frame > last_frame.saturating_add(1) {
                held = false;
            }
            let keyboard = microphone.has_focus();
            let (pressed, released, cancelled, space_pressed) = ui.input(|input| {
                (
                    (microphone.hovered() && input.pointer.primary_pressed())
                        || (keyboard && input.key_pressed(egui::Key::Space)),
                    if keyboard_hold {
                        input.key_released(egui::Key::Space)
                    } else {
                        input.pointer.primary_released()
                    },
                    !input.focused || input.key_pressed(egui::Key::Escape),
                    keyboard && input.key_pressed(egui::Key::Space),
                )
            });
            let mut intent = None;
            if held && (cancelled || !control.enabled) {
                held = false;
                intent = Some(DictationIntent::Cancel);
            } else if held && released {
                held = false;
                intent = Some(DictationIntent::Finish);
            } else if !held && pressed && control.enabled && control.state == DictationUiState::Idle
            {
                held = true;
                keyboard_hold = space_pressed;
                intent = Some(DictationIntent::Start);
                if !keyboard {
                    editor.response.request_focus();
                    editor.cursor_range = editor.state.cursor.char_range();
                }
            }
            ui.data_mut(|data| data.insert_temp(held_id, (held, frame, keyboard_hold)));
            microphone.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    control.enabled,
                    "Hold to dictate",
                )
            });
            let active = held || control.state != DictationUiState::Idle;
            if active
                || editor.response.hovered()
                || editor.response.has_focus()
                || microphone.hovered()
                || microphone.has_focus()
            {
                let color = if !control.enabled {
                    ui.visuals().weak_text_color()
                } else if control.state == DictationUiState::Recording {
                    ui.visuals().error_fg_color
                } else {
                    ui.visuals().text_color()
                };
                if microphone.hovered() || active {
                    ui.painter()
                        .rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
                }
                let center = rect.center();
                let stroke = egui::Stroke::new(1.5, color);
                if matches!(
                    control.state,
                    DictationUiState::Starting | DictationUiState::Processing
                ) {
                    for offset in [-5.0, 0.0, 5.0] {
                        ui.painter()
                            .circle_filled(center + egui::vec2(offset, 0.0), 1.5, color);
                    }
                } else {
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(
                            center + egui::vec2(0.0, -3.0),
                            egui::vec2(6.0, 10.0),
                        ),
                        3.0,
                        color,
                    );
                    ui.painter().line(
                        vec![
                            center + egui::vec2(-6.0, -4.0),
                            center + egui::vec2(-6.0, 2.0),
                            center + egui::vec2(0.0, 5.0),
                            center + egui::vec2(6.0, 2.0),
                            center + egui::vec2(6.0, -4.0),
                        ],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [center + egui::vec2(0.0, 5.0), center + egui::vec2(0.0, 8.0)],
                        stroke,
                    );
                }
            }
            microphone = microphone.on_hover_text(if control.enabled {
                "Hold to dictate; release to transcribe. Escape cancels."
            } else {
                control
                    .disabled_reason
                    .unwrap_or("Dictation is unavailable for this field.")
            });
            DictationTextEditOutput {
                editor,
                intent,
                microphone,
            }
        })
        .inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        text: &mut String,
        events: Vec<egui::Event>,
        focused: bool,
    ) -> DictationTextEditOutput {
        let mut result = None;
        let mut full = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events,
                focused,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    result = Some(
                        SpellCheckTextEdit::multiline(text)
                            .id_salt("field")
                            .with_dictation(DictationControl::default())
                            .show(ui),
                    );
                });
            },
        );
        full.textures_delta.clear();
        result.unwrap()
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    #[test]
    fn opting_in_preserves_an_explicit_singleline_width() {
        let ctx = egui::Context::default();
        let mut plain = "café".to_owned();
        let mut controlled = plain.clone();
        let mut full = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                let plain = SpellCheckTextEdit::singleline(&mut plain)
                    .desired_width(120.0)
                    .show(ui);
                let controlled = SpellCheckTextEdit::singleline(&mut controlled)
                    .desired_width(120.0)
                    .with_dictation(DictationControl::default())
                    .show(ui);
                assert_eq!(
                    plain.response.rect.width(),
                    controlled.editor.response.rect.width()
                );
            },
        );
        full.textures_delta.clear();
    }

    #[test]
    fn hold_releases_outside_the_button_and_hover_does_not_resize_the_field() {
        let ctx = egui::Context::default();
        let mut text = "café µg".to_owned();
        let idle = frame(&ctx, &mut text, Vec::new(), true);
        let pressed = frame(
            &ctx,
            &mut text,
            pointer(idle.microphone.rect.center(), true),
            true,
        );
        assert_eq!(pressed.intent, Some(DictationIntent::Start));
        assert_eq!(
            idle.editor.response.rect.size(),
            pressed.editor.response.rect.size()
        );
        let released = frame(
            &ctx,
            &mut text,
            pointer(egui::pos2(2.0, 150.0), false),
            true,
        );
        assert_eq!(released.intent, Some(DictationIntent::Finish));
        assert_eq!(frame(&ctx, &mut text, Vec::new(), true).intent, None);
        assert_eq!(text, "café µg");
    }

    #[test]
    fn microphone_press_preserves_the_character_selection_and_text_focus() {
        let ctx = egui::Context::default();
        let mut text = "café µg".to_owned();
        let initial = frame(&ctx, &mut text, Vec::new(), true);
        let range =
            egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(5));
        let mut state = initial.editor.state;
        state.cursor.set_char_range(Some(range));
        state.store(&ctx, initial.editor.response.id);
        ctx.memory_mut(|memory| memory.request_focus(initial.editor.response.id));
        let idle = frame(&ctx, &mut text, Vec::new(), true);
        let pressed = frame(
            &ctx,
            &mut text,
            pointer(idle.microphone.rect.center(), true),
            true,
        );
        assert_eq!(pressed.intent, Some(DictationIntent::Start));
        assert_eq!(
            pressed.editor.cursor_range.unwrap().as_sorted_char_range(),
            range.as_sorted_char_range()
        );
        assert!(pressed.editor.response.has_focus());
        assert_eq!(text, "café µg");
    }

    #[test]
    fn an_editor_hidden_during_release_can_start_a_new_hold_when_shown_again() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        let idle = frame(&ctx, &mut text, Vec::new(), true);
        let pressed = frame(
            &ctx,
            &mut text,
            pointer(idle.microphone.rect.center(), true),
            true,
        );
        assert_eq!(pressed.intent, Some(DictationIntent::Start));
        let mut hidden = ctx.run_ui(
            egui::RawInput {
                events: pointer(egui::pos2(2.0, 150.0), false),
                ..Default::default()
            },
            |_| {},
        );
        hidden.textures_delta.clear();
        let shown = frame(
            &ctx,
            &mut text,
            vec![egui::Event::PointerMoved(idle.microphone.rect.center())],
            true,
        );
        let pressed_again = frame(
            &ctx,
            &mut text,
            pointer(shown.microphone.rect.center(), true),
            true,
        );
        assert_eq!(pressed_again.intent, Some(DictationIntent::Start));
    }

    #[test]
    fn application_focus_loss_cancels_instead_of_finishing() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        let idle = frame(&ctx, &mut text, Vec::new(), true);
        assert_eq!(
            frame(
                &ctx,
                &mut text,
                pointer(idle.microphone.rect.center(), true),
                true
            )
            .intent,
            Some(DictationIntent::Start)
        );
        assert_eq!(
            frame(&ctx, &mut text, Vec::new(), false).intent,
            Some(DictationIntent::Cancel)
        );
    }
}
