/// Defines the working tree and staging index update mode for `reset`.
// Used by: src/workspace/reset.rs
// Used by command/helper: The `reset` command
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}
