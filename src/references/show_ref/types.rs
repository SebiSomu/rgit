/// Parsed arguments for the `show-ref` command. Parsed by hand like git's option parser
/// (`--hash[=<n>]`, `--abbrev[=<n>]`, `--exclude-existing[=<pattern>]`, options after
/// patterns), so usage errors can exit 129 as git does.
// Used by: src/references/show_ref.rs, src/references/show_ref/engine.rs
// Used by command/helper: `parse_show_ref_args`, `show_ref_line`, and the `show_ref` command
#[derive(Debug, Clone, Default)]
pub struct ShowRefOptions {
    /// `--head`: also show HEAD, even when patterns or `--heads`/`--tags` would filter it out.
    pub head: bool,
    /// `-d` / `--dereference`: after an annotated tag, also show what it peels to as `<ref>^{}`.
    pub dereference: bool,
    /// `-s` / `--hash[=<n>]`: print only the (optionally abbreviated) hash.
    pub hash_only: bool,
    /// Hash length from `--hash=<n>` / `--abbrev[=<n>]` (minimum unique length; `None` = full).
    pub abbrev: Option<usize>,
    /// `--heads` / `--tags`: only refs under `refs/heads/` and/or `refs/tags/`.
    pub heads: bool,
    pub tags: bool,
    /// `--verify`: arguments are exact ref names (`refs/...` or `HEAD`), all must exist.
    pub verify: bool,
    /// `--exists`: check that one ref exists, without resolving it (exit 2 if it doesn't).
    pub exists: bool,
    /// `-q` / `--quiet`: print nothing; only the exit status reports the result.
    pub quiet: bool,
    /// `--exclude-existing[=<pattern>]`: filter refs read from stdin down to ones that don't exist.
    pub exclude_existing: Option<String>,
    /// Patterns (or, with `--verify` / `--exists`, ref names).
    pub patterns: Vec<String>,
}
