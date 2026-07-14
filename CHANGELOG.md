# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses a version scheme whose major/minor components track the
supported `egui` release (0.35.x targets egui 0.35), while the patch component
is bumped for this crate's own fixes and features within that egui line.

## [Unreleased]

### Added
- `prewarm()` public helper that builds the spelling and grammar engines ahead
  of time (on a background thread natively, eagerly on `wasm32`) so the first
  check does not stall the UI.
- Bundled, optional medical word list (`medical-en` feature) generated from the
  freely redistributable UMLS Metathesaurus sources (RxNorm, ICD-10-CM, HPO).
- `WebAssembly` and `Startup performance` sections in the README, documenting
  the `getrandom` browser-backend configuration required for wasm builds.
- Project metadata for publishing: MPL-2.0 `LICENSE`, `THIRD_PARTY_NOTICES.md`,
  per-source data license documentation, and `docs.rs` all-features metadata.

### Changed
- Spell/grammar checks now run on a shared background worker thread on native
  targets instead of inline on the UI thread, so the widget no longer freezes
  while re-checking after the user pauses typing (the previous spans stay
  visible until the fresh ones are ready). `wasm32` keeps the synchronous path
  (no background thread in the browser).
- Migrated to `egui` 0.35, including the new `egui::text::ByteIndex` text-layout
  API.
- Grammar checking (`harper-core`) no longer surfaces its own per-word spelling
  lints; spelling is handled solely by the spellbook/medical/personal
  dictionaries, removing false positives on accepted medical terms.
- Corrected licensing after the medical data source changed from LOINC to the
  UMLS Metathesaurus; the source code is licensed under MPL-2.0 and the bundled
  dictionary data is licensed separately.
- Slimmed the published crate: the Python dictionary generator, IDE config, CI
  config, and privately built (licensed) dictionaries are excluded from the
  package.
