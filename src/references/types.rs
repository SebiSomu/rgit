/// What a single ref file (or `packed-refs` entry) holds, read without following symbolic
/// refs: nothing, an object hash, a `ref: <target>` pointer, or contents that are neither.
// Used by: src/references/symbolic_ref/engine.rs, src/references/update_ref/engine.rs
// Used by command/helper: `read_raw_ref_value` and `resolve_ref_unsafe` (for the `symbolic-ref` command)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawRefValue {
    Missing,
    Hash(String),
    Symbolic(String),
    Broken,
}
