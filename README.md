# egui_spellcheck

`egui_spellcheck` provides a native-Rust [`egui`](https://github.com/emilk/egui)
text-editing widget with embedded Hunspell spelling dictionaries and optional
grammar hints. It works without native FFI, runtime file access, or network
access, which makes it suitable for desktop and WebAssembly applications (see
[WebAssembly](#webassembly) for the small `getrandom` configuration the grammar
feature needs on the web).

[`SpellCheckTextEdit`] mirrors `egui::TextEdit`'s builder-style API while
underlining misspelled words and offering suggestions in its context menu.

This project began as a widget for a few personal medical projects, hence the 
medical terminology and grammar hints.

Screenshot in action (/examples/spellcheck_demo.rs)
![Screenshot of program](/examples/screenshot.jpg)


## Example
```no_run
use egui_spellcheck::SpellCheckTextEdit;
fn note_editor(ui: &mut egui::Ui, note: &mut String) {
    egui::CentralPanel::default().show(ui, |ui| {
        SpellCheckTextEdit::multiline(note)
            .hint_text("Clinical note")
            .desired_width(f32::INFINITY)
            .desired_rows(8)
            .show(ui);
    });
}
```

Enable the default `grammar` feature for grammar hints, the optional
`medical-en` feature for bundled medical terminology, or use
`default-features = false` to build a spelling-only widget.


## Features

- A `TextEdit`-style builder API for single-line and multi-line editors.
- Misspelled words are underlined and receive suggestions in a structured
  right-click context menu that also includes dictionary actions and the
  standard editing commands (Undo, Cut/Copy/Paste/Delete, Select All).
- Personal words persist in egui's storage; ignored words are kept for the
  current session.
- An optional bundled medical word list, enabled once per application with the
  `medical-en` feature.
- Optional grammar checks, enabled by default with the `grammar` feature.
- No automatic text replacement: users explicitly select every suggestion.

## Installation

```toml
[dependencies]
egui_spellcheck = "0.35.0"
```

To omit the optional grammar engine:

```toml
[dependencies]
egui_spellcheck = { version = "0.35.0", default-features = false }
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
cargo run --example spellcheck_demo --features medical-en
```

## Feature flags

| Feature | Default | Description |
| --- | --- | --- |
| `grammar` | Yes | Enables grammar hints through `harper-core`. Disable default features for spelling-only builds. |
| `medical-en` | No | Adds the bundled medical dictionary to every widget. Increases the final binary size. |

Enable medical terminology for an application with:

```toml
[dependencies]
egui_spellcheck = { version = "0.35.0", features = ["medical-en"] }
```

## Startup performance

Building the spelling and (especially) the grammar dictionaries is a one-time
cost that can take a few seconds on the first check. Call
[`prewarm`](https://docs.rs/egui_spellcheck) once at application startup to build
these engines ahead of time so the first check doesn't stall the UI:

```no_run
// Call once at application startup, before entering the event loop.
egui_spellcheck::prewarm();
// ... start your eframe/egui app ...
```

On native targets the warm-up runs on a background thread; on `wasm32` it runs
eagerly on the calling thread (browsers have no background thread to offload
to). The call is idempotent and safe to invoke more than once.

Beyond the one-time warm-up, the recurring per-check work (dominated by the
grammar linter) also runs on a shared background worker thread on native
targets: after you pause typing, the widget dispatches the check off-thread and
keeps showing the previous underlines until the fresh results arrive, so editing
never freezes. On `wasm32` the check runs synchronously on the single browser
thread.

## WebAssembly

The crate is WebAssembly-friendly and requires no network or file access at
runtime. One caveat applies when the default `grammar` feature is enabled: a
transitive dependency (`getrandom`) needs its browser backend selected for
`wasm32-unknown-unknown`. Add the following to the consuming application:

```toml
# Cargo.toml of your wasm app
[target.'cfg(target_arch = "wasm32")'.dependencies]
getrandom = { version = "0.3", features = ["wasm_js"] }
```

and build with the matching backend flag:

```sh
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
  cargo build --target wasm32-unknown-unknown
```

Spelling-only builds (`default-features = false`) do not need this.

## License

The `egui_spellcheck` **source code** is licensed under the
[Mozilla Public License 2.0](https://github.com/stocidlowski/egui_spellcheck/blob/main/LICENSE)
(`MPL-2.0`), a file-level copyleft
license: modifications to MPL-covered files must be shared under the MPL, but
the crate can be combined with proprietary code in a larger work.

## Dictionary data and licensing

The bundled dictionary data files (under
`src/spell_check/dictionaries/`) are **licensed separately** from the crate's
MPL-2.0 code. Full provenance and the applicable licenses are documented in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md):

- **`en_US`** Hunspell dictionary — WordNet 2.1 / SCOWL based; see the included
  license files.
- **`medical`** word list — **generated** from the **UMLS Metathesaurus**. The
  publicly bundled build uses only freely redistributable sources (**RxNorm**,
  **ICD-10-CM**, **HPO**), each with its required attribution. Restrictively
  licensed UMLS sources such as **SNOMED CT** and **MedDRA** are deliberately
  excluded from the published dictionary; they can be added only via the
  generator's `--include-licensed` switch for personal / authorized use, and a
  dictionary built that way must not be redistributed.

## Development

Run the complete local validation suite with:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
```