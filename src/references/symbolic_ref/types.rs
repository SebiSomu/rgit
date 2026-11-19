/// Parsed arguments for the `symbolic-ref` command. Parsed by hand like git's option parser
/// (bundled switches such as `-qd`, `-m<reason>` or `-m <reason>`, options after the names,
/// `--` ending option parsing), so usage errors can exit 129 as git does.
// Used by: src/references/symbolic_ref/types.rs, src/references/symbolic_ref.rs, src/references/symbolic_ref/engine.rs
// Used by command/helper: `parse_symbolic_ref_args` and the `symbolic_ref` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolicRefOptions {
    /// `-q` / `--quiet`: exit 1 silently instead of failing when the ref is not symbolic.
    pub quiet: bool,
    /// `-d` / `--delete`: delete the symbolic ref itself (never what it points at).
    pub delete: bool,
    /// `--short`: print the target shortened (`refs/heads/main` → `main`).
    pub short: bool,
    /// `--[no-]recurse`: follow chains of symbolic refs to the last one (default on).
    pub recurse: bool,
    /// `-m <reason>`: reflog message for an update; without it no reflog entry is written.
    pub message: Option<String>,
    /// `<name>` and, when setting, `<ref>`.
    pub args: Vec<String>,
}

impl Default for SymbolicRefOptions {
    fn default() -> Self {
        Self { quiet: false, delete: false, short: false, recurse: true, message: None, args: Vec::new() }
    }
}

/// The result of resolving a ref name the way git's `resolve_ref_unsafe` does: the ref name
/// the walk ended on, its hash when that ref exists, and whether any symbolic ref was followed.
// Used by: src/references/symbolic_ref/engine.rs
// Used by command/helper: `resolve_ref_unsafe` and the `symbolic_ref` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    pub name: String,
    pub hash: Option<String>,
    pub is_symref: bool,
}
