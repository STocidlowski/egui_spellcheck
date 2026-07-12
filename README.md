# egui_spellcheck

`egui_spellcheck` provides a native-Rust [`egui`](https://github.com/emilk/egui)
text-editing widget with embedded Hunspell spelling dictionaries and optional
grammar hints. It works without native FFI, runtime file access, or network
access, which makes it suitable for desktop and WebAssembly applications.

## Features

- A `TextEdit`-style builder API for single-line and multi-line editors.
- Misspelled words are underlined and receive suggestions in the right-click
  context menu.
- Personal words persist in egui's storage; ignored words are kept for the
  current session.
- An optional bundled medical word list, enabled once per application with the
  `medical` feature.
- Optional grammar checks, enabled by default with the `grammar` feature.
- No automatic text replacement: users explicitly select every suggestion.

## Installation

```toml
[dependencies]
egui_spellcheck = "0.1"
```

To omit the optional grammar engine:

```toml
[dependencies]
egui_spellcheck = { version = "0.1", default-features = false }
```

## Usage

```rust
use egui_spellcheck::SpellCheckTextEdit;

fn note_editor(ui: &mut egui::Ui, note: &mut String) {
    SpellCheckTextEdit::multiline(note)
        .hint_text("Clinical note")
        .desired_width(f32::INFINITY)
        .desired_rows(8)
        .id_salt("clinical-note")
        .show(ui);
}
```

Use `SpellCheckTextEdit::singleline` for a one-line field. The widget also
implements `egui::Widget` when only an `egui::Response` is needed.

## Demo application

The repository includes a native `eframe` demonstration that shows both a
single-line field and a clinical-note editor. It starts with intentional
spelling and grammar mistakes; right-click an underlined word to see available
suggestions.

```sh
cargo run --example spellcheck_demo
```

To also enable the bundled medical terminology, run:

```sh
cargo run --example spellcheck_demo --features medical
```

## Feature flags

| Feature | Default | Description |
| --- | --- | --- |
| `grammar` | Yes | Enables grammar hints through `harper-core`. Disable default features for spelling-only builds. |
| `medical` | No | Adds the bundled medical dictionary to every widget. Increases the final binary size. |

Enable medical terminology for an application with:

```toml
[dependencies]
egui_spellcheck = { version = "0.1", features = ["medical"] }
```

## Dictionary data and licensing

The bundled `en_US` Hunspell dictionary is sourced from the
[`spellbook` project](https://github.com/helix-editor/spellbook). Before the
first public release, document the applicable upstream dictionary license and
the provenance and license of the bundled medical word list in a third-party
notice. This crate's own license must also be selected before publication.

## Development

Run the complete local validation suite with:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
```