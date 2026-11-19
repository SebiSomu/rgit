/// What the `replace` command was asked to do.
// Used by: src/references/replace/types.rs, src/references/replace.rs, src/references/replace/engine.rs
// Used by command/helper: `parse_replace_args` and the `replace` command
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReplaceMode {
    /// `-l [<pattern>]`, or no arguments: list replace refs.
    #[default]
    List,
    /// `-d <object>...`: delete replace refs.
    Delete,
    /// `<object> <replacement>`: replace one object with another.
    Replace,
    /// `-e <object>`: edit a copy of the object and replace it with that.
    Edit,
    /// `-g <commit> [<parent>...]`: replace a commit with a copy that has other parents.
    Graft,
}

/// Parsed arguments for the `replace` command, which manages `refs/replace/` object substitutions.
// Used by: src/references/replace/engine.rs
// Used by command/helper: `parse_replace_args` and the `replace` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplaceOptions {
    pub mode: ReplaceMode,
    /// `-f`: overwrite an existing replace ref and allow objects of different types.
    pub force: bool,
    /// `--raw`: with `--edit`, edit a tree's raw content instead of its `ls-tree` listing.
    pub raw: bool,
    /// `--format=short|medium|long` for listing.
    pub format: Option<String>,
    /// The mode's arguments: objects, a pattern, or a commit and its new parents.
    pub args: Vec<String>,
}
