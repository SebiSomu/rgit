/// Which kind of line `log --graph` prints next for the current commit (git's `graph_state`):
/// plain branch lines, a `...` marker for skipped output, the rows that make room before an
/// octopus merge, the commit row itself, the row that fans a merge out to its parents, and the
/// rows that move branch lines left until they sit in their columns.
// Used by: src/history/graph/types.rs, src/history/graph.rs
// Used by command/helper: `graph_update`, `graph_next_line`, and the `log` command through them
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphState {
    #[default]
    Padding,
    Skip,
    PreCommit,
    Commit,
    PostMerge,
    Collapsing,
}

/// The state of a `log --graph` drawing (a port of git's `struct git_graph`): the commit being
/// shown and its interesting parents, the branch lines (each the commit it leads to) before and
/// after this commit, and the mapping from screen columns (two characters per branch line) to the
/// line each one has to move to. Indices stay signed, with -1 for "none", as in git.
// Used by: src/history/graph.rs
// Used by command/helper: every `graph_*` helper and the `log` command
#[derive(Debug, Clone, Default)]
pub struct GitGraph {
    /// The commit being shown, and the mark printed for it (`*`, or `<`/`>` with `--left-right`).
    pub commit: Option<String>,
    pub mark: String,
    /// The commit's parents that are shown too (only the first with `--first-parent`).
    pub parents: Vec<String>,
    /// How many screen columns the lines for this commit need.
    pub width: i32,
    pub expansion_row: i32,
    pub state: GraphState,
    pub prev_state: GraphState,
    pub commit_index: i32,
    pub prev_commit_index: i32,
    /// For a merge, where its first parent is: 0 to the left (skewed left), 1 otherwise.
    pub merge_layout: i32,
    pub edges_added: i32,
    pub prev_edges_added: i32,
    /// Branch lines before this commit, and after it (both as the commit each line leads to).
    pub columns: Vec<String>,
    pub new_columns: Vec<String>,
    /// Target branch line for every screen column (-1: none), and the one from the row before.
    pub mapping: Vec<i32>,
    pub old_mapping: Vec<i32>,
    pub mapping_size: usize,
    /// Color index (into git's 12 column colors) of each branch line in `columns` and
    /// `new_columns`, and the one the next new line gets (git's `default_column_color`).
    pub column_colors: Vec<usize>,
    pub new_column_colors: Vec<usize>,
    pub default_color: usize,
    /// Whether the lines are drawn in color.
    pub color: bool,
}
