/// One submodule as described by `.gitmodules`.
// Used by: src/workspace/submodule/engine.rs
// Used by command/helper: `read_gitmodules`, `selected_submodules`, and the `submodule` command
#[derive(Debug, Clone, Default)]
pub struct SubmoduleInfo {
    /// The name in `[submodule "<name>"]`.
    pub name: String,
    /// Where it lives in the working tree.
    pub path: String,
    /// The URL to clone it from, as written in `.gitmodules`.
    pub url: String,
    /// `branch = <name>`, if set.
    pub branch: Option<String>,
    /// The commit recorded for it in the index (the gitlink), if it is tracked.
    pub staged: Option<String>,
}
