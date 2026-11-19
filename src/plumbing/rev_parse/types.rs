/// Parsed options for the `rev-parse` command, covering both revision-resolution
/// mode and repository-property query mode.
// Used by: src/cli.rs, src/plumbing/rev_parse.rs
// Used by command/helper: `resolve_rev`, `run_rev_parse`, and the `rev_parse` command
#[derive(Debug, Clone, Default)]
pub struct RevParseOptions {
    /// `--git-dir`: print the path to the `.git` directory.
    pub git_dir: bool,
    /// `--show-toplevel`: print the absolute path of the working-tree root.
    pub show_toplevel: bool,
    /// `--is-inside-work-tree`: print `true` or `false`.
    pub is_inside_work_tree: bool,
    /// `--is-bare-repository`: always `false` for rgit repos.
    pub is_bare_repository: bool,
    /// `--abbrev-ref`: convert a hash/rev to a short symbolic name (`HEAD`, branch, tag).
    pub abbrev_ref: bool,
    /// `--symbolic-full-name`: like `--abbrev-ref` but emit the full ref path.
    pub symbolic_full_name: bool,
    /// `--verify`: exit non-zero if the revision cannot be resolved.
    pub verify: bool,
    /// `-q` / `--quiet`: suppress error messages when used with `--verify`.
    pub quiet: bool,
    /// `--short[=N]`: abbreviate the output hash to N characters (default 7).
    pub short: Option<usize>,
    /// `--sq`: shell-quote all outputs into one line.
    pub sq: bool,
    /// Revision arguments collected after all flags.
    pub revs: Vec<String>,
}
