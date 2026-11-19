/// Options for the `ls-files` command, which lists paths from the staging index
/// (`.git/index`) and/or untracked paths from the working tree.
// Used by: src/cli.rs, src/workspace/ls_files.rs
// Used by command/helper: the `ls_files` command (option parsing and output selection)
#[derive(Debug, Clone, Default)]
pub struct LsFilesOptions {
    /// `-c` / `--cached`: list every tracked path in the index (the default when no other mode is given).
    pub cached: bool,
    /// `-d` / `--deleted`: list tracked paths whose working-tree file is missing.
    pub deleted: bool,
    /// `-m` / `--modified`: list tracked paths whose working-tree content differs from the index (includes deleted).
    pub modified: bool,
    /// `-o` / `--others`: list untracked paths in the working tree.
    pub others: bool,
    /// `-i` / `--ignored`: only show paths matching `.gitignore` rules (requires `-o` or `-c`, plus `--exclude-standard`).
    pub ignored: bool,
    /// `--exclude-standard`: apply `.gitignore` rules when listing.
    pub exclude_standard: bool,
    /// `-s` / `--stage`: print `<mode> <hash> <stage>\t<path>` instead of just the path.
    pub stage: bool,
    /// `-u` / `--unmerged`: show only unmerged (stage 1-3) entries; implies `--stage`.
    pub unmerged: bool,
    /// `-t`: prefix each line with a status tag (`H`, `S`, `M`, `C`, `R`, `?`).
    pub show_tags: bool,
    /// `-z`: terminate lines with NUL instead of newline.
    pub null_terminated: bool,
    /// Pathspecs restricting the output (exact paths, directory prefixes, or globs).
    pub pathspecs: Vec<String>,
}

/// Working-tree state of a tracked index entry, as reported by `ls-files -m` / `-d`.
// Used by: src/workspace/ls_files.rs, src/workspace/ls_files/engine.rs
// Used by command/helper: `worktree_state` helper and the `ls_files` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeState {
    Unchanged,
    Modified,
    Deleted,
}

/// An untracked working-tree path found by the `ls-files --others` walk, together with
/// whether `.gitignore` rules (including an ignored ancestor directory) exclude it.
// Used by: src/workspace/ls_files/engine.rs
// Used by command/helper: `collect_untracked_paths` helper and the `ls_files` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedPath {
    /// Forward-slash relative path; nested repositories end with `/`.
    pub path: String,
    pub ignored: bool,
}
