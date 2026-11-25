//! Types of `rgit tip`.

/// One tip: a short title, two or three sentences, and a command to try.
// Used by: src/plumbing/tip/types.rs, src/plumbing/tip/engine.rs, src/plumbing/tip.rs
// Used by command/helper: `TIPS` and the `tip` command
#[derive(Debug, Clone, Copy)]
pub struct Tip {
    pub title: &'static str,
    pub text: &'static str,
    pub example: &'static str,
}
