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

/// A loose object file found in `.git/objects/xx/`: what it is called, where it lives, how
/// large the compressed file is and when it was last modified (which is how `gc` decides an
/// unreferenced object is old enough to prune).
// Used by: src/helpers/gc.rs, src/commands/gc.rs
// Used by command/helper: `list_loose_objects`, `plan_gc`, `remove_loose_object`, and the `gc` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseObject {
    /// Full 40-digit hash of the object.
    pub hash: String,
    /// Path of the object file inside `.git/objects`.
    pub path: PathBuf,
    /// Size of the compressed object file, in bytes.
    pub size: u64,
    /// Last-modified time of the file, as seconds since the Unix epoch.
    pub modified: u64,
}

/// What a `gc` run found: how many loose objects exist, how many are reachable, which
/// unreachable ones were (or, in a dry run, would be) pruned, and how many reflog entries
/// expired.
// Used by: src/helpers/gc.rs, src/commands/gc.rs
// Used by command/helper: `plan_gc` and the `gc` command
#[derive(Debug, Clone, Default)]
pub struct GcReport {
    /// Loose objects in the repository.
    pub total_objects: usize,
    /// Of those, the ones reachable from a ref, HEAD, the index, the stash, an operation in progress or a live reflog entry.
    pub reachable_objects: usize,
    /// Of those, the ones nothing leads to.
    pub unreachable_objects: usize,
    /// Unreachable objects that were spared: too new, referenced by a too-new object, or pruning is off.
    pub kept_unreachable: usize,
    /// Unreachable objects that were (or would be) deleted.
    pub pruned: Vec<LooseObject>,
    /// Reflog entries that were (or would be) expired.
    pub reflog_entries_expired: usize,
}

impl GcReport {
    /// Total size in bytes of the objects that were (or would be) pruned.
    pub fn pruned_bytes(&self) -> u64 {
        self.pruned.iter().map(|o| o.size).sum()
    }
}

/// How serious a problem found by `fsck` is: an error means the repository is damaged and
/// makes `fsck` exit non-zero; a warning is worth knowing about but doesn't.
// Used by: src/helpers/fsck.rs, src/commands/fsck.rs
// Used by command/helper: `FsckIssue`, `FsckReport`, and the `fsck` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsckSeverity {
    Error,
    Warning,
}

/// One problem found by `fsck`, with a human-readable description.
// Used by: src/helpers/fsck.rs, src/commands/fsck.rs
// Used by command/helper: `verify_loose_object`, `check_refs`, `check_index`, `mark_reachable_in_graph`, and the `fsck` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckIssue {
    pub severity: FsckSeverity,
    pub message: String,
}
 
impl FsckIssue {
    /// A problem that means the repository is damaged.
    pub fn error(message: String) -> FsckIssue {
        FsckIssue { severity: FsckSeverity::Error, message }
    }
 
    /// A problem worth reporting that doesn't mean damage.
    pub fn warning(message: String) -> FsckIssue {
        FsckIssue { severity: FsckSeverity::Warning, message }
    }
}

/// One object in the graph `fsck` builds: its type (`blob`, `tree`, `commit`, `tag`, or
/// `unknown` if it could not be read) and the objects it links to, each with the type the link
/// claims they have (`any` for a tag's target).
// Used by: src/helpers/fsck.rs
// Used by command/helper: `verify_loose_object`, `mark_reachable_in_graph`, and `run_fsck_checks`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckNode {
    pub kind: String,
    pub links: Vec<(String, &'static str)>,
}

/// Everything an `fsck` run found: how many objects it looked at, the problems, and the objects
/// nothing reaches (all of them, and the subset that nothing else unreachable points at, which
/// are called dangling). Types and hashes are stored as `(type, hash)` pairs.
// Used by: src/helpers/fsck.rs, src/commands/fsck.rs
// Used by command/helper: `run_fsck_checks` and the `fsck` command
#[derive(Debug, Clone, Default)]
pub struct FsckReport {
    pub objects_checked: usize,
    pub issues: Vec<FsckIssue>,
    pub dangling: Vec<(String, String)>,
    pub unreachable: Vec<(String, String)>,
}
 
impl FsckReport {
    /// Number of problems that mean damage.
    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == FsckSeverity::Error).count()
    }
 
    /// Number of problems that don't.
    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == FsckSeverity::Warning).count()
    }
}

/// Options controlling pattern search, target files, and output formatting for `grep`.
// Used by: src/commands/grep.rs, src/helpers/grep.rs
// Used by command/helper: `grep` command and `search_content` helper
#[derive(Debug, Clone)]
pub struct GrepOptions {
    pub ignore_case: bool,
    pub invert_match: bool,
    pub line_number: bool,
    pub count: bool,
    pub files_with_matches: bool,
    pub files_without_match: bool,
    pub fixed_strings: bool,
    pub cached: bool,
}

/// The result of searching a single file or blob in `grep`.
// Used by: src/commands/grep.rs, src/helpers/grep.rs
// Used by command/helper: `grep` command and `search_content` helper
#[derive(Debug, Clone)]
pub struct GrepFileResult {
    pub display_path: String,
    pub is_binary: bool,
    pub matches: Vec<(usize, String)>,
}

/// Statistics collected by the `count-objects` command about loose objects, packs, and garbage.
// Used by: src/helpers/count_objects.rs, src/commands/count_objects.rs
// Used by command/helper: `collect_object_counts`, `format_human_readable_size`, and the `count_objects` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectCountReport {
    pub count: usize,
    pub size_bytes: u64,
    pub in_pack: usize,
    pub packs: usize,
    pub size_pack_bytes: u64,
    pub prune_packable: usize,
    pub garbage: usize,
    pub size_garbage_bytes: u64,
}

/// Parsed options for the `rev-parse` command, covering both revision-resolution
/// mode and repository-property query mode.
// Used by: src/helpers/rev_parse.rs, src/commands/rev_parse.rs
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

/// Options for the `shortlog` command, which summarises `git log` output grouped by author.
// Used by: src/helpers/shortlog.rs, src/commands/shortlog.rs
// Used by command/helper: `collect_shortlog` and the `shortlog` command
#[derive(Debug, Clone, Default)]
pub struct ShortlogOptions {
    /// `-s` / `--summary`: show only commit counts, not subject lines.
    pub summary: bool,
    /// `-n` / `--numbered`: sort output by number of commits (descending) instead of alphabetically.
    pub numbered: bool,
    /// `-e` / `--email`: include the email address in the author key / display.
    pub email: bool,
    /// `--all`: walk every ref in `refs/heads/` and `refs/tags/` instead of just HEAD.
    pub all: bool,
    /// Starting revision (default: HEAD when empty).
    pub rev: Option<String>,
}
/// Options for the `ls-files` command, which lists paths from the staging index
/// (`.git/index`) and/or untracked paths from the working tree.
// Used by: src/cli.rs, src/commands/ls_files.rs
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
    /// `-t`: prefix each line with a status tag (`H`, `C`, `R`, `?`).
    pub show_tags: bool,
    /// `-z`: terminate lines with NUL instead of newline.
    pub null_terminated: bool,
    /// Pathspecs restricting the output (exact paths, directory prefixes, or globs).
    pub pathspecs: Vec<String>,
}

/// Working-tree state of a tracked index entry, as reported by `ls-files -m` / `-d`.
// Used by: src/helpers/ls_files.rs, src/commands/ls_files.rs
// Used by command/helper: `worktree_state` helper and the `ls_files` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeState {
    Unchanged,
    Modified,
    Deleted,
}

/// An untracked working-tree path found by the `ls-files --others` walk, together with
/// whether `.gitignore` rules (including an ignored ancestor directory) exclude it.
// Used by: src/helpers/ls_files.rs, src/commands/ls_files.rs
// Used by command/helper: `collect_untracked_paths` helper and the `ls_files` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedPath {
    /// Forward-slash relative path; nested repositories end with `/`.
    pub path: String,
    pub ignored: bool,
}

/// Options for the `check-ignore` command, which reports which paths are excluded by
/// `.gitignore` rules and, in verbose mode, which rule excluded them.
// Used by: src/cli.rs, src/commands/check_ignore.rs
// Used by command/helper: the `check_ignore` command (validation, input source, and output format)
#[derive(Debug, Clone, Default)]
pub struct CheckIgnoreOptions {
    /// `-q` / `--quiet`: print nothing; only the exit status reports the result (single path only).
    pub quiet: bool,
    /// `-v` / `--verbose`: print `<source>:<line>:<pattern>\t<path>` for the matching rule.
    pub verbose: bool,
    /// `-n` / `--non-matching`: with `-v`, also print paths that matched no rule as `::\t<path>`.
    pub non_matching: bool,
    /// `--no-index`: also check tracked paths, which are skipped by default.
    pub no_index: bool,
    /// `--stdin`: read paths from standard input, one per line.
    pub stdin: bool,
    /// `-z`: with `--stdin`, input and output records are NUL-terminated.
    pub null_terminated: bool,
    /// Paths given on the command line.
    pub paths: Vec<String>,
}

/// A parsed `.gitignore` rule together with where it was defined: the `.gitignore` file,
/// the 1-based line number, and the pattern text as written (including `!` and trailing `/`).
// Used by: src/helpers/check_ignore.rs, src/commands/check_ignore.rs
// Used by command/helper: `find_ignore_match` helper and the `check_ignore` command (`-v` output)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcedIgnoreRule {
    pub rule: GitIgnoreRule,
    pub source: String,
    pub line_number: usize,
    pub pattern_text: String,
}

/// Options for the `apply` command, which applies a unified/git diff to the working tree
/// and/or the index, or only inspects it (`--check`, `--stat`, `--numstat`, `--summary`).
// Used by: src/cli.rs, src/commands/apply.rs
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
// Used by: src/helpers/apply.rs, src/commands/apply.rs
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
// Used by: src/helpers/apply.rs
// Used by command/helper: `parse_patch` and `apply_hunks`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkLine {
    pub kind: HunkLineKind,
    pub text: String,
    pub had_cr: bool,
    pub no_newline: bool,
}

/// A single `@@ -old_start,old_count +new_start,new_count @@` hunk of a file patch.
// Used by: src/helpers/apply.rs
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
// Used by: src/helpers/apply.rs, src/commands/apply.rs
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

/// One decoded hunk of a `GIT binary patch`: either the full new content (`literal`) or a
/// git delta against the old content (`delta`), already inflated from zlib + base85.
// Used by: src/helpers/apply.rs
// Used by command/helper: `parse_patch`, `apply_git_delta`, `check_file_patch`, and `FilePatch::reversed`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BinaryHunk {
    pub is_delta: bool,
    pub size: usize,
    pub data: Vec<u8>,
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

/// One line of a file being patched: its text (without terminator) and its original line
/// ending (`"\n"`, `"\r\n"`, or `""` for a final line without newline), so CRLF files and
/// missing final newlines survive `apply` unchanged.
// Used by: src/helpers/apply.rs
// Used by command/helper: `split_image`, `join_image`, and `apply_hunks`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchImageLine {
    pub text: String,
    pub eol: String,
}

/// Why a hunk could not be applied: the hunk's 1-based position in its file patch, the
/// `old_start` line git reports, and the preimage text that was searched for (for `-v`).
// Used by: src/helpers/apply.rs, src/commands/apply.rs
// Used by command/helper: `apply_hunks` and the `apply` command (error reporting)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkFailure {
    pub hunk_number: usize,
    pub old_start: usize,
    pub searched_for: String,
}

/// Planned results of checking a set of file patches, keyed by path: `Some((content, mode))`
/// for a file to write, `None` for a file to delete. Nothing touches disk until every patch
/// in the set has been checked, which is what makes `apply` and `am` all-or-nothing.
// Used by: src/helpers/apply.rs, src/commands/apply.rs, src/commands/am.rs
// Used by command/helper: `check_file_patch`, `load_patch_target`, `patch_target_exists`, `write_planned_changes`, and the `apply`/`am` commands
pub type PlannedChanges = std::collections::BTreeMap<String, Option<(Vec<u8>, u32)>>;

/// Options for the `format-patch` command, which turns commits into mailbox-style patch files.
// Used by: src/cli.rs, src/commands/format_patch.rs
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

/// An author or committer identity from a commit object: name, email, Unix timestamp, and
/// the `+HHMM` timezone it was recorded in.
// Used by: src/helpers/format_patch.rs, src/helpers/am.rs, src/commands/format_patch.rs, src/commands/am.rs
// Used by command/helper: `parse_commit_object`, `format_rfc2822_date`, `parse_mail`, `build_commit_with_author`, and the `format_patch`/`am` commands
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitSignature {
    pub name: String,
    pub email: String,
    pub timestamp: i64,
    pub tz: String,
}

/// A commit object parsed into its parts: tree, parents, author, committer, and raw message.
// Used by: src/helpers/format_patch.rs, src/commands/format_patch.rs
// Used by command/helper: `parse_commit_object`, `select_patch_commits`, and the `format_patch` command
#[derive(Debug, Clone, Default)]
pub struct ParsedCommit {
    pub hash: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub author: CommitSignature,
    pub committer: CommitSignature,
    pub message: String,
}

/// One file's contribution to a `--stat` block: display name, added/deleted line counts,
/// and `(old_size, new_size)` in bytes for binary files.
// Used by: src/helpers/format_patch.rs
// Used by command/helper: `diff_tree_file` and `format_diffstat`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffStatEntry {
    pub name: String,
    pub added: usize,
    pub deleted: usize,
    pub binary_sizes: Option<(usize, usize)>,
}

/// The git-format diff of one path between two trees: the full `diff --git` section, its
/// diffstat entry, and its `--summary` line (creation, deletion, or mode change), if any.
// Used by: src/helpers/format_patch.rs, src/commands/format_patch.rs
// Used by command/helper: `diff_tree_file`, `diff_commit_against_parent`, and the `format_patch` command
#[derive(Debug, Clone, Default)]
pub struct FileDiff {
    pub text: String,
    pub stat: DiffStatEntry,
    pub summary: Option<String>,
}

/// Options for the `am` command, which applies a mailbox of patches as commits.
// Used by: src/cli.rs, src/commands/am.rs
// Used by command/helper: the `am` command (input, resume actions, and subject handling)
#[derive(Debug, Clone, Default)]
pub struct AmOptions {
    /// Mailbox files (or `format-patch` files) to read; standard input when empty.
    pub mboxes: Vec<String>,
    /// `--continue` / `-r` / `--resolved`: commit the resolved index for the stopped patch and go on.
    pub cont: bool,
    /// `--skip`: drop the stopped patch and go on with the rest.
    pub skip: bool,
    /// `--abort`: restore the branch, index, and working tree to where `am` started.
    pub abort: bool,
    /// `--quit`: forget the session but keep HEAD, index, and working tree as they are.
    pub quit: bool,
    /// `--show-current-patch[=raw|diff]`: print the stopped patch's mail or just its diff.
    pub show_current_patch: Option<String>,
    /// `-k` / `--keep-subject`: keep `[PATCH]` and other bracketed prefixes in the subject.
    pub keep_subject: bool,
}

/// A patch email decoded by `am`: author identity and date (from `From:`/`Date:`, possibly
/// overridden by in-body headers), the cleaned subject, the final commit message, and the
/// diff text that follows the `---` separator.
// Used by: src/helpers/am.rs, src/commands/am.rs
// Used by command/helper: `parse_mail`, `commit_mail`, and the `am` command
#[derive(Debug, Clone, Default)]
pub struct MailPatch {
    pub author: CommitSignature,
    pub subject: String,
    pub message: String,
    pub patch: String,
}

/// Parsed arguments for the `rev-list` command: which commits to start from and exclude,
/// how to walk and order them, which to keep, and how to print them. Parsed by hand (not by
/// clap) because git's syntax is order-sensitive (`--not`) and allows `-<n>` / `-n<n>`.
// Used by: src/helpers/rev_list.rs, src/commands/rev_list.rs
// Used by command/helper: `parse_rev_list_args`, `run_rev_list_walk`, `format_rev_list_entry`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct RevListOptions {
    /// Revision arguments as given (`A`, `^A`, `A..B`, `A...B`); `--not` is already applied as `^`.
    pub revs: Vec<String>,
    /// `--all`, `--branches`, `--tags`, `--remotes`: start from every ref of that kind.
    pub all: bool,
    pub branches: bool,
    pub tags: bool,
    pub remotes: bool,
    /// `-n` / `-<n>` / `--max-count`: show at most this many commits.
    pub max_count: Option<usize>,
    /// `--skip`: drop this many commits before showing any.
    pub skip: usize,
    /// `--reverse`: print the selected commits oldest first.
    pub reverse: bool,
    /// `--count`: print only how many commits would be shown.
    pub count: bool,
    /// `--parents`: print each commit's (possibly rewritten) parents after it.
    pub parents: bool,
    /// `--timestamp`: print the committer timestamp before each commit.
    pub timestamp: bool,
    /// `--left-right`: mark commits `<` (left side of `A...B`) or `>`.
    pub left_right: bool,
    /// `--first-parent`: follow only the first parent of merges.
    pub first_parent: bool,
    /// `--topo-order`: children before parents, keeping lines of history together.
    pub topo_order: bool,
    /// `--date-order`: children before parents, otherwise by committer date.
    pub date_order: bool,
    /// `--abbrev-commit`: print abbreviated (unique, at least 7 character) hashes.
    pub abbrev_commit: bool,
    /// `--min-parents` / `--max-parents` (`--merges` = min 2, `--no-merges` = max 1).
    pub min_parents: Option<usize>,
    pub max_parents: Option<usize>,
    /// `--since` / `--after` and `--until` / `--before`, as Unix timestamps.
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// `--author`, `--committer`, `--grep` regexes, with `--all-match`, `--invert-grep`,
    /// `-i` / `--regexp-ignore-case`, and `-F` / `--fixed-strings`.
    pub authors: Vec<String>,
    pub committers: Vec<String>,
    pub greps: Vec<String>,
    pub all_match: bool,
    pub invert_grep: bool,
    pub ignore_case: bool,
    pub fixed_strings: bool,
    /// `--oneline`, `--pretty=oneline`, `--format=<fmt>`, `--pretty=format:<fmt>` / `tformat:<fmt>`.
    pub format: Option<RevListFormat>,
    /// Paths after `--` (or non-revision arguments naming existing files) that limit history.
    pub paths: Vec<String>,
}

/// How `rev-list` prints each commit beyond its hash line.
// Used by: src/helpers/rev_list.rs, src/commands/rev_list.rs
// Used by command/helper: `parse_rev_list_args`, `format_rev_list_entry`, and the `rev_list` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevListFormat {
    /// `<hash> <subject>` on one line.
    Oneline,
    /// A `commit <hash>` line, then the expanded template. `terminator` records `tformat:` vs
    /// `format:`; like git, `rev-list` ends every entry with a newline either way.
    Template { template: String, terminator: bool },
}

/// A commit visited by the `rev-list` walk: its parsed object, the parents the walk followed
/// (simplified when paths limit history), whether history simplification hides it, and
/// whether it belongs to the left side of a symmetric range.
// Used by: src/helpers/rev_list.rs, src/commands/rev_list.rs
// Used by command/helper: `run_rev_list_walk`, `sort_rev_list_topologically`, `rewrite_rev_list_parents`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct WalkedCommit {
    pub commit: ParsedCommit,
    pub parents: Vec<String>,
    pub hidden: bool,
    pub left: bool,
}

/// Where a `rev-list` walk starts: the revisions in the order git adds them (each with whether
/// it is negative, i.e. excluded, and whether it is the left side of a symmetric `A...B`), and
/// the "bottoms" (the excluded commits that were named explicitly, which git still treats as
/// relevant parents when simplifying path-limited history).
// Used by: src/helpers/rev_list.rs, src/commands/rev_list.rs
// Used by command/helper: `resolve_rev_list_tips`, `run_rev_list_walk`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct RevListRoots {
    pub pending: Vec<(String, bool, bool)>,
    pub bottoms: std::collections::HashSet<String>,
}

/// The mutable state of a `rev-list` walk, mirroring the object flags git keeps per commit:
/// parsed commits (`commits` holds exactly the commits git would have parsed, since an
/// unparsed commit has no known tree or parents) and their current (possibly simplified) parents, `SEEN` (queued once),
/// `ADDED` (parents processed), `UNINTERESTING` (excluded, marked gradually as the walk goes,
/// which is what makes path simplification depend on walk order exactly like git),
/// `TREESAME` (hidden by simplification), and `SYMMETRIC_LEFT`.
// Used by: src/helpers/rev_list.rs, src/commands/rev_list.rs
// Used by command/helper: `run_rev_list_walk`, `rewrite_rev_list_parents`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct RevWalkState {
    pub commits: std::collections::HashMap<String, ParsedCommit>,
    pub parents: std::collections::HashMap<String, Vec<String>>,
    pub seen: std::collections::HashSet<String>,
    pub added: std::collections::HashSet<String>,
    pub uninteresting: std::collections::HashSet<String>,
    pub bottoms: std::collections::HashSet<String>,
    pub treesame: std::collections::HashSet<String>,
    pub left: std::collections::HashSet<String>,
}

impl RevWalkState {
    /// git's `relevant_commit`: not uninteresting, or uninteresting but named explicitly (a bottom).
    pub fn is_relevant(&self, hash: &str) -> bool {
        !self.uninteresting.contains(hash) || self.bottoms.contains(hash)
    }
}

/// Subcommands of `worktree`, which manages extra working trees attached to one repository.
/// Each linked worktree has its own HEAD, index and in-progress state under
/// `.git/worktrees/<id>/`, while objects, refs and config stay shared.
// Used by: src/cli.rs, src/commands/worktree.rs
// Used by command/helper: CLI argument parser and the `worktree` command dispatcher
#[derive(Subcommand, Debug)]
pub enum WorktreeAction {
    /// Create a worktree at <path> and check out <commit-ish> in it
    Add {
        path: PathBuf,
        commit_ish: Option<String>,
        /// Create a new branch <new-branch> starting at <commit-ish> and check it out
        #[arg(short = 'b', value_name = "new-branch", conflicts_with_all = ["force_new_branch", "detach"])]
        new_branch: Option<String>,
        /// Like -b, but reset <new-branch> to <commit-ish> if it already exists
        #[arg(short = 'B', value_name = "new-branch", conflicts_with = "detach")]
        force_new_branch: Option<String>,
        /// Check out <commit-ish> as a detached HEAD
        #[arg(short = 'd', long = "detach")]
        detach: bool,
        /// Allow a branch that is already checked out elsewhere, or a missing registered path
        #[arg(short = 'f', long = "force")]
        force: bool,
        /// Lock the new worktree right after creating it
        #[arg(long = "lock")]
        lock: bool,
        /// Reason recorded with --lock
        #[arg(long = "reason", requires = "lock")]
        reason: Option<String>,
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
    },
    /// List the main worktree and every linked worktree
    List {
        #[arg(long = "porcelain")]
        porcelain: bool,
        #[arg(short = 'v', long = "verbose", conflicts_with = "porcelain")]
        verbose: bool,
    },
    /// Delete a linked worktree (refuses a dirty one unless forced)
    Remove {
        worktree: String,
        /// Remove even with local changes; give twice to also remove a locked worktree
        #[arg(short = 'f', long = "force", action = clap::ArgAction::Count)]
        force: u8,
    },
    /// Move a linked worktree to a new location
    Move {
        worktree: String,
        new_path: PathBuf,
        /// Give twice to move a locked worktree
        #[arg(short = 'f', long = "force", action = clap::ArgAction::Count)]
        force: u8,
    },
    /// Protect a linked worktree from being pruned, moved or removed
    Lock {
        worktree: String,
        #[arg(long = "reason")]
        reason: Option<String>,
    },
    /// Undo `worktree lock`
    Unlock {
        worktree: String,
    },
    /// Delete the records of worktrees whose directories no longer exist
    Prune {
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
    },
}

/// One worktree of the repository as `worktree list` reports it: where it is, what its HEAD
/// points at, and whether it is locked or can be pruned. The main worktree has no `id`.
// Used by: src/helpers/worktree.rs, src/commands/worktree.rs, src/commands/branch.rs
// Used by command/helper: `list_worktrees`, `find_worktree`, `worktree_using_branch`, and the `worktree` command
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Absolute top directory of the worktree.
    pub path: PathBuf,
    /// Directory holding its HEAD and index: the shared `.git` for the main worktree,
    /// `.git/worktrees/<id>` for a linked one.
    pub admin_dir: PathBuf,
    /// Name of the linked worktree's directory under `.git/worktrees/`; `None` for the main one.
    pub id: Option<String>,
    /// Commit HEAD resolves to; `None` on an unborn branch.
    pub head: Option<String>,
    /// Full ref HEAD is attached to (`refs/heads/main`); `None` when detached.
    pub branch: Option<String>,
    /// `Some(reason)` when locked; the reason may be empty.
    pub locked: Option<String>,
    /// `Some(reason)` when the worktree's directory is gone and its record can be pruned.
    pub prunable: Option<String>,
    /// Whether the command is running inside this worktree.
    pub is_current: bool,
}

impl WorktreeInfo {
    /// Says whether this is the repository's main worktree.
    pub fn is_main(&self) -> bool {
        self.id.is_none()
    }

    /// The path the way git prints it: forward slashes on every platform.
    pub fn display_path(&self) -> String {
        crate::repo::to_git_path_string(&self.path)
    }
}

/// How `worktree add` comes by the branch it checks out in the new worktree.
// Used by: src/commands/worktree.rs
// Used by command/helper: the `worktree add` command
pub enum BranchCreation {
    /// Already exists (or there is no branch: detached HEAD).
    Existing,
    /// Created by this command (`-b`, or the branch named after the path).
    New,
    /// `-B` moved an existing branch; holds its previous tip.
    Reset(String),
}

/// Options for the `check-ref-format` command, which validates a proposed ref name against
/// git's ref naming rules (and, with `--branch`, against the rules for a branch name).
// Used by: src/cli.rs, src/commands/check_ref_format.rs, src/helpers/check_ref_format.rs
// Used by command/helper: the `check_ref_format` command and `check_refname_format`
#[derive(Debug, Clone, Default)]
pub struct CheckRefFormatOptions {
    /// `--normalize` / `--print`: strip leading slashes, collapse repeated ones, and print the result if valid.
    pub normalize: bool,
    /// `--allow-onelevel`: accept names with a single component (e.g. `main`); off by default.
    pub allow_onelevel: bool,
    /// `--refspec-pattern`: allow a single `*` anywhere in the name, as in a refspec.
    pub refspec_pattern: bool,
    /// `--branch <name>`: check `<name>` as a branch name and print it if valid.
    pub branch: Option<String>,
    /// The ref name to check (not used with `--branch`).
    pub refname: Option<String>,
}
