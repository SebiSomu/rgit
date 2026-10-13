use clap::Subcommand;
use std::path::{Path, PathBuf};

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

#[derive(Subcommand, Debug)]
pub enum ReflogAction {
    Show {
        reference: Option<String>,
        #[arg(short = 'n', long = "max-count")]
        max_count: Option<usize>,
    },
    Exists {
        reference: String,
    },
    Delete {
        entries: Vec<String>,
    },
    Expire {
        #[arg(long = "all", conflicts_with = "reference")]
        all: bool,
        #[arg(long = "expire", default_value = "90")]
        expire: String,
        reference: Option<String>,
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

/// Represents the result or update status of a reference during a push operation.
// Used by: src/helpers/push.rs, src/commands/push.rs
// Used by command/helper: `push` command and push reporting helpers
#[derive(Debug, PartialEq, Eq)]
pub enum PushStatus {
    UpToDate,
    FastForward { old_hash: String, new_hash: String },
    ForcedUpdate { old_hash: String, new_hash: String },
    NewBranch { new_hash: String },
    Deleted,
}

/// The update status of a single remote-tracking ref after a fetch operation.
// Used by: src/helpers/fetch.rs, src/commands/fetch.rs
// Used by command/helper: `fetch_from_local`, `fetch_from_github`, and the `fetch` command
#[derive(Debug, PartialEq, Eq)]
pub enum FetchRefResult {
    NewBranch { branch: String, sha: String },
    Updated { branch: String, old_sha: String, new_sha: String },
    UpToDate { branch: String },
}

/// Target repository metadata and authentication credentials for pushing to GitHub via the Git Data API.
// Used by: src/helpers/github.rs, src/commands/push.rs
// Used by command/helper: `push_to_github`, `resolve_github_target`, and the `push` command
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubTarget {
    pub owner: String,
    pub repo: String,
    pub token: String,
}

/// Parsed author or committer metadata extracted from a raw commit object for API serialization.
// Used by: src/helpers/github.rs
// Used by command/helper: `parse_commit_identity`, `create_github_commit`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommitIdentity {
    pub name: String,
    pub email: String,
    pub date: String,
}

/// A single file or subtree entry in a GitHub tree creation request.
// Used by: src/helpers/github.rs
// Used by command/helper: `upload_tree_to_github`, `create_github_tree`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GitHubTreeEntry {
    pub path: String,
    pub mode: String,
    pub r#type: String,
    pub sha: String,
}

/// The place a `clone` reads from, once the repository argument has been classified. It is
/// either a directory on the local filesystem (cloned by copying objects across, the same
/// way `fetch` reads a local remote) or a GitHub repository reached through the Git Data API.
// Used by: src/helpers/clone.rs, src/commands/clone.rs
// Used by command/helper: `classify_clone_source`, `remote_default_branch`, and the `clone` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneSource {
    Local { path: PathBuf, git_dir: PathBuf },
    GitHub { owner: String, repo: String },
}

impl CloneSource {
    /// The string stored as `remote.<name>.url`: the absolute path for a local source, the
    /// https URL for a GitHub one.
    pub fn url(&self) -> String {
        match self {
            CloneSource::Local { path, .. } => path.display().to_string(),
            CloneSource::GitHub { owner, repo } => format!("https://github.com/{}/{}", owner, repo),
        }
    }
 
    /// The directory a clone goes into when none is given: the repository's name, with any
    /// `.git` suffix dropped (`proj.git` -> `proj`, `proj/.git` -> `proj`).
    pub fn default_dir_name(&self) -> String {
        match self {
            CloneSource::Local { path, .. } => {
                let name_of = |p: &Path| {
                    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                };
                let mut name = name_of(path);
                if name == ".git" {
                    name = path.parent().map(name_of).unwrap_or_default();
                }
                let name = name.strip_suffix(".git").unwrap_or(&name).to_string();
                if name.is_empty() { "repository".to_string() } else { name }
            }
            CloneSource::GitHub { repo, .. } => repo.clone(),
        }
    }
}

/// Subcommands supported by the `remote` command for managing tracking repositories.
// Used by: src/cli.rs, src/commands/remote.rs
// Used by command/helper: CLI argument parser and the `remote` command dispatcher
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum RemoteAction {
    Add {
        name: String,
        url: String,
    },
    Remove {
        name: String,
    },
    Rm {
        name: String,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
    GetUrl {
        #[arg(long = "push")]
        push: bool,
        name: String,
    },
    SetUrl {
        name: String,
        new_url: String,
    },
    Show {
        name: String,
    },
}

/// Detailed information about a configured remote repository.
// Used by: src/helpers/remote.rs, src/commands/remote.rs
// Used by command/helper: `get_remote_info` helper and the `remote show` command
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
    pub tracking_branches: Vec<String>,
}

/// One line of a file's blame result: the line itself and the commit that last changed it,
/// with that commit's author and author time. Lines that exist only in the working copy
/// carry an all-zero commit hash and the author "Not Committed Yet".
// Used by: src/helpers/blame.rs, src/commands/blame.rs
// Used by command/helper: `blame_file`, `format_blame_output`, and the `blame` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub commit: String,
    pub author: String,
    pub timestamp: u64,
    pub tz: String,
    pub line_no: usize,
    pub content: String,
    pub boundary: bool,
}
 
impl BlameLine {
    pub fn hash_label(&self) -> String {
        if self.boundary {
            format!("^{}", self.commit.chars().take(7).collect::<String>())
        } else {
            self.commit.chars().take(8).collect()
        }
    }
}

/// Represents the supported serialization archive format (tar, tar.gz, or zip).
// Used by: src/commands/archive.rs, src/helpers/archive.rs
// Used by command/helper: `create_archive`, `ArchiveFormat::parse`, and the `archive` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Tar,
    TarGz,
    Zip,
}

impl ArchiveFormat {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tar" => Ok(ArchiveFormat::Tar),
            "tar.gz" | "tgz" => Ok(ArchiveFormat::TarGz),
            "zip" => Ok(ArchiveFormat::Zip),
            other => anyhow::bail!("Unknown archive format '{}'", other),
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(ArchiveFormat::TarGz)
        } else if name.ends_with(".tar") {
            Some(ArchiveFormat::Tar)
        } else if name.ends_with(".zip") {
            Some(ArchiveFormat::Zip)
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Zip => "zip",
        }
    }
}

/// One line of a reflog: a ref moving from one commit to another, who did it, when, and why.
/// Stored one per line in `.git/logs/<ref>` in git's own format:
/// `<old> <new> <name> <<email>> <timestamp> <tz>\t<message>`.
// Used by: src/helpers/reflog.rs, src/commands/reflog.rs
// Used by command/helper: `append_reflog_entry`, `read_reflog`, `write_reflog`, `expire_reflog`, `format_reflog_output`, and the `reflog` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// Hash the ref pointed at before (all zeros if it was just created).
    pub old_hash: String,
    /// Hash the ref points at after (all zeros if it was deleted).
    pub new_hash: String,
    /// `Name <email>` of whoever made the move.
    pub identity: String,
    /// When the move happened, as seconds since the Unix epoch.
    pub timestamp: u64,
    /// Timezone offset of `timestamp`, as `+HHMM` / `-HHMM`.
    pub tz: String,
    /// What caused the move, e.g. `commit -m fix typo` or `switch feature`.
    pub message: String,
}

impl ReflogEntry {
    /// Renders the entry as one line of a reflog file (without the trailing newline).
    pub fn to_line(&self) -> String {
        format!(
            "{} {} {} {} {}\t{}",
            self.old_hash, self.new_hash, self.identity, self.timestamp, self.tz, self.message
        )
    }

    /// Parses one line of a reflog file; `None` if it is malformed.
    pub fn parse(line: &str) -> Option<ReflogEntry> {
        let (head, message) = line.split_once('\t').unwrap_or((line, ""));
        let old_hash = head.get(0..40)?;
        let new_hash = head.get(41..81)?;
        let is_hash = |h: &str| h.chars().all(|c| c.is_ascii_hexdigit());
        if !is_hash(old_hash) || !is_hash(new_hash) || head.as_bytes().get(40) != Some(&b' ') {
            return None;
        }

        let rest = head.get(82..)?;
        let close = rest.rfind('>')?;
        let identity = rest[..=close].trim().to_string();
        let mut tail = rest[close + 1..].split_whitespace();
        let timestamp: u64 = tail.next()?.parse().ok()?;
        let tz = tail.next().unwrap_or("+0000").to_string();

        Some(ReflogEntry {
            old_hash: old_hash.to_string(),
            new_hash: new_hash.to_string(),
            identity,
            timestamp,
            tz,
            message: message.to_string(),
        })
    }
}