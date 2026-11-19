/// Options for the `format-patch` command, which turns commits into mailbox-style patch files.
// Used by: src/cli.rs, src/mail/format_patch.rs
// Used by command/helper: the `format_patch` command (commit selection, numbering, and output)
#[derive(Debug, Clone, Default)]
pub struct FormatPatchOptions {
    /// `<since>`, `<rev-range>` (`A..B`), or a tip when combined with `-<n>` / `--root`.
    pub revs: Vec<String>,
    /// `-<n>` / `--max-count <n>`: only the last `n` commits.
    pub max_count: Option<usize>,
    /// `--root`: treat a single revision as a tip and include everything reachable from it.
    pub root: bool,
    /// `--stdout`: print all patches to standard output instead of writing files.
    pub stdout: bool,
    /// `-o` / `--output-directory <dir>`: where to write the patch files.
    pub output_directory: Option<String>,
    /// `-n` / `--numbered`: always use `[PATCH n/m]`, even for a single patch.
    pub numbered: bool,
    /// `-N` / `--no-numbered`: always use `[PATCH]`.
    pub no_numbered: bool,
    /// `--start-number <n>`: number of the first patch (default 1).
    pub start_number: usize,
    /// `--subject-prefix <prefix>`: replaces `PATCH` in the subject prefix.
    pub subject_prefix: String,
    /// `-k` / `--keep-subject`: no `[PATCH]` prefix at all.
    pub keep_subject: bool,
    /// `--signature <text>`: signature after the `-- ` line (default `rgit <version>`).
    pub signature: Option<String>,
    /// `--no-signature`: omit the signature block entirely.
    pub no_signature: bool,
}
