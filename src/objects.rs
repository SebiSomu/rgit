use clap::Subcommand;

/// Represents an uncompressed Git database object holding its decoded payload.
// Used by: src/helpers/objects.rs
// Used by command/helper: Object serialization and decoding helpers
#[derive(Debug)]
pub enum GitObject {
    Blob(Vec<u8>),
    Tree(Vec<TreeEntry>),
    Commit(CommitData),
}

/// Represents a single file or directory record stored within a parsed Git tree object.
// Used by: src/helpers/objects.rs
// Used by command/helper: Tree parsing and traversal helpers
#[derive(Debug)]
pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub hash: [u8; 20],
}

/// Represents the deserialized header fields and message body of a commit object.
// Used by: src/helpers/commit.rs
// Used by command/helper: Commit parsing and resolution helpers
#[derive(Debug)]
pub struct CommitData {
    pub tree_hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub message: String,
}

/// Encapsulates a parsed gitignore glob pattern rule and its directory context.
// Used by: src/helpers/gitignore.rs, src/helpers/clean.rs, src/commands/clean.rs
// Used by command/helper: `parse_gitignore_line`, `is_path_ignored`, `collect_clean_candidates`, and the `clean` command
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitIgnoreRule {
    pub pattern: String,
    pub base_dir: String,
    pub negated: bool,
    pub dir_only: bool,
    pub has_slash: bool,
}

/// Represents an atomic edit operation emitted during Myers line-by-line diff calculation.
// Used by: src/helpers/diff.rs, src/commands/diff.rs
// Used by command/helper: `myers_diff`, `format_diff_output`, and the `diff` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOp {
    Keep(String),
    Delete(String),
    Insert(String),
}

/// Holds conflicting three-way file contents (base, ours, and theirs) for conflict marker generation.
// Used by: src/helpers/merge.rs, src/commands/merge.rs, src/helpers/rebase.rs
// Used by command/helper: `three_way_tree_merge`, `generate_conflict_markers`, and the `merge`/`rebase` commands
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    pub path: String,
    pub base: Vec<u8>,
    pub ours: Vec<u8>,
    pub theirs: Vec<u8>,
}

/// Enum of subcommands supported by the stash command.
// Used by: src/cli.rs, src/commands/stash.rs
// Used by command/helper: CLI argument parser and the `stash` command dispatcher
#[derive(Subcommand, Debug)]
pub enum StashAction {
    Push {
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
    },
    Pop {
        stash: Option<String>,
    },
    Apply {
        stash: Option<String>,
    },
    List,
    Drop {
        stash: Option<String>,
    },
    Show {
        stash: Option<String>,
    },
    Clear,
}

/// Represents an individual recorded entry in the stash reference log (`.git/logs/refs/stash`).
// Used by: src/helpers/state_files.rs, src/commands/stash.rs
// Used by command/helper: `read_stash_list`, `write_stash_list`, and the `stash` command
pub struct StashEntry {
    pub hash: String,
    pub message: String,
}

/// Represents the step calculation result of a binary search commit inspection in bisect.
// Used by: src/helpers/bisect.rs, src/commands/bisect.rs
// Used by command/helper: `bisect_step` helper and the `bisect` command
pub enum BisectOutcome {
    WaitingForBad,
    WaitingForGood,
    Continue(String, usize),
    Found(String),
}

/// Enum of subcommands supported by the bisect command.
// Used by: src/cli.rs, src/commands/bisect.rs
// Used by command/helper: CLI argument parser and the `bisect` command dispatcher
#[derive(Subcommand, Debug)]
pub enum BisectAction {
    Start {
        bad: Option<String>,
        good: Vec<String>,
    },
    Bad {
        rev: Option<String>,
    },
    Good {
        rev: Option<String>,
    },
    Skip {
        revs: Vec<String>,
    },
    Reset {
        commit: Option<String>,
    },
    Log,
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

/// Defines the working tree and staging index update mode for `reset`.
// Used by: src/commands/reset.rs
// Used by command/helper: The `reset` command
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

/// An in-memory flattened map of relative file paths to their 20-byte object hash and mode.
// Used by: src/helpers/objects.rs, src/helpers/commit.rs, src/helpers/checkout.rs, src/helpers/state_files.rs, src/helpers/banana_split.rs, src/commands/banana_split.rs
// Used by command/helper: `flatten_tree`, `resolve_tree_from_source`, `build_tree_from_map`, and the `banana_split` command
pub type Tree = std::collections::BTreeMap<String, ([u8; 20], u32)>;

/// Filter mode determining whether untracked files, ignored files, or both are targeted for cleaning.
// Used by: src/helpers/clean.rs, src/commands/clean.rs
// Used by command/helper: `collect_clean_candidates` helper and the `clean` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanMode {
    Normal,
    IncludeIgnored,
    IgnoredOnly,
}

impl CleanMode {
    pub fn wants(&self, ignored: bool) -> bool {
        match self {
            CleanMode::Normal => !ignored,
            CleanMode::IncludeIgnored => true,
            CleanMode::IgnoredOnly => ignored,
        }
    }
}

/// An instruction action assigned to an individual commit entry in an interactive rebase todo file.
// Used by: src/helpers/rebase.rs, src/commands/rebase.rs
// Used by command/helper: `parse_edited_todo`, `render_todo_for_editing`, `commit_item`, and the `rebase` command
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

impl Action {
    pub fn parse(word: &str) -> Option<Action> {
        match word {
            "pick" | "p" => Some(Action::Pick),
            "reword" | "r" => Some(Action::Reword),
            "edit" | "e" => Some(Action::Edit),
            "squash" | "s" => Some(Action::Squash),
            "fixup" | "f" => Some(Action::Fixup),
            "drop" | "d" => Some(Action::Drop),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Reword => "reword",
            Action::Edit => "edit",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Drop => "drop",
        }
    }
}

/// A parsed line entry from `.git/REBASE_TODO` tracking a commit, its instruction action, and subject.
// Used by: src/helpers/rebase.rs, src/commands/rebase.rs
// Used by command/helper: `read_todo`, `write_todo`, `render_todo_for_editing`, `process_rebase_todo`, and the `rebase` command
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoItem {
    pub action: Action,
    pub hash: String,
    pub subject: String,
}

impl TodoItem {
    pub fn format_line(&self) -> String {
        let short_hash = &self.hash[..self.hash.len().min(7)];
        format!("{} {} {}", self.action.name(), short_hash, self.subject)
    }
}

/// Execution outcome of applying a single todo item during the rebase replay cycle.
// Used by: src/helpers/rebase.rs
// Used by command/helper: `commit_item`, `rebase_continue`, and `process_rebase_todo` helpers
#[derive(Debug, PartialEq, Eq)]
pub enum ItemResult {
    Done,
    PausedForEdit,
}