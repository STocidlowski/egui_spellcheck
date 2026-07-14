#![doc=include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod spell_check;

pub use spell_check::{SpellCheckTextEdit, prewarm};
