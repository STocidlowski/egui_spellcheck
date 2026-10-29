#![doc=include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod dictation;
mod spell_check;
pub use dictation::{
    DictationControl, DictationIntent, DictationTextEdit, DictationTextEditOutput, DictationUiState,
};

pub use spell_check::{SpellCheckTextEdit, prewarm, suggestions};
