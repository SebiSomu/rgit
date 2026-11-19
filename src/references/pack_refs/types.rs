/// Parsed arguments for the `pack-refs` command: which loose refs get moved into
/// `.git/packed-refs` and whether their loose files are removed afterwards.
// Used by: src/references/pack_refs/types.rs, src/references/pack_refs/engine.rs
// Used by command/helper: `parse_pack_refs_args`, `pack_refs_matching`, and the `pack_refs` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRefsOptions {
    /// `--all`: pack every ref (except per-worktree and symbolic ones), not just tags.
    pub all: bool,
    /// `--[no-]prune`: delete the loose ref files once they are packed (default on).
    pub prune: bool,
    /// `--include <pattern>`: glob patterns of refs to pack (replace the default `refs/tags/*`).
    pub includes: Vec<String>,
    /// `--exclude <pattern>`: glob patterns of refs never to pack (win over includes).
    pub excludes: Vec<String>,
}

impl Default for PackRefsOptions {
    fn default() -> Self {
        Self { all: false, prune: true, includes: Vec::new(), excludes: Vec::new() }
    }
}

/// One `.git/packed-refs` entry: the object a ref points at and, for an annotated tag, the
/// non-tag object it peels to (written as the `^<hash>` line after the entry).
// Used by: src/references/pack_refs/engine.rs
// Used by command/helper: `read_packed_entries`, `write_packed_refs`, and `pack_refs_matching`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedRefEntry {
    pub hash: String,
    pub peeled: Option<String>,
}
