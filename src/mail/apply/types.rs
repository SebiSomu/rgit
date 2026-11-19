/// Options for the `apply` command, which applies a unified/git diff to the working tree
/// and/or the index, or only inspects it (`--check`, `--stat`, `--numstat`, `--summary`).
// Used by: src/cli.rs, src/mail/am.rs, src/mail/apply.rs, src/mail/apply/engine.rs
// Used by command/helper: the `apply` command (mode selection, targets, and output)
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    /// `--check`: verify the patch applies cleanly without changing anything.
    pub check: bool,
    /// `--stat`: print a diffstat instead of applying.
    pub stat: bool,
    /// `--numstat`: print `added\tdeleted\tpath` per file instead of applying.
    pub numstat: bool,
    /// `--summary`: print creations, deletions, renames, and mode changes instead of applying.
    pub summary: bool,
    /// `--apply`: apply even when an inspection flag (`--stat`, `--numstat`, `--summary`, `--check`) is given.
    pub apply: bool,
    /// `--cached`: apply to the index only, leaving the working tree untouched.
    pub cached: bool,
    /// `--index`: apply to both the working tree and the index (they must match beforehand).
    pub index: bool,
    /// `-R` / `--reverse`: apply the patch in reverse.
    pub reverse: bool,
    /// `-v` / `--verbose`: report progress and failing context on stderr.
    pub verbose: bool,
    /// `-p<n>`: number of leading path components to strip (default 1).
    pub strip: usize,
    /// Patch files to read; standard input when empty or `-`.
    pub patches: Vec<String>,
}

/// The role of a single line inside a patch hunk.
// Used by: src/mail/apply/types.rs, src/mail/apply/engine.rs
// Used by command/helper: `parse_patch`, `apply_hunks`, `FilePatch::reversed`, and the `apply` command (stats)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkLineKind {
    Context,
    Removed,
    Added,
}

/// One line of a patch hunk: its role, its text (without line terminator or trailing `\r`),
/// whether the patch line carried a `\r` (CRLF content), and whether it was followed by
/// `\ No newline at end of file`.
// Used by: src/mail/apply/types.rs, src/mail/apply/engine.rs
// Used by command/helper: `parse_patch` and `apply_hunks`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkLine {
    pub kind: HunkLineKind,
    pub text: String,
    pub had_cr: bool,
    pub no_newline: bool,
}

/// A single `@@ -old_start,old_count +new_start,new_count @@` hunk of a file patch.
// Used by: src/mail/apply/types.rs, src/mail/apply/engine.rs
// Used by command/helper: `parse_patch`, `apply_hunks`, and `FilePatch::reversed`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

/// Everything a patch says about one file: old/new paths (`None` for `/dev/null`), modes,
/// creation/deletion/rename/copy flags, rename similarity, binary marker, and its hunks.
// Used by: src/mail/apply/types.rs, src/mail/apply.rs, src/mail/apply/engine.rs
// Used by command/helper: `parse_patch`, `apply_hunks`, `format_apply_stat`, and the `apply` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilePatch {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
    pub is_new: bool,
    pub is_delete: bool,
    pub is_rename: bool,
    pub is_copy: bool,
    pub similarity: Option<u32>,
    pub is_binary: bool,
    pub hunks: Vec<PatchHunk>,
    /// Full `index <old>..<new>` blob hashes, required to verify binary patches.
    pub index_hashes: Option<(String, String)>,
    /// `GIT binary patch` data: the forward hunk, then the optional reverse hunk.
    pub binary_hunks: Vec<BinaryHunk>,
}

impl FilePatch {
    /// The name git uses when reporting on this patch: the old path, or the new one for creations.
    pub fn display_name(&self) -> &str {
        self.old_path.as_deref().or(self.new_path.as_deref()).unwrap_or("")
    }

    /// The name shown by `apply -v`: `old => new` for renames, otherwise `display_name`.
    pub fn verbose_name(&self) -> String {
        match (&self.old_path, &self.new_path) {
            (Some(old), Some(new)) if self.is_rename && old != new => format!("{} => {}", old, new),
            _ => self.display_name().to_string(),
        }
    }

    /// The name used by `--stat` / `--numstat`: the new path, or the old one for deletions.
    pub fn stat_name(&self) -> &str {
        self.new_path.as_deref().or(self.old_path.as_deref()).unwrap_or("")
    }

    /// Number of added and removed lines across all hunks.
    pub fn line_counts(&self) -> (usize, usize) {
        let mut added = 0;
        let mut removed = 0;
        for line in self.hunks.iter().flat_map(|h| &h.lines) {
            match line.kind {
                HunkLineKind::Added => added += 1,
                HunkLineKind::Removed => removed += 1,
                HunkLineKind::Context => {}
            }
        }
        (added, removed)
    }

    /// The patch that undoes this one (`apply -R`): sides, modes, and line roles swapped.
    pub fn reversed(&self) -> FilePatch {
        FilePatch {
            old_path: self.new_path.clone(),
            new_path: self.old_path.clone(),
            old_mode: self.new_mode,
            new_mode: self.old_mode,
            is_new: self.is_delete,
            is_delete: self.is_new,
            is_rename: self.is_rename,
            is_copy: self.is_copy,
            similarity: self.similarity,
            is_binary: self.is_binary,
            index_hashes: self.index_hashes.as_ref().map(|(old, new)| (new.clone(), old.clone())),
            binary_hunks: self.binary_hunks.iter().rev().take(if self.binary_hunks.len() == 2 { 2 } else { 0 }).cloned().collect(),
            hunks: self
                .hunks
                .iter()
                .map(|h| PatchHunk {
                    old_start: h.new_start,
                    old_count: h.new_count,
                    new_start: h.old_start,
                    new_count: h.old_count,
                    lines: h
                        .lines
                        .iter()
                        .map(|l| HunkLine {
                            kind: match l.kind {
                                HunkLineKind::Added => HunkLineKind::Removed,
                                HunkLineKind::Removed => HunkLineKind::Added,
                                HunkLineKind::Context => HunkLineKind::Context,
                            },
                            ..l.clone()
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// One decoded hunk of a `GIT binary patch`: either the full new content (`literal`) or a
/// git delta against the old content (`delta`), already inflated from zlib + base85.
// Used by: src/mail/apply/types.rs, src/mail/apply/engine.rs
// Used by command/helper: `parse_patch`, `apply_git_delta`, `check_file_patch`, and `FilePatch::reversed`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BinaryHunk {
    pub is_delta: bool,
    pub size: usize,
    pub data: Vec<u8>,
}

/// One line of a file being patched: its text (without terminator) and its original line
/// ending (`"\n"`, `"\r\n"`, or `""` for a final line without newline), so CRLF files and
/// missing final newlines survive `apply` unchanged.
// Used by: src/mail/apply/engine.rs
// Used by command/helper: `split_image`, `join_image`, and `apply_hunks`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchImageLine {
    pub text: String,
    pub eol: String,
}

/// Why a hunk could not be applied: the hunk's 1-based position in its file patch, the
/// `old_start` line git reports, and the preimage text that was searched for (for `-v`).
// Used by: src/mail/apply/engine.rs
// Used by command/helper: `apply_hunks` and the `apply` command (error reporting)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkFailure {
    pub hunk_number: usize,
    pub old_start: usize,
    pub searched_for: String,
}
