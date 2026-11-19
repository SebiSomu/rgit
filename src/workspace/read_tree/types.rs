/// Parsed arguments of the `read-tree` command.
// Used by: src/workspace/read_tree/engine.rs
// Used by command/helper: `parse_read_tree_args`, `plan_read_tree`, and the `read_tree` command
#[derive(Debug, Clone, Default)]
pub struct ReadTreeOptions {
    /// `-m`: merge the trees into the index instead of replacing it.
    pub merge: bool,
    /// `--reset`: like `-m`, but local changes never stop the operation.
    pub reset: bool,
    /// `--prefix=<dir>/`: add the tree under this directory, keeping the rest of the index.
    pub prefix: Option<String>,
    /// `-u`: also update the working tree.
    pub update: bool,
    /// `-i`: do not look at the working tree at all.
    pub index_only: bool,
    /// `-n` / `--dry-run`: check whether the operation would succeed, change nothing.
    pub dry_run: bool,
    /// `--trivial`: fail if a three-way merge needs more than the trivial cases.
    pub trivial: bool,
    /// `--aggressive`: also resolve deletions and identical additions in a three-way merge.
    pub aggressive: bool,
    /// `--empty`: empty the index.
    pub empty: bool,
    /// The tree-ish arguments.
    pub trees: Vec<String>,
}

/// What `read-tree` decided to do: the new index, and for `-u` the files to write and remove.
// Used by: src/workspace/read_tree/engine.rs
// Used by command/helper: `plan_read_tree` and the `read_tree` command
#[derive(Debug, Clone, Default)]
pub struct ReadTreePlan {
    /// The index entries after the operation.
    pub entries: Vec<crate::core::index::IndexEntry>,
    /// Paths whose file must be (re)written from the new entry.
    pub write: Vec<String>,
    /// Paths whose file must be removed.
    pub remove: Vec<String>,
    /// Problems that stop the operation, as git words them.
    pub errors: Vec<String>,
    /// Paths a three-way merge could not settle trivially.
    pub unmerged: Vec<String>,
}
