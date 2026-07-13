//! # Spell-checking text widget
//!
//! [`SpellCheckTextEdit`] wraps an [`TextEdit`] and underlines misspelled
//! words using the native-Rust, Hunspell-compatible [`spellbook`] crate. It is
//! WASM-friendly (no native FFI, no blocking I/O) and ships an embedded
//! `en_US` dictionary.
//!
//! Per the project's patient-safety principle, the widget *only* flags and
//! suggests; it never autocorrects clinical text. Suggestions are surfaced via
//! a right-click context menu and require an explicit clinician click.
//!
//! Note: `spellbook` performs *spelling* checks only. Optional *grammar*
//! checking is available behind the `grammar` cargo feature, which pulls in the
//! native-Rust [`harper-core`] crate. When enabled, grammar issues are
//! underlined in a distinct (blue) color and their fixes are offered through the
//! same right-click context menu. Like spelling, grammar checking only ever
//! *suggests* — it never rewrites clinical text on its own.
//!
//! The right-click menu also carries the standard editing actions — Cut, Copy
//! and Paste. Cut/Copy are enabled only when text is selected and act on the
//! current selection. Paste is always available and delegates to egui's
//! [`egui::ViewportCommand::RequestPaste`] (the same path as Ctrl+V), so it
//! pulls the real OS clipboard rather than tracking an in-app copy. Right-
//! clicking collapses egui's cursor and drops the field's focus to the menu, so
//! the selection is captured *before* the field processes the click. egui only
//! paints its *own* selection highlight while the field has focus, so to keep
//! the highlight visible while the menu is open we paint it ourselves: the
//! layouter draws a selection-colored background over the captured byte range
//! (see [`apply_selection_background`]), wholly independent of focus. The
//! selection is sampled once when the menu opens and cleared when it closes, so
//! we never re-scan the buffer every frame.
//!
//! TODO: add "Ignore All for session" to context menu underneath ignore
//!

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use egui::text::{ByteIndex, LayoutJob, LayoutSection};
use egui::{Color32, FontSelection, Response, Stroke, TextEdit, TextFormat, Ui};
use serde::{Deserialize, Serialize};
use spellbook::Dictionary;

/// Embedded Hunspell dictionary (`en_US`). Bundled so the checker works offline
/// and inside WASM without any network fetch.
const EN_US_AFF: &str = include_str!("spell_check/dictionaries/en_US.aff");
const EN_US_DIC: &str = include_str!("spell_check/dictionaries/en_US.dic");

/// Supplemental medical word list (drug names, lab/test names, etc.) that the
/// stock `en_US` dictionary doesn't know about. It is compiled only when the
/// `medical` feature is enabled.
#[cfg(feature = "medical-en")]
const MEDICAL_AFF: &str = include_str!("spell_check/dictionaries/medical.aff");
#[cfg(feature = "medical-en")]
const MEDICAL_DIC: &str = include_str!("spell_check/dictionaries/medical.dic");

/// Key under which the per-user personal word list is stored in egui's
/// persisted memory (eframe-backed local storage / on-disk app state). Scoped
/// as a `const` so it's greppable and used consistently.
const PERSONAL_DICTIONARY_KEY: &str = "spellcheck.personal_dictionary";

/// Underline color for misspelled words (egui has no wavy stroke, so a solid
/// underline is used).
const SPELL_UNDERLINE_COLOR: Color32 = Color32::from_rgb(0xD3, 0x2F, 0x2F);
/// Underline color for grammar issues, kept visually distinct from spelling.
#[cfg(feature = "grammar")]
const GRAMMAR_UNDERLINE_COLOR: Color32 = Color32::from_rgb(0x19, 0x76, 0xD2);

/// Lazily built process-wide base dictionary (`en_US`). Building parses the
/// whole word list, so it is done once and cached (see `spellbook` docs: don't
/// build per-frame).
fn dictionary() -> Option<&'static Dictionary> {
    static DICT: OnceLock<Option<Dictionary>> = OnceLock::new();
    DICT.get_or_init(|| match Dictionary::new(EN_US_AFF, EN_US_DIC) {
        Ok(dict) => Some(dict),
        Err(err) => {
            log::error!("failed to load en_US spelling dictionary: {err}");
            None
        }
    })
    .as_ref()
}

/// Lazily built, cached supplemental medical dictionary. Built once on first
/// use, just like the base dictionary.
#[cfg(feature = "medical-en")]
fn medical_dictionary() -> Option<&'static Dictionary> {
    static DICT: OnceLock<Option<Dictionary>> = OnceLock::new();
    DICT.get_or_init(|| match Dictionary::new(MEDICAL_AFF, MEDICAL_DIC) {
        Ok(dict) => Some(dict),
        Err(err) => {
            log::error!("failed to load medical spelling dictionary: {err}");
            None
        }
    })
    .as_ref()
}

/// Per-user list of words the clinician has explicitly accepted ("Add to
/// dictionary"). Persisted via egui's memory, which eframe flushes to local
/// storage on the web (and the on-disk app state natively) when the
/// `persistence` feature is enabled. Words are stored lower-cased so matching is
/// case-insensitive.
#[derive(Clone, Default, Serialize, Deserialize)]
struct PersonalDictionary {
    words: BTreeSet<String>,
}

/// Stable id used to read/write the [`PersonalDictionary`] in egui memory.
fn personal_dictionary_id() -> egui::Id {
    egui::Id::new(PERSONAL_DICTIONARY_KEY)
}

/// Loads the persisted personal dictionary from egui memory (empty if none).
fn load_personal_dictionary(ui: &Ui) -> PersonalDictionary {
    ui.data_mut(|d| d.get_persisted::<PersonalDictionary>(personal_dictionary_id()))
        .unwrap_or_default()
}

/// Permanently adds `word` to the per-user personal dictionary (persisted).
fn add_to_personal_dictionary(ui: &Ui, word: &str) {
    let key = word.to_lowercase();
    ui.data_mut(|d| {
        let mut personal = d
            .get_persisted::<PersonalDictionary>(personal_dictionary_id())
            .unwrap_or_default();
        personal.words.insert(key);
        d.insert_persisted(personal_dictionary_id(), personal);
    });
}

/// Words ignored for the current session only (not persisted). Stored in egui's
/// temp memory so they survive across frames but reset on restart.
fn session_ignored_id() -> egui::Id {
    egui::Id::new("spellcheck.session_ignored")
}

fn load_session_ignored(ui: &Ui) -> HashSet<String> {
    ui.data(|d| d.get_temp::<SessionIgnored>(session_ignored_id()))
        .map(|s| s.0)
        .unwrap_or_default()
}

fn ignore_for_session(ui: &Ui, word: &str) {
    let key = word.to_lowercase();
    ui.data_mut(|d| {
        let mut ignored = d
            .get_temp::<SessionIgnored>(session_ignored_id())
            .unwrap_or_default();
        ignored.0.insert(key);
        d.insert_temp(session_ignored_id(), ignored);
    });
}

/// Newtype wrapper so the session-ignored set has a distinct type in egui's
/// type-keyed temp memory.
#[derive(Clone, Default)]
struct SessionIgnored(HashSet<String>);

/// Selection captured at right-click time and reused while the context menu
/// stays open. Cut/Copy act on `selection` (empty => disabled). Char indices
/// match egui's cursor model; they are converted to byte offsets only when an
/// edit is actually applied. Read once per right-click and cleared when the menu
/// closes, so we never re-scan the buffer per frame.
#[derive(Clone, Default)]
struct ClipboardSnapshot {
    selection: String,
    sel_start: usize,
    sel_end: usize,
}

/// Bundles everything that decides whether a word counts as correct: which
/// dictionaries are enabled plus the user's personal/session word lists. Built
/// once per widget render and threaded through the tokenizer.
struct CheckContext {
    personal: BTreeSet<String>,
    ignored: HashSet<String>,
}

impl CheckContext {
    /// Memoized variant of [`Self::is_correct`]. Looks the word up in `memo`
    /// first and only consults the (potentially expensive, large medical-list)
    /// dictionaries on a miss, then records the result. The memo must be cleared
    /// whenever the context changes (see [`Self::signature`]); callers key it by
    /// that signature. This keeps a post-pause re-check from re-hitting the
    /// dictionary for words it already resolved in a previous pass.
    fn is_correct_memo(&self, word: &str, memo: &mut HashMap<String, bool>) -> bool {
        if let Some(&cached) = memo.get(word) {
            return cached;
        }
        let result = self.is_correct(word);
        memo.insert(word.to_string(), result);
        result
    }

    /// Whether `word` is acceptable under any enabled source. Fails *open* (treats
    /// the word as correct) when the base dictionary can't be loaded, so a load
    /// failure never blocks typing or floods the field with squiggles.
    fn is_correct(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        if self.personal.contains(&lower) || self.ignored.contains(&lower) {
            return true;
        }
        match dictionary() {
            Some(dict) => {
                if dict.check(word) {
                    return true;
                }
            }
            // Base dictionary failed to load: fail-open.
            None => return true,
        }
        #[cfg(feature = "medical-en")]
        if let Some(dict) = medical_dictionary() {
            if dict.check(word) {
                return true;
            }
        }
        false
    }

    /// Best-first suggestions for a misspelled `word`, drawing from the base
    /// dictionary and, when the `medical` feature is enabled, the medical list.
    fn suggestions(&self, word: &str) -> Vec<String> {
        #[cfg(feature = "medical-en")]
        let mut out = suggestions(word);
        #[cfg(not(feature = "medical-en"))]
        let out = suggestions(word);
        #[cfg(feature = "medical-en")]
        if let Some(dict) = medical_dictionary() {
            let mut med = Vec::new();
            dict.suggest(word, &mut med);
            for s in med {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        out
    }

    /// A hash of the context that, combined with the text, decides whether the
    /// cached spans are still valid. Recomputed spans pick up newly added
    /// personal/ignored words.
    fn signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for w in &self.personal {
            w.hash(&mut hasher);
        }
        // BTreeSet for personal is ordered; sort the session set for stability.
        let mut ignored: Vec<&String> = self.ignored.iter().collect();
        ignored.sort();
        for w in ignored {
            w.hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// A single word occurrence within the source text, as a byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WordSpan {
    start: usize,
    end: usize,
}

/// Splits `text` into word spans (maximal runs of alphabetic chars plus inner
/// apostrophes, e.g. `don't`). Numbers and punctuation are skipped so they are
/// never flagged.
fn word_spans(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        let part_of_word = ch.is_alphabetic() || ch == '\'' || ch == '\u{2019}';
        match (part_of_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                push_trimmed(&mut spans, text, s, i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        push_trimmed(&mut spans, text, s, text.len());
    }
    spans
}

/// Pushes the span `[s, e)` after trimming leading/trailing apostrophes so that
/// quotes around a word aren't treated as part of it.
fn push_trimmed(spans: &mut Vec<WordSpan>, text: &str, s: usize, e: usize) {
    let word = &text[s..e];
    let trimmed = word.trim_matches(|c| c == '\'' || c == '\u{2019}');
    if trimmed.is_empty() {
        return;
    }
    let lead = word.len() - word.trim_start_matches(['\'', '\u{2019}']).len();
    spans.push(WordSpan {
        start: s + lead,
        end: s + lead + trimmed.len(),
    });
}

/// Returns the byte ranges of misspelled words in `text` according to `ctx`
/// (the base dictionary, optional feature-selected medical list, and
/// personal/session words). If the base dictionary failed to load,
/// `ctx.is_correct` fails open so nothing is flagged (never block typing).
#[cfg(debug_assertions)]
#[cfg(test)]
fn misspelled_spans(text: &str, ctx: &CheckContext) -> Vec<WordSpan> {
    word_spans(text)
        .into_iter()
        .filter(|span| !ctx.is_correct(&text[span.start..span.end]))
        .collect()
}

/// Like [`misspelled_spans`] but memoizes per-word results in `memo`, so a word
/// that already appeared in this (or a previous, same-context) pass is not
/// re-checked against the dictionaries. This is what keeps the debounced
/// re-check cheap on long notes: only genuinely new tokens cost a lookup.
fn misspelled_spans_memo(
    text: &str,
    ctx: &CheckContext,
    memo: &mut HashMap<String, bool>,
) -> Vec<WordSpan> {
    word_spans(text)
        .into_iter()
        .filter(|span| !ctx.is_correct_memo(&text[span.start..span.end], memo))
        .collect()
}

/// Suggestions for a single (assumed misspelled) word, in best-first order.
pub fn suggestions(word: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(dict) = dictionary() {
        dict.suggest(word, &mut out);
    }
    out
}

/// Whether a single word is spelled correctly. Words unknown to the dictionary
/// loader (e.g., load failure) are reported as correct to avoid false positives.
#[cfg(test)]
fn is_correct(word: &str) -> bool {
    dictionary().is_none_or(|dict| dict.check(word))
}

/// A byte range to underline plus the color to use, so spelling (red) and
/// grammar (blue) issues can share one [`LayoutJob`] builder.
#[derive(Clone, Copy)]
struct ColoredSpan {
    start: usize,
    end: usize,
    color: Color32,
}

/// Builds a [`LayoutJob`] for `text` where misspelled `spans` get a red
/// underline. All other text uses `base` formatting. Thin wrapper over
/// [`build_layout_job_colored`]; retained as a spelling-only helper for the
/// tests (the widget itself goes through [`build_layout_job_colored`]).
#[cfg(test)]
fn build_layout_job(
    text: &str,
    spans: &[WordSpan],
    base: TextFormat,
    wrap_width: f32,
) -> LayoutJob {
    let colored: Vec<ColoredSpan> = spans
        .iter()
        .map(|s| ColoredSpan {
            start: s.start,
            end: s.end,
            color: SPELL_UNDERLINE_COLOR,
        })
        .collect();
    build_layout_job_colored(text, &colored, base, wrap_width)
}

/// Builds a [`LayoutJob`] for `text` underlining each [`ColoredSpan`] in its own
/// color. `spans` must be sorted by `start`; overlapping spans (e.g. a grammar
/// lint covering a misspelled word) are resolved by the same forward-cursor
/// logic that drops stale ranges, so a later overlapping span is skipped rather
/// than slicing mid-codepoint.
fn build_layout_job_colored(
    text: &str,
    spans: &[ColoredSpan],
    base: TextFormat,
    wrap_width: f32,
) -> LayoutJob {
    let len = text.len();
    let mut sections = Vec::with_capacity(spans.len() * 2 + 1);
    let mut cursor = 0usize;
    for span in spans {
        // The cached spans may have been computed against an older version of
        // the text (e.g. while rapidly backspacing the buffer egui hands the
        // layouter can be shorter than the text we last spell-checked). Guard
        // against stale/out-of-bounds ranges so we never slice the galley text
        // outside a valid `char` boundary (which would panic in epaint).
        if span.start < cursor
            || span.end > len
            || span.start >= span.end
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            continue;
        }
        if span.start > cursor {
            sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(cursor)..ByteIndex(span.start),
                format: base.clone(),
            });
        }
        let mut error = base.clone();
        error.underline = Stroke::new(1.5, span.color);
        sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(span.start)..ByteIndex(span.end),
            format: error,
        });
        cursor = span.end;
    }
    if cursor < text.len() {
        sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(cursor)..ByteIndex(text.len()),
            format: base.clone(),
        });
    }

    LayoutJob {
        text: text.to_owned(),
        sections,
        wrap: egui::text::TextWrapping {
            max_width: wrap_width,
            ..Default::default()
        },
        break_on_newline: true,
        ..Default::default()
    }
}

/// Paints a selection-colored `background` behind the `[start, end)` **byte**
/// range of an already-built [`LayoutJob`], splitting sections at the range
/// edges as needed. This is how the user's highlight stays visible while the
/// right-click context menu is open: egui only paints its *own* selection
/// highlight while the field has focus (which the popup steals), so we render
/// the highlight ourselves, wholly independent of focus. Guards against
/// stale/out-of-bounds ranges so we never slice mid-codepoint.
fn apply_selection_background(job: &mut LayoutJob, start: usize, end: usize, color: Color32) {
    let text_len = job.text.len();
    if start >= end
        || end > text_len
        || !job.text.is_char_boundary(start)
        || !job.text.is_char_boundary(end)
    {
        return;
    }
    let mut out: Vec<LayoutSection> = Vec::with_capacity(job.sections.len() + 2);
    for section in job.sections.drain(..) {
        let s = section.byte_range.start.0;
        let e = section.byte_range.end.0;
        // No overlap with the selection: keep the section untouched.
        if e <= start || s >= end {
            out.push(section);
            continue;
        }
        // Part of this section before the selection (plain).
        if s < start {
            out.push(LayoutSection {
                leading_space: section.leading_space,
                byte_range: ByteIndex(s)..ByteIndex(start),
                format: section.format.clone(),
            });
        }
        // Overlapping part: same format but with the selection background.
        let mut sel_format = section.format.clone();
        sel_format.background = color;
        out.push(LayoutSection {
            // `leading_space` only applies to the first sub-section.
            leading_space: if s >= start {
                section.leading_space
            } else {
                0.0
            },
            byte_range: ByteIndex(s.max(start))..ByteIndex(e.min(end)),
            format: sel_format,
        });
        // Part of this section after the selection (plain).
        if e > end {
            out.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(end)..ByteIndex(e),
                format: section.format,
            });
        }
    }
    job.sections = out;
}

/// A grammar issue found by [`harper-core`], expressed in source **byte**
/// offsets (harper reports `char` indices, which we translate up front so the
/// rest of the widget can keep working in bytes like the spelling path). Carries
/// the human-readable message and any whole-text replacement suggestions.
#[cfg(feature = "grammar")]
#[derive(Clone, Debug, Default)]
struct GrammarSpan {
    start: usize,
    end: usize,
    message: String,
    suggestions: Vec<String>,
}

/// Runs harper's curated grammar linter over `text` and returns the issues as
/// byte-range [`GrammarSpan`]s. Built fresh on each (debounced) pass rather than
/// cached in a `static`: with harper's default (non-`concurrent`) feature set
/// the linter is `!Send`/`!Sync`, and the curated dictionary it needs is already
/// memoized inside harper, so re-creating the `LintGroup` is cheap.
///
/// Mirrors the spelling path's safety contract: this only *flags* and offers
/// suggestions; it never edits the text. Only `ReplaceWith` suggestions are
/// surfaced (a concrete alternative the clinician can click); `Remove`/
/// `InsertAfter` fixes are intentionally left out to keep the menu unambiguous.
///
/// harper's curated set includes its own per-word spell checker
/// ([`LintKind::Spelling`]), which uses a general-English dictionary that does
/// not know the medical vocabulary our spellbook/medical dictionaries accept
/// (e.g. it flags `Quantiferon` with "Did you mean to spell ... this way?").
/// Spelling is entirely the responsibility of the spelling path here, so we drop
/// harper's `Spelling`-kind lints to avoid double-checking and false positives
/// on words our dictionaries already treat as correct.
#[cfg(feature = "grammar")]
fn grammar_spans(text: &str) -> Vec<GrammarSpan> {
    use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
    use harper_core::parsers::PlainEnglish;
    use harper_core::spell::FstDictionary;
    use harper_core::{Dialect, Document};

    if text.is_empty() {
        return Vec::new();
    }

    let document = Document::new_curated(text, &PlainEnglish);
    let dictionary = FstDictionary::curated();
    let mut linter = LintGroup::new_curated(dictionary, Dialect::American);
    let lints = linter.lint(&document);

    // harper reports spans as `char` indices; build a char-index -> byte-offset
    // table once so we can translate every lint without rescanning the string.
    let char_to_byte: Vec<usize> = text.char_indices().map(|(byte, _)| byte).collect();
    let byte_at = |char_idx: usize| -> Option<usize> {
        if char_idx == char_to_byte.len() {
            Some(text.len())
        } else {
            char_to_byte.get(char_idx).copied()
        }
    };

    let mut spans = Vec::new();
    for lint in lints {
        // Spelling is handled by the spelling path (spellbook + medical/personal
        // dictionaries); skip harper's own per-word spell checker so it can't
        // flag medical terms our dictionaries already accept.
        if lint.lint_kind == LintKind::Spelling {
            continue;
        }
        let (Some(start), Some(end)) = (byte_at(lint.span.start), byte_at(lint.span.end)) else {
            // Out-of-range span (stale/odd lint); skip rather than risk a panic.
            continue;
        };
        if start >= end {
            continue;
        }
        let suggestions = lint
            .suggestions
            .iter()
            .filter_map(|suggestion| match suggestion {
                Suggestion::ReplaceWith(chars) => Some(chars.iter().collect::<String>()),
                Suggestion::InsertAfter(_) | Suggestion::Remove => None,
            })
            .collect();
        spans.push(GrammarSpan {
            start,
            end,
            message: lint.message,
            suggestions,
        });
    }
    spans
}

/// How long (seconds) the user must pause typing before the (potentially
/// expensive) spell check re-runs. Large word lists make a full re-check on
/// every keystroke noticeable, so we debounce: the previous spans keep showing
/// while typing and the recheck only fires once the field goes idle.
const SPELLCHECK_DEBOUNCE_SECS: f64 = 0.4;

/// Per-id cache of the misspelled spans for a given text, so spellchecking only
/// runs when the inputs actually change (not every frame) and only after the
/// user pauses typing. The cache key mixes the text with the
/// [`CheckContext::signature`] so adding a personal/ignored word re-flags the
/// field once the debounce window elapses.
#[derive(Clone, Default)]
struct SpellCache {
    /// Key (text + context) that `spans` were last computed for.
    key: u64,
    spans: Vec<WordSpan>,
    /// Key currently awaiting a debounced recompute. May differ from `key`
    /// while the user is mid-edit; equals `key` once the check has caught up.
    pending_key: u64,
    /// Time (egui `input.time`, seconds) at which `pending_key` was first seen.
    /// The recheck runs once `now - pending_since >= SPELLCHECK_DEBOUNCE_SECS`.
    pending_since: f64,
    /// Context signature the `memo` results are valid for. When the context
    /// changes (a personal/ignored word added) the memo is cleared so stale
    /// per-word verdicts don't linger.
    memo_signature: u64,
    /// Memoized per-word check results, reused across debounced re-checks so an
    /// idle recompute only pays for newly typed words rather than re-scanning
    /// (and re-`check`-ing against the large medical list) the whole buffer.
    memo: HashMap<String, bool>,
    /// Grammar issues for the cached text, recomputed alongside `spans` on each
    /// debounced pass when the `grammar` feature is enabled.
    #[cfg(feature = "grammar")]
    grammar_spans: Vec<GrammarSpan>,
}

fn hash_text(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Combines the text hash with the check-context signature into the cache key.
/// Takes an already-computed context signature so the caller can reuse it (e.g.
/// to also key the per-word memo) instead of hashing the context twice.
fn cache_key_with_signature(text: &str, signature: u64) -> u64 {
    hash_text(text) ^ signature.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// A [`TextEdit`]-like widget that underlines misspelled words and offers
/// spelling suggestions through a right-click context menu.
///
/// Usage mirrors `egui::TextEdit`:
/// ```no_run
/// use egui_spellcheck::SpellCheckTextEdit;
///
/// let mut note = String::new();
/// let _edit = SpellCheckTextEdit::multiline(&mut note)
///     .desired_width(f32::INFINITY)
///     .hint_text("Note");
/// ```
pub struct SpellCheckTextEdit<'t> {
    text: &'t mut String,
    multiline: bool,
    hint_text: Option<String>,
    desired_width: Option<f32>,
    desired_rows: Option<usize>,
    font_id: Option<egui::FontId>,
    id_salt: Option<egui::Id>,
    interactive: bool,
}

impl<'t> SpellCheckTextEdit<'t> {
    /// Enable or disable user interaction while retaining spell-check styling.
    #[must_use]
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }
}

impl<'t> SpellCheckTextEdit<'t> {
    /// A single-line spellchecked text field.
    pub fn singleline(text: &'t mut String) -> Self {
        Self::new(text, false)
    }

    /// A multi-line spellchecked text field.
    pub fn multiline(text: &'t mut String) -> Self {
        Self::new(text, true)
    }

    fn new(text: &'t mut String, multiline: bool) -> Self {
        Self {
            text,
            multiline,
            hint_text: None,
            desired_width: None,
            desired_rows: None,
            font_id: None,
            id_salt: None,
            interactive: true,
        }
    }

    /// Placeholder text shown when empty.
    #[must_use]
    pub fn hint_text(mut self, hint: impl Into<String>) -> Self {
        self.hint_text = Some(hint.into());
        self
    }

    /// Desired width (e.g. `f32::INFINITY` to fill the available space).
    #[must_use]
    pub fn desired_width(mut self, width: f32) -> Self {
        self.desired_width = Some(width);
        self
    }

    /// Desired number of visible rows (multiline only).
    #[must_use]
    pub fn desired_rows(mut self, rows: usize) -> Self {
        self.desired_rows = Some(rows);
        self
    }

    /// Override the font used to render the text.
    #[must_use]
    pub fn font(mut self, font_id: egui::FontId) -> Self {
        self.font_id = Some(font_id);
        self
    }

    /// Stable id salt; needed when several widgets share the same parent id.
    #[must_use]
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(egui::Id::new(salt));
        self
    }

    /// Render the widget and return the full [`TextEditOutput`], mirroring
    /// [`egui::TextEdit::show`]. Callers that only need the [`Response`] can use
    /// the [`egui::Widget`] impl below (or read `.response`), while collaborative
    /// callers can reach the laid-out `galley`, its screen `galley_pos`, and the
    /// `cursor_range` to paint peer carets and report the local caret.
    ///
    /// [`TextEditOutput`]: egui::text_edit::TextEditOutput
    pub fn show(self, ui: &mut Ui) -> egui::text_edit::TextEditOutput {
        let id = self
            .id_salt
            .map_or_else(|| ui.next_auto_id(), |salt| ui.make_persistent_id(salt));

        // Assemble the check context (base dictionary + feature-selected
        // medical list + the user's persisted personal words + this session's
        // ignored words).
        let ctx = CheckContext {
            personal: load_personal_dictionary(ui).words,
            ignored: load_session_ignored(ui),
        };

        // Recompute misspelled spans only when the text or context changed, and
        // only after the user has paused typing (debounce). While the field is
        // being edited we keep showing the previously computed spans; the
        // stale-span guards below (and in `build_layout_job`) keep slicing safe
        // even though those spans were computed against the older text.
        let ctx_signature = ctx.signature();
        let current_key = cache_key_with_signature(self.text, ctx_signature);
        let now = ui.input(|i| i.time);
        let mut cache: SpellCache = ui
            .data(|d| d.get_temp::<SpellCache>(id))
            .unwrap_or_default();
        // Drop memoized per-word verdicts whenever the context changes, since a
        // word's correctness depends on the enabled dictionaries / word lists.
        if cache.memo_signature != ctx_signature {
            cache.memo.clear();
            cache.memo_signature = ctx_signature;
        }
        if cache.key != current_key {
            // The text/context differs from what we last checked. (Re)start the
            // idle window whenever the pending target changes.
            if cache.pending_key != current_key {
                cache.pending_key = current_key;
                cache.pending_since = now;
                ui.data_mut(|d| d.insert_temp(id, cache.clone()));
            }
            let elapsed = now - cache.pending_since;
            if elapsed >= SPELLCHECK_DEBOUNCE_SECS {
                cache.key = current_key;
                cache.spans = misspelled_spans_memo(self.text, &ctx, &mut cache.memo);
                #[cfg(feature = "grammar")]
                {
                    cache.grammar_spans = grammar_spans(self.text);
                }
                ui.data_mut(|d| d.insert_temp(id, cache.clone()));
            } else {
                // Wake up to run the deferred check once the field goes idle.
                let remaining = SPELLCHECK_DEBOUNCE_SECS - elapsed;
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(remaining));
            }
        }

        // Base format derived from the requested (or default) font + color.
        let font_id = self
            .font_id
            .clone()
            .unwrap_or_else(|| FontSelection::Default.resolve(ui.style()));
        let text_color = ui.visuals().text_color();
        let base_format = TextFormat {
            font_id: font_id.clone(),
            color: text_color,
            ..Default::default()
        };

        // Merge spelling (red) and, when enabled, grammar (blue) issues into one
        // color-tagged, start-sorted list for the layouter. Sorting lets the
        // builder's forward cursor drop a later span that overlaps an earlier
        // one (e.g. a grammar lint covering a misspelled word).
        let spelling = cache.spans.iter().map(|s| ColoredSpan {
            start: s.start,
            end: s.end,
            color: SPELL_UNDERLINE_COLOR,
        });
        #[cfg(feature = "grammar")]
        let colored: Vec<ColoredSpan> = {
            let mut colored: Vec<ColoredSpan> = spelling.collect();
            colored.extend(cache.grammar_spans.iter().map(|g| ColoredSpan {
                start: g.start,
                end: g.end,
                color: GRAMMAR_UNDERLINE_COLOR,
            }));
            colored.sort_by_key(|s| s.start);
            colored
        };
        #[cfg(not(feature = "grammar"))]
        let colored: Vec<ColoredSpan> = spelling.collect();

        // Temp-memory key for the selection snapshot captured on the last
        // right-click (cleared when the menu closes). Defined here so we can read
        // it *before* `show`.
        let clipboard_id = id.with("clipboard_snapshot");

        // Keep the user's highlighted text visibly selected while a context menu
        // opened by an earlier right-click is still showing. egui only paints its
        // own selection highlight when the field has focus *and* its stored cursor
        // holds a range, but a right-click collapses the cursor to the click point
        // and the just-opened popup steals the field's focus, so egui stops
        // painting it -- the long-standing "right-click clears the highlight" bug.
        // Rather than fighting egui's focus model (which proved fragile), we paint
        // the highlight ourselves: the layouter below sets a selection-colored
        // background on the snapshot's byte range, which is wholly independent of
        // focus. Only do so while the snapshot still matches the live buffer, so we
        // never highlight text that has since shifted.
        let menu_selection_bytes: Option<(usize, usize)> = {
            let snap: ClipboardSnapshot = ui.data(|d| d.get_temp(clipboard_id)).unwrap_or_default();
            if snap.sel_end > snap.sel_start
                && char_slice(self.text, snap.sel_start, snap.sel_end).as_deref()
                    == Some(snap.selection.as_str())
            {
                let b_start = self
                    .text
                    .char_indices()
                    .nth(snap.sel_start)
                    .map_or(self.text.len(), |(b, _)| b);
                let b_end = self
                    .text
                    .char_indices()
                    .nth(snap.sel_end)
                    .map_or(self.text.len(), |(b, _)| b);
                Some((b_start, b_end))
            } else {
                None
            }
        };
        let selection_bg_color = ui.visuals().selection.bg_fill;
        let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job =
                build_layout_job_colored(buf.as_str(), &colored, base_format.clone(), wrap_width);
            if let Some((s, e)) = menu_selection_bytes {
                apply_selection_background(&mut job, s, e, selection_bg_color);
            }
            ui.fonts_mut(|f| f.layout_job(job))
        };

        // Capture the selection as it stood *before* egui processes this frame's
        // events: a right-click (indeed any pointer press) collapses the cursor to
        // the click point *inside* `TextEdit::show`, so by the time we read
        // `output` the user's highlight is already gone. Reading the stored state
        // up front (before `text_edit` mutably borrows `self.text`) lets us
        // preserve that selection for Cut/Copy and restore it while the menu is
        // open.
        let prev_selection: Option<(usize, usize)> =
            egui::widgets::text_edit::TextEditState::load(ui.ctx(), id)
                .and_then(|s| s.cursor.char_range())
                .map(|r| {
                    let r = r.as_sorted_char_range();
                    (r.start.into(), r.end.into())
                });

        let mut text_edit = if self.multiline {
            TextEdit::multiline(self.text)
        } else {
            TextEdit::singleline(self.text)
        }
        .id(id)
        .font(font_id)
        // Mirror `egui::TextEdit`: when not interactive the field is rendered as
        // read-only (hover-only sense, no caret/selection editing), which is how
        // the collab panel shows sections the local user may not edit.
        .interactive(self.interactive)
        .layouter(&mut layouter);

        if let Some(hint) = self.hint_text {
            text_edit = text_edit.hint_text(hint);
        }
        if let Some(width) = self.desired_width {
            text_edit = text_edit.desired_width(width);
        }
        if let Some(rows) = self.desired_rows {
            text_edit = text_edit.desired_rows(rows);
        }

        // Use `show` (not `ui.add`) so we get the laid-out galley and its screen
        // position, which lets us map a right-click to the exact word under the
        // cursor.
        let output = text_edit.show(ui);
        // Keep a cheap handle to the inner `Response` for the right-click /
        // context-menu handling below; the full `output` is returned at the end
        // so callers get the galley, caret, etc. `output.response` is an
        // `AtomLayoutResponse`, so reach through to its inner `Response`.
        let response = output.response.response.clone();

        // egui may have mutated `self.text` during `show` (e.g. the user just
        // backspaced), so the cached spans — computed at the start of the frame
        // against the previous, longer text — can now point past the end of the
        // buffer or land mid-codepoint. Guard every slice so we never panic
        // (see `build_layout_job` for the same rationale).
        let len = self.text.len();
        let span_valid = |s: &WordSpan| {
            s.start < s.end
                && s.end <= len
                && self.text.is_char_boundary(s.start)
                && self.text.is_char_boundary(s.end)
        };

        // On a right-click, find the misspelled word under the pointer so the
        // context menu can target only that word (no word-picker submenu). The
        // resolved word/span is stored in temp memory because the context menu
        // stays open across frames.
        let menu_id = id.with("spell_clicked_word");
        if response.secondary_clicked() {
            let clicked = response.interact_pointer_pos().and_then(|pos| {
                let ccursor = output.galley.cursor_from_pos(pos - output.galley_pos);
                // `CCursor::index` is a char index; convert to a byte offset.
                let byte = self
                    .text
                    .char_indices()
                    .nth(ccursor.index.into())
                    .map_or(len, |(b, _)| b);
                cache
                    .spans
                    .iter()
                    .find(|s| span_valid(s) && s.start <= byte && byte <= s.end)
                    .map(|s| ClickedWord {
                        word: self.text[s.start..s.end].to_owned(),
                        start: s.start,
                        end: s.end,
                    })
            });
            ui.data_mut(|d| d.insert_temp(menu_id, clicked.unwrap_or_default()));

            // Snapshot the selection *once*, now, while the menu opens, from the
            // selection captured *before* show (egui collapses the cursor on the
            // right-click itself). Caching here — cleared when the menu closes —
            // avoids re-reading the buffer every frame. Paste no longer needs any
            // snapshot: it delegates straight to the OS clipboard via
            // `ViewportCommand::RequestPaste`.
            let char_len = self.text.chars().count();
            // A right-click collapses egui's cursor to the click point inside
            // `show`, so `output.state` no longer holds the user's highlight. Use
            // the selection captured *before* show for Cut/Copy.
            let (sel_start, sel_end) = match prev_selection {
                Some((s, e)) if e > s => (s.min(char_len), e.min(char_len)),
                _ => (char_len, char_len),
            };
            let selection: String = self
                .text
                .chars()
                .skip(sel_start)
                .take(sel_end.saturating_sub(sel_start))
                .collect();
            let snapshot = ClipboardSnapshot {
                selection,
                sel_start,
                sel_end,
            };
            ui.data_mut(|d| d.insert_temp(clipboard_id, snapshot));
        }

        // Grammar issues get their own resolved-on-click entry. Spelling takes
        // priority when both land on the same byte (a misspelled word is the
        // more concrete fix), so the grammar menu below only opens when no
        // misspelled word was clicked.
        #[cfg(feature = "grammar")]
        let grammar_menu_id = id.with("grammar_clicked_word");
        #[cfg(feature = "grammar")]
        if response.secondary_clicked() {
            let clicked_grammar = response.interact_pointer_pos().and_then(|pos| {
                let ccursor = output.galley.cursor_from_pos(pos - output.galley_pos);
                let byte = self
                    .text
                    .char_indices()
                    .nth(ccursor.index.into())
                    .map_or(len, |(b, _)| b);
                cache
                    .grammar_spans
                    .iter()
                    .find(|g| {
                        g.start < g.end
                            && g.end <= len
                            && self.text.is_char_boundary(g.start)
                            && self.text.is_char_boundary(g.end)
                            && g.start <= byte
                            && byte <= g.end
                    })
                    .map(|g| ClickedGrammar {
                        start: g.start,
                        end: g.end,
                        original: self.text[g.start..g.end].to_owned(),
                        message: g.message.clone(),
                        suggestions: g.suggestions.clone(),
                    })
            });
            ui.data_mut(|d| d.insert_temp(grammar_menu_id, clicked_grammar.unwrap_or_default()));
        }

        let clicked: ClickedWord = ui.data(|d| d.get_temp(menu_id)).unwrap_or_default();
        #[cfg(feature = "grammar")]
        let grammar: ClickedGrammar = ui.data(|d| d.get_temp(grammar_menu_id)).unwrap_or_default();
        let snapshot: ClipboardSnapshot = ui.data(|d| d.get_temp(clipboard_id)).unwrap_or_default();

        // Spelling suggestions are computed (and cached) only when a misspelled
        // word was actually clicked. They draw from the base and (when opted in)
        // medical dictionary, so we compute them outside the menu closure to
        // avoid borrowing `ctx` there. The cache is keyed by the clicked word +
        // context signature because the menu stays open across frames and
        // `dict.suggest` against the large medical list is expensive enough that
        // recomputing it every frame drops the UI to a few fps.
        let sugg = if clicked.word.is_empty() {
            Vec::new()
        } else {
            let sugg_id = id.with("spell_suggestions");
            let mut cached_sugg: CachedSuggestions =
                ui.data(|d| d.get_temp(sugg_id)).unwrap_or_default();
            if cached_sugg.word != clicked.word || cached_sugg.signature != ctx_signature {
                cached_sugg = CachedSuggestions {
                    word: clicked.word.clone(),
                    signature: ctx_signature,
                    suggestions: ctx.suggestions(&clicked.word),
                };
                ui.data_mut(|d| d.insert_temp(sugg_id, cached_sugg.clone()));
            }
            cached_sugg.suggestions
        };

        // A single right-click menu carries every action: spelling suggestions /
        // grammar fixes for the issue under the pointer (if any), followed by the
        // standard Cut/Copy/Paste clipboard items (always present, enabled per
        // selection/clipboard state). egui's context menu defaults to a narrow,
        // button-sized width which truncates long suggestions; give it a sensible
        // minimum.
        const MENU_MIN_WIDTH: f32 = 220.0;
        let menu = response.clone().context_menu(|ui| {
            ui.set_min_width(MENU_MIN_WIDTH);
            let mut shown_issue = false;

            // Spelling suggestions + dictionary actions for the clicked word.
            if !clicked.word.is_empty() {
                shown_issue = true;
                if sugg.is_empty() {
                    ui.label("(no suggestions)");
                }
                let mut replacement: Option<String> = None;
                for s in sugg.iter().take(8) {
                    // Stretch each suggestion button to the full menu width so the
                    // whole row is clickable and aligned.
                    if ui
                        .add(egui::Button::new(s).min_size(egui::vec2(ui.available_width(), 0.0)))
                        .clicked()
                    {
                        replacement = Some(s.clone());
                        ui.close();
                    }
                }
                if let Some(to) = replacement {
                    // Prefer replacing the exact clicked span when it's still
                    // valid and unchanged; otherwise fall back to the first match.
                    let cur_len = self.text.len();
                    if clicked.end <= cur_len
                        && self.text.is_char_boundary(clicked.start)
                        && self.text.is_char_boundary(clicked.end)
                        && self.text.get(clicked.start..clicked.end) == Some(clicked.word.as_str())
                    {
                        *self.text = replace_span(self.text, clicked.start, clicked.end, &to);
                    } else {
                        *self.text = replace_first_word(self.text, &clicked.word, &to);
                    }
                }

                // Dictionary actions. These never change the clinical text — they
                // only teach the checker to stop flagging this word — so they
                // stay within the "suggest-only, never autocorrect" patient-safety
                // contract. The cache is keyed by the context signature, so the
                // squiggle disappears on the next frame.
                ui.separator();
                if ui
                    .add(
                        egui::Button::new(format!("Add “{}” to dictionary", clicked.word))
                            .min_size(egui::vec2(ui.available_width(), 0.0)),
                    )
                    .clicked()
                {
                    add_to_personal_dictionary(ui, &clicked.word);
                    ui.close();
                }
                if ui
                    .add(
                        egui::Button::new("Ignore for this session")
                            .min_size(egui::vec2(ui.available_width(), 0.0)),
                    )
                    .clicked()
                {
                    ignore_for_session(ui, &clicked.word);
                    ui.close();
                }
            }

            // Grammar message + replacement fixes, but only when no misspelled
            // word was clicked (spelling is the more concrete fix on overlap).
            #[cfg(feature = "grammar")]
            if clicked.word.is_empty() && !grammar.original.is_empty() {
                shown_issue = true;
                ui.label(&grammar.message);
                ui.separator();
                if grammar.suggestions.is_empty() {
                    ui.label("(no suggestions)");
                }
                let mut replacement: Option<String> = None;
                for s in grammar.suggestions.iter().take(8) {
                    let label = if s.is_empty() { "(remove)" } else { s.as_str() };
                    if ui
                        .add(
                            egui::Button::new(label)
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        )
                        .clicked()
                    {
                        replacement = Some(s.clone());
                        ui.close();
                    }
                }
                // Apply only when the clicked span is still intact (the user
                // hasn't edited over it). Never autocorrect on its own.
                if let Some(to) = replacement {
                    let cur_len = self.text.len();
                    if grammar.end <= cur_len
                        && self.text.is_char_boundary(grammar.start)
                        && self.text.is_char_boundary(grammar.end)
                        && self.text.get(grammar.start..grammar.end)
                            == Some(grammar.original.as_str())
                    {
                        *self.text = replace_span(self.text, grammar.start, grammar.end, &to);
                    }
                }
            }

            // Standard clipboard actions, available on every right-click. Add a
            // separator only when an issue section precedes them.
            if shown_issue {
                ui.separator();
            }
            add_clipboard_items(ui, self.text, &snapshot, id);
        });

        // Per the clipboard design, the selection/clipboard snapshot is read once
        // on right-click and dropped as soon as the menu closes, so the next
        // right-click re-reads fresh state instead of reusing stale data.
        if menu.is_none() {
            ui.data_mut(|d| d.remove::<ClipboardSnapshot>(clipboard_id));
        }

        output
    }
}

impl egui::Widget for SpellCheckTextEdit<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response.response
    }
}

/// The misspelled word resolved from a right-click position, stored in temp
/// memory so the (persistent) context menu can target only that word.
#[derive(Clone, Default)]
struct ClickedWord {
    word: String,
    start: usize,
    end: usize,
}

/// The grammar issue resolved from a right-click position, stored in temp memory
/// so the (persistent) context menu can show its message and replacement fixes
/// for that span only. `original` is the text the span covered when clicked, used
/// to confirm the buffer hasn't shifted before applying a fix.
#[cfg(feature = "grammar")]
#[derive(Clone, Default)]
struct ClickedGrammar {
    start: usize,
    end: usize,
    original: String,
    message: String,
    suggestions: Vec<String>,
}

/// Cached spelling suggestions for the word under the (persistent) context menu.
/// Stored in temp memory so the expensive `dict.suggest` call (especially
/// against the large medical list) runs once when the menu opens rather than on
/// every frame the menu stays open. Recomputed only when the targeted word or
/// the check context changes.
#[derive(Clone, Default)]
struct CachedSuggestions {
    /// Word the `suggestions` were computed for.
    word: String,
    /// Context signature (see [`CheckContext::signature`]) the suggestions are
    /// valid for; invalidates the cache when the enabled dictionaries change.
    signature: u64,
    suggestions: Vec<String>,
}

/// Replaces the byte range `[start, end)` of `text` with `to`.
fn replace_span(text: &str, start: usize, end: usize, to: &str) -> String {
    let mut out = String::with_capacity(text.len() - (end - start) + to.len());
    out.push_str(&text[..start]);
    out.push_str(to);
    out.push_str(&text[end..]);
    out
}

/// Replaces the first whole-word occurrence of `from` with `to`, preserving the
/// surrounding text. Used by the suggestion context menu.
fn replace_first_word(text: &str, from: &str, to: &str) -> String {
    for span in word_spans(text) {
        if &text[span.start..span.end] == from {
            let mut out = String::with_capacity(text.len() - from.len() + to.len());
            out.push_str(&text[..span.start]);
            out.push_str(to);
            out.push_str(&text[span.end..]);
            return out;
        }
    }
    text.to_owned()
}

/// Replaces the `[start, end)` *char* range of `text` with `ins`, returning the
/// new string and the caret char index just past the inserted text. Clipboard
/// Cut/Paste work in char indices (egui's cursor model) and convert to byte
/// offsets only here, at the moment of the edit.
fn replace_char_range(text: &str, start: usize, end: usize, ins: &str) -> (String, usize) {
    let offsets: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    let bs = offsets.get(start).copied().unwrap_or(text.len());
    let be = offsets.get(end).copied().unwrap_or(text.len());
    let mut out = String::with_capacity(text.len() - (be - bs) + ins.len());
    out.push_str(&text[..bs]);
    out.push_str(ins);
    out.push_str(&text[be..]);
    (out, start + ins.chars().count())
}

/// Returns the `[start, end)` char-range substring of `text`, or `None` when the
/// range is out of bounds. Lets clipboard actions confirm the snapshot still
/// matches the live buffer before mutating it (never edit text that shifted).
fn char_slice(text: &str, start: usize, end: usize) -> Option<String> {
    if start > end || end > text.chars().count() {
        return None;
    }
    Some(text.chars().skip(start).take(end - start).collect())
}

/// Moves the text field's caret to a collapsed cursor at `char_index` by editing
/// the stored [`egui::TextEditState`]. Called after a Cut/Paste mutates the
/// buffer so the caret lands where the user expects.
fn set_caret(ui: &Ui, id: egui::Id, char_index: usize) {
    if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), id) {
        let cursor = egui::text::CCursor::new(char_index);
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
        state.store(ui.ctx(), id);
    }
}

/// Appends the standard Cut/Copy/Paste items to a context-menu `ui`. Cut/Copy
/// are enabled only when text is selected (see [`ClipboardSnapshot`]) and write
/// the selection to the OS clipboard (`copy_text`); Cut also deletes it and
/// repositions the caret, re-checking the snapshot against the live buffer first
/// so a buffer that shifted since the menu opened is left untouched (keeping the
/// "never silently mangle clinical text" contract). Paste is always enabled and
/// delegates to egui's [`egui::ViewportCommand::RequestPaste`] — the same path
/// as Ctrl+V — which pastes the real OS clipboard into the focused field. egui
/// exposes no synchronous clipboard *read*, so there is nothing to inspect up
/// front; we simply re-focus the field and let the integration deliver the
/// paste.
fn add_clipboard_items(ui: &mut Ui, text: &mut String, snap: &ClipboardSnapshot, id: egui::Id) {
    let has_selection = !snap.selection.is_empty();

    let cut = ui
        .add_enabled(
            has_selection,
            egui::Button::new("Cut").min_size(egui::vec2(ui.available_width(), 0.0)),
        )
        .clicked();
    if cut {
        if char_slice(text, snap.sel_start, snap.sel_end).as_deref()
            == Some(snap.selection.as_str())
        {
            ui.ctx().copy_text(snap.selection.clone());
            let (new_text, caret) = replace_char_range(text, snap.sel_start, snap.sel_end, "");
            *text = new_text;
            set_caret(ui, id, caret);
        }
        ui.close();
    }

    let copy = ui
        .add_enabled(
            has_selection,
            egui::Button::new("Copy").min_size(egui::vec2(ui.available_width(), 0.0)),
        )
        .clicked();
    if copy {
        ui.ctx().copy_text(snap.selection.clone());
        ui.close();
    }

    let paste = ui
        .add(egui::Button::new("Paste").min_size(egui::vec2(ui.available_width(), 0.0)))
        .clicked();
    if paste {
        // egui offers no synchronous OS-clipboard read (and none at all on WASM),
        // so instead of tracking our own copy we ask the integration to paste the
        // real clipboard into the focused field — exactly what Ctrl+V does. Keep
        // the field focused so the resulting `Event::Paste` lands here next frame.
        ui.memory_mut(|mem| mem.request_focus(id));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A check context with no personal/session words.
    fn ctx() -> CheckContext {
        CheckContext {
            personal: BTreeSet::new(),
            ignored: HashSet::new(),
        }
    }

    #[test]
    fn dictionary_loads() {
        assert!(
            dictionary().is_some(),
            "embedded en_US dictionary must load"
        );
    }

    #[cfg(feature = "medical-en")]
    #[test]
    fn medical_dictionary_loads() {
        assert!(
            medical_dictionary().is_some(),
            "embedded medical dictionary must load"
        );
    }

    #[test]
    fn known_words_are_correct() {
        assert!(is_correct("hello"));
        assert!(is_correct("world"));
        assert!(is_correct("patient"));
    }

    #[test]
    fn misspelled_word_is_flagged() {
        assert!(!is_correct("teh"));
        assert!(!is_correct("recieve"));
    }

    #[test]
    fn word_spans_skip_numbers_and_punctuation() {
        let text = "Take 2 tablets, twice-daily.";
        let words: Vec<&str> = word_spans(text)
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect();
        assert_eq!(words, vec!["Take", "tablets", "twice", "daily"]);
    }

    #[test]
    fn apostrophes_kept_inside_words() {
        let text = "don't 'quoted'";
        let words: Vec<&str> = word_spans(text)
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect();
        assert_eq!(words, vec!["don't", "quoted"]);
    }

    #[test]
    fn misspelled_spans_point_at_the_bad_word() {
        let text = "the quik brown fox";
        let spans = misspelled_spans(text, &ctx());
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].start..spans[0].end], "quik");
    }

    #[cfg(feature = "medical-en")]
    #[test]
    fn medical_feature_accepts_medical_words_without_widget_configuration() {
        // Derive the test word from the embedded medical list rather than
        // hardcoding one: the bundled `.dic` is curated/pruned out-of-band, so a
        // hardcoded sample word can disappear. We pick a real entry the stock
        // en_US dictionary rejects and assert that the feature makes it valid
        // for every widget without per-widget configuration.
        let base = dictionary().expect("en_US dictionary must load");
        let word = MEDICAL_DIC
            .lines()
            .skip(1) // first line is the Hunspell word count
            .map(|line| line.split('/').next().unwrap_or(line).trim()) // drop affix flags
            .find(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic()) && !base.check(w))
            .expect("medical list should contain a word the base dictionary rejects");

        assert!(
            ctx().is_correct(word),
            "{word:?} should be accepted whenever the medical feature is enabled"
        );
    }

    #[test]
    fn personal_words_are_accepted_case_insensitively() {
        let mut c = ctx();
        c.personal.insert("zztestdrug".to_string());
        assert!(c.is_correct("zztestdrug"));
        // Stored lower-cased; matching ignores case.
        assert!(c.is_correct("ZzTestDrug"));
        // A different unknown word is still flagged.
        assert!(!c.is_correct("zzotherdrug"));
    }

    #[test]
    fn session_ignored_words_are_accepted() {
        let mut c = ctx();
        c.ignored.insert("zzignoreme".to_string());
        assert!(c.is_correct("zzignoreme"));
        assert!(c.is_correct("ZZIGNOREME"));
    }

    #[test]
    fn context_signature_changes_with_added_words() {
        let base = ctx().signature();
        let mut c = ctx();
        c.personal.insert("newword".to_string());
        assert_ne!(
            base,
            c.signature(),
            "adding a word must invalidate the span cache"
        );
    }

    #[test]
    fn suggestions_offered_for_typo() {
        let sugg = suggestions("recieve");
        assert!(
            sugg.iter().any(|s| s == "receive"),
            "expected 'receive' among suggestions, got {sugg:?}"
        );
    }

    #[test]
    fn memoized_check_matches_uncached() {
        // The memoized path must agree with the direct check and populate the
        // memo so repeat lookups don't re-hit the dictionary.
        let c = ctx();
        let mut memo = HashMap::new();
        assert_eq!(c.is_correct_memo("hello", &mut memo), c.is_correct("hello"));
        assert_eq!(c.is_correct_memo("teh", &mut memo), c.is_correct("teh"));
        assert_eq!(memo.get("hello"), Some(&true));
        assert_eq!(memo.get("teh"), Some(&false));
        // A second call returns the cached verdict (same result).
        assert!(c.is_correct_memo("hello", &mut memo));
        assert!(!c.is_correct_memo("teh", &mut memo));
    }

    #[test]
    fn memoized_spans_match_unmemoized() {
        let text = "the quik brown fax";
        let c = ctx();
        let mut memo = HashMap::new();
        let plain = misspelled_spans(text, &c);
        let memoized = misspelled_spans_memo(text, &c, &mut memo);
        assert_eq!(plain, memoized);
        // Every distinct word is recorded once in the memo.
        assert_eq!(memo.get("quik"), Some(&false));
        assert_eq!(memo.get("the"), Some(&true));
    }

    #[test]
    fn memo_returns_cached_value_without_rechecking() {
        // A deliberately wrong memo entry is honored, proving the memo (not the
        // dictionary) is consulted on a hit — i.e. repeat words are not
        // re-checked after the cache is warm.
        let c = ctx();
        let mut memo = HashMap::new();
        memo.insert("teh".to_string(), true); // pretend we already accepted it
        assert!(
            c.is_correct_memo("teh", &mut memo),
            "memo hit must be returned without re-checking the dictionary"
        );
    }

    #[test]
    fn replace_first_word_preserves_surroundings() {
        let out = replace_first_word("the quik brown fox", "quik", "quick");
        assert_eq!(out, "the quick brown fox");
    }

    #[test]
    fn replace_span_replaces_exact_range() {
        // Replacing the second "quik" by its span leaves the first untouched.
        let text = "quik quik brown";
        let out = replace_span(text, 5, 9, "quick");
        assert_eq!(out, "quik quick brown");
    }

    #[test]
    fn stale_spans_past_end_do_not_panic() {
        // Simulate rapid backspacing: spans computed against a longer text are
        // applied to a now-shorter buffer. Must not panic and must still cover
        // the whole (shorter) text.
        let stale = vec![WordSpan { start: 4, end: 8 }];
        let job = build_layout_job("ab", &stale, TextFormat::default(), 100.0);
        assert_eq!(job.text, "ab");
        // The whole text is covered by exactly one (base) section.
        assert_eq!(job.sections.len(), 1);
        assert_eq!(job.sections[0].byte_range, ByteIndex(0)..ByteIndex(2));
    }

    #[test]
    fn stale_spans_filtered_when_slicing_words() {
        // Mirrors the suggestion-collection guard: stale spans (computed against
        // a longer text) must be filtered out instead of slicing out of bounds.
        let text = "hello"; // len 5
        let stale = [
            WordSpan { start: 0, end: 6 }, // past end
            WordSpan { start: 0, end: 5 }, // valid
        ];
        let len = text.len();
        let words: Vec<String> = stale
            .iter()
            .filter(|s| {
                s.start < s.end
                    && s.end <= len
                    && text.is_char_boundary(s.start)
                    && text.is_char_boundary(s.end)
            })
            .map(|s| text[s.start..s.end].to_owned())
            .collect();
        assert_eq!(words, vec!["hello".to_owned()]);
    }

    #[cfg(feature = "grammar")]
    #[test]
    fn grammar_flags_article_error() {
        // Classic article agreement mistake harper's curated linter catches.
        let text = "This is an test.";
        let spans = grammar_spans(text);
        assert!(
            !spans.is_empty(),
            "grammar checker should flag 'an test', got no lints"
        );
        // Every reported span must be a valid byte range into the source.
        for span in &spans {
            assert!(span.start < span.end, "span must be non-empty: {span:?}");
            assert!(span.end <= text.len(), "span must stay in bounds: {span:?}");
            assert!(
                text.is_char_boundary(span.start) && text.is_char_boundary(span.end),
                "span must land on char boundaries: {span:?}"
            );
        }
    }

    #[cfg(feature = "grammar")]
    #[test]
    fn grammar_clean_text_has_no_lints() {
        assert!(
            grammar_spans("The patient is stable.").is_empty(),
            "well-formed sentence should produce no grammar lints"
        );
    }

    #[cfg(feature = "grammar")]
    #[test]
    fn grammar_ignores_spelling_of_unknown_words() {
        // harper's own per-word spell checker doesn't know medical terms like
        // "Quantiferon" and would emit a Spelling lint ("Did you mean to spell
        // ... this way?"). Spelling is the spelling path's job, so the grammar
        // path must not flag it. (The word is spelled correctly here; only the
        // surrounding sentence is otherwise clean.)
        assert!(
            grammar_spans("Quantiferon negative.").is_empty(),
            "grammar path must not surface spelling lints for unknown words"
        );
    }

    #[cfg(feature = "grammar")]
    #[test]
    fn grammar_still_flags_real_errors() {
        // Dropping Spelling-kind lints must not suppress genuine grammar issues.
        assert!(
            !grammar_spans("This is an test.").is_empty(),
            "non-spelling grammar errors must still be reported"
        );
    }

    #[test]
    fn span_on_non_char_boundary_is_skipped() {
        // "é" is two bytes; a stale span ending mid-codepoint must be dropped.
        let text = "é";
        let bad = vec![WordSpan { start: 0, end: 1 }];
        let job = build_layout_job(text, &bad, TextFormat::default(), 100.0);
        assert_eq!(job.text, text);
        assert_eq!(job.sections.len(), 1);
        assert_eq!(job.sections[0].byte_range, ByteIndex(0)..ByteIndex(text.len()));
    }

    #[test]
    fn selection_background_highlights_only_selected_range() {
        // The visible right-click highlight: only the selected bytes get the
        // selection background; the rest stays transparent.
        let text = "hello world";
        let mut job = build_layout_job(text, &[], TextFormat::default(), 100.0);
        let sel = Color32::from_rgb(1, 2, 3);
        apply_selection_background(&mut job, 0, 5, sel);
        assert_eq!(job.text, text);
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].byte_range, ByteIndex(0)..ByteIndex(5));
        assert_eq!(job.sections[0].format.background, sel);
        assert_eq!(job.sections[1].byte_range, ByteIndex(5)..ByteIndex(11));
        assert_eq!(job.sections[1].format.background, Color32::TRANSPARENT);
    }

    #[test]
    fn selection_background_splits_section_into_three() {
        // A selection in the middle of a section splits it into before / inside /
        // after parts, with only the middle highlighted.
        let text = "hello world";
        let mut job = build_layout_job(text, &[], TextFormat::default(), 100.0);
        let sel = Color32::from_rgb(10, 20, 30);
        apply_selection_background(&mut job, 2, 5, sel);
        assert_eq!(job.sections.len(), 3);
        assert_eq!(job.sections[0].byte_range, ByteIndex(0)..ByteIndex(2));
        assert_eq!(job.sections[0].format.background, Color32::TRANSPARENT);
        assert_eq!(job.sections[1].byte_range, ByteIndex(2)..ByteIndex(5));
        assert_eq!(job.sections[1].format.background, sel);
        assert_eq!(job.sections[2].byte_range, ByteIndex(5)..ByteIndex(11));
        assert_eq!(job.sections[2].format.background, Color32::TRANSPARENT);
    }

    #[test]
    fn selection_background_skips_invalid_range() {
        // 'é' is two bytes, so byte 2 lands mid-codepoint; an out-of-bounds end
        // is also stale. Both must leave the job untouched (never slice a glyph).
        let text = "Aé";
        let mut job = build_layout_job(text, &[], TextFormat::default(), 100.0);
        let before = job.sections.len();
        apply_selection_background(&mut job, 0, 2, Color32::RED); // mid-codepoint
        assert_eq!(job.sections.len(), before);
        apply_selection_background(&mut job, 0, 99, Color32::RED); // past end
        assert_eq!(job.sections.len(), before);
    }

    #[test]
    fn replace_char_range_inserts_at_collapsed_caret() {
        // Empty range = collapsed caret: pure insertion, caret moves past it.
        let (out, caret) = replace_char_range("hello world", 5, 5, " there");
        assert_eq!(out, "hello there world");
        assert_eq!(caret, 11);
    }

    #[test]
    fn replace_char_range_replaces_selection() {
        let (out, caret) = replace_char_range("hello world", 0, 5, "hi");
        assert_eq!(out, "hi world");
        // Caret sits just past the inserted "hi" (2 chars from start).
        assert_eq!(caret, 2);
    }

    #[test]
    fn replace_char_range_is_codepoint_aware() {
        // "café" is 4 chars but 5 bytes; replacing the trailing "é" by char
        // index must not slice mid-codepoint.
        let (out, caret) = replace_char_range("café", 3, 4, "e");
        assert_eq!(out, "cafe");
        assert_eq!(caret, 4);
    }

    #[test]
    fn char_slice_returns_range_or_none() {
        assert_eq!(char_slice("hello", 1, 4).as_deref(), Some("ell"));
        assert_eq!(char_slice("café", 3, 4).as_deref(), Some("é"));
        // Out-of-range / inverted ranges yield None rather than panicking.
        assert_eq!(char_slice("hi", 0, 5), None);
        assert_eq!(char_slice("hi", 2, 1), None);
    }
}
