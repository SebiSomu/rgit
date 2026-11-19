use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
pub(crate) use crate::objects::{BisectAction, ReflogAction, RemoteAction, StashAction, WorktreeAction};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None, disable_help_subcommand = true)]
pub struct Cli {
    /// Narrate what the command actually did, and what it means (`rgit --explain merge x`).
    #[arg(long = "explain", conflicts_with_all = ["teach", "plain"])]
    pub explain: bool,
    /// Like --explain, plus the refs and index changes underneath.
    #[arg(long = "teach", conflicts_with = "plain")]
    pub teach: bool,
    /// Exactly git's output, no rgit narration.
    #[arg(long = "plain")]
    pub plain: bool,
    #[command(subcommand)]
    pub command: Box<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(visible_alias = "init-db")]
    Init,
    HashObject {
        #[arg(short = 'w')]
        write: bool,
        file: PathBuf
    },
    CatFile {
        #[arg(short = 'p')]
        pretty_print: bool,
        object_hash: String
    },
    WriteTree,
    #[command(visible_alias = "stage")]
    Add {
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        paths: Vec<PathBuf>
    },
    LsTree {
        #[arg(long)]
        name_only: bool,
        tree_hash: String
    },
    CommitTree {
        tree_hash: String,
        #[arg(short = 'p')]
        parent_hash: Option<String>,
        #[arg(short = 'm')]
        message: String
    },
    Commit {
        #[arg(short = 'm', long = "message", value_name = "MSG")]
        messages: Vec<String>,
        #[arg(short = 'F', long = "file", value_name = "FILE", conflicts_with = "messages")]
        file: Option<String>,
        #[arg(short = 'C', long = "reuse-message", value_name = "COMMIT", conflicts_with_all = ["messages", "file", "reedit_message"])]
        reuse_message: Option<String>,
        #[arg(short = 'c', long = "reedit-message", value_name = "COMMIT", conflicts_with_all = ["messages", "file"])]
        reedit_message: Option<String>,
        #[arg(short = 'a', long = "all")]
        all: bool,
        #[arg(long = "amend")]
        amend: bool,
        #[arg(short = 'e', long = "edit", conflicts_with = "no_edit")]
        edit: bool,
        #[arg(long = "no-edit")]
        no_edit: bool,
        #[arg(long = "author", value_name = "AUTHOR")]
        author: Option<String>,
        #[arg(long = "date", value_name = "DATE")]
        date: Option<String>,
        #[arg(long = "reset-author")]
        reset_author: bool,
        #[arg(long = "allow-empty")]
        allow_empty: bool,
        #[arg(long = "allow-empty-message")]
        allow_empty_message: bool,
        #[arg(short = 's', long = "signoff")]
        signoff: bool,
        /// `strip` (drop `#` lines), `whitespace` or `default`.
        #[arg(long = "cleanup", value_name = "MODE", value_parser = ["strip", "whitespace", "default"])]
        cleanup: Option<String>,
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
    },
    Log {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    Status {
        #[arg(short = 's', long = "short")]
        short: bool,
        #[arg(short = 'b', long = "branch")]
        branch: bool,
        /// `--porcelain[=v1]`: the short format, stable for scripts.
        #[arg(long = "porcelain", num_args = 0..=1, default_missing_value = "v1", require_equals = true)]
        porcelain: Option<String>,
        #[arg(short = 'z')]
        nul: bool,
        #[arg(short = 'u', long = "untracked-files", num_args = 0..=1, default_missing_value = "all")]
        untracked: Option<String>,
    },
    #[command(disable_help_flag = true)]
    Branch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    Switch {
        branch: String,
        #[arg(short = 'c', long = "create", conflicts_with = "force")]
        create: bool,
        #[arg(long = "detach", conflicts_with = "create")]
        detach: bool,
        #[arg(short = 'f', long = "force", conflicts_with = "create")]
        force: bool
    },
    Checkout {
        target: Option<String>,
        #[arg(short = 'b')]
        create_branch: Option<String>,
        #[arg(long = "detach")]
        detach: bool,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// For unmerged paths, take stage 2 (ours) / stage 3 (theirs).
        #[arg(long = "ours", overrides_with = "theirs")]
        ours: bool,
        #[arg(long = "theirs", overrides_with = "ours")]
        theirs: bool,
        /// Recreate the conflicted merge of unmerged paths.
        #[arg(short = 'm', long = "merge")]
        merge: bool,
        /// Like `--merge`, with `merge`, `diff3` or `zdiff3` markers.
        #[arg(long = "conflict", value_name = "STYLE")]
        conflict: Option<String>,
        /// Paths after `--`: restore them instead of switching branches.
        #[arg(last = true)]
        paths: Vec<String>
    },
    Restore {
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        files: Vec<PathBuf>,
        #[arg(short = 'S', long = "staged")]
        staged: bool,
        #[arg(short = 'W', long = "worktree")]
        worktree: bool,
        #[arg(long = "source")]
        source: Option<String>,
        /// For unmerged paths, take stage 2 (ours) / stage 3 (theirs).
        #[arg(long = "ours", overrides_with = "theirs")]
        ours: bool,
        #[arg(long = "theirs", overrides_with = "ours")]
        theirs: bool,
        /// Recreate the conflicted merge of unmerged paths.
        #[arg(short = 'm', long = "merge")]
        merge: bool,
        /// Like `--merge`, with `merge`, `diff3` or `zdiff3` markers.
        #[arg(long = "conflict", value_name = "STYLE")]
        conflict: Option<String>,
    },
    Diff {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    Merge {
        #[arg(long = "ff", overrides_with_all = ["no_ff", "ff_only"])]
        ff: bool,
        #[arg(long = "no-ff", overrides_with_all = ["ff", "ff_only"])]
        no_ff: bool,
        #[arg(long = "ff-only", overrides_with_all = ["ff", "no_ff"])]
        ff_only: bool,
        #[arg(long = "squash", overrides_with = "no_squash")]
        squash: bool,
        #[arg(long = "no-squash", overrides_with = "squash")]
        no_squash: bool,
        #[arg(long = "commit", overrides_with = "no_commit")]
        commit: bool,
        #[arg(long = "no-commit", overrides_with = "commit")]
        no_commit: bool,
        #[arg(short = 'm', long = "message", value_name = "MESSAGE")]
        messages: Vec<String>,
        #[arg(short = 'F', long = "file", value_name = "FILE")]
        file: Option<String>,
        #[arg(short = 's', long = "strategy", value_name = "STRATEGY")]
        strategy: Option<String>,
        #[arg(short = 'X', long = "strategy-option", value_name = "OPTION")]
        strategy_options: Vec<String>,
        #[arg(long = "abort")]
        abort: bool,
        #[arg(long = "continue")]
        cont: bool,
        #[arg(long = "quit")]
        quit: bool,
        #[arg(short = 'n', long = "no-stat", overrides_with = "stat")]
        no_stat: bool,
        #[arg(long = "stat", overrides_with = "no_stat")]
        stat: bool,
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Accepted for compatibility: rgit never opens an editor for a merge commit.
        #[arg(long = "no-edit")]
        no_edit: bool,
        #[arg(long = "allow-unrelated-histories")]
        allow_unrelated: bool,
        commits: Vec<String>,
    },
    MergeBase {
        #[arg(short = 'a', long = "all", conflicts_with = "is_ancestor")]
        all: bool,
        #[arg(long = "is-ancestor", conflicts_with_all = ["all", "octopus"])]
        is_ancestor: bool,
        #[arg(long = "octopus", conflicts_with = "is_ancestor")]
        octopus: bool,
        #[arg(required = true)]
        commits: Vec<String>,
    },
    Rm {
        files: Vec<PathBuf>,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(long = "cached")]
        cached: bool,
        #[arg(short = 'r', long = "recursive")]
        recursive: bool
    },
    Reset {
        commit: Option<String>,
        #[arg(long = "soft", conflicts_with_all = ["mixed", "hard"])]
        soft: bool,
        #[arg(long = "mixed", conflicts_with_all = ["soft", "hard"])]
        mixed: bool,
        #[arg(long = "hard", conflicts_with_all = ["soft", "mixed"])]
        hard: bool,
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        #[arg(last = true)]
        paths: Vec<PathBuf>
    },
    Clean {
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(short = 'd')]
        dirs: bool,
        #[arg(short = 'x', conflicts_with = "only_ignored")]
        ignored: bool,
        #[arg(short = 'X', conflicts_with = "ignored")]
        only_ignored: bool
    },
    CherryPick {
        commits: Vec<String>,
        #[arg(short = 'n', long = "no-commit", overrides_with = "commit")]
        no_commit: bool,
        #[arg(long = "commit", overrides_with = "no_commit")]
        commit: bool,
        #[arg(short = 'x')]
        record_origin: bool,
        #[arg(short = 's', long = "signoff")]
        signoff: bool,
        #[arg(short = 'm', long = "mainline", value_name = "PARENT-NUMBER")]
        mainline: Option<usize>,
        #[arg(long = "ff")]
        ff: bool,
        #[arg(long = "allow-empty")]
        allow_empty: bool,
        #[arg(long = "allow-empty-message")]
        allow_empty_message: bool,
        #[arg(long = "keep-redundant-commits")]
        keep_redundant: bool,
        #[arg(long = "continue", conflicts_with_all = ["skip", "abort", "quit"])]
        cont: bool,
        #[arg(long = "skip", conflicts_with_all = ["cont", "abort", "quit"])]
        skip: bool,
        #[arg(long = "abort", conflicts_with_all = ["cont", "skip", "quit"])]
        abort: bool,
        #[arg(long = "quit", conflicts_with_all = ["cont", "skip", "abort"])]
        quit: bool,
    },
    BananaSplit {
        commit: Option<String>,
        #[arg(long = "by-dir")]
        by_dir: bool,
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
    },
    Revert {
        commit: Option<String>,
        #[arg(short = 'n', long = "no-commit", conflicts_with_all = ["cont", "abort"])]
        no_commit: bool,
        #[arg(long = "continue", conflicts_with_all = ["abort", "no_commit"])]
        cont: bool,
        #[arg(long = "abort", conflicts_with_all = ["cont", "no_commit"])]
        abort: bool
    },
    Stash {
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        #[arg(short = 'u', long = "include-untracked", conflicts_with = "all")]
        include_untracked: bool,
        #[arg(short = 'a', long = "all")]
        all: bool,
        #[command(subcommand)]
        action: Option<StashAction>
    },
    Bisect {
        #[command(subcommand)]
        action: BisectAction
    },
    Show {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    Tag {
        #[arg(short = 'l', long = "list")]
        list: bool,
        #[arg(short = 'd', long = "delete")]
        delete: bool,
        #[arg(short = 'a', long = "annotate")]
        annotate: bool,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(short = 'm', long = "message", value_name = "MSG")]
        messages: Vec<String>,
        #[arg(short = 'F', long = "file", value_name = "FILE")]
        file: Option<String>,
        #[arg(short = 'e', long = "edit")]
        edit: bool,
        /// -n[<num>]: lines of each tag message to print (attached number, like git)
        #[arg(short = 'n', num_args = 0..=1, default_missing_value = "1", value_name = "NUM")]
        lines: Option<String>,
        #[arg(long = "contains", value_name = "COMMIT")]
        contains: Option<String>,
        #[arg(long = "points-at", value_name = "OBJECT")]
        points_at: Option<String>,
        args: Vec<String>,
    },
    Describe {
        #[arg(long = "tags")]
        tags: bool,
        #[arg(long = "always")]
        always: bool,
        #[arg(long = "long")]
        long: bool,
        #[arg(long = "abbrev", value_name = "N", require_equals = true)]
        abbrev: Option<usize>,
        #[arg(long = "exact-match")]
        exact_match: bool,
        commits: Vec<String>,
    },
    Mv {
        source: PathBuf,
        destination: PathBuf,
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
    Config {
        key: Option<String>,
        value: Option<String>,
        #[arg(long = "global")]
        global: bool,
        #[arg(short = 'l', long = "list", conflicts_with_all = ["value", "unset"])]
        list: bool,
        #[arg(long = "unset", conflicts_with = "value")]
        unset: bool,
    },
    Rebase {
        upstream: Option<String>,
        branch: Option<String>,
        #[arg(long = "onto", value_name = "NEWBASE", conflicts_with = "keep_base")]
        onto: Option<String>,
        #[arg(long = "keep-base")]
        keep_base: bool,
        #[arg(long = "reapply-cherry-picks", overrides_with = "no_reapply_cherry_picks")]
        reapply_cherry_picks: bool,
        #[arg(long = "no-reapply-cherry-picks", overrides_with = "reapply_cherry_picks")]
        no_reapply_cherry_picks: bool,
        #[arg(short = 'i', long = "interactive", conflicts_with_all = ["cont", "abort", "skip", "edit_todo"])]
        interactive: bool,
        #[arg(long = "continue")]
        cont: bool,
        #[arg(long = "abort")]
        abort: bool,
        #[arg(long = "skip")]
        skip: bool,
        #[arg(long = "edit-todo")]
        edit_todo: bool,
    },
    Push {
        remote: Option<String>,
        refspec: Option<String>,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(short = 'u', long = "set-upstream")]
        set_upstream: bool,
    },
    Fetch {
        remote: Option<String>,
        branch: Option<String>,
    },
    Clone {
        repository: String,
        directory: Option<String>,
        #[arg(short = 'b', long = "branch")]
        branch: Option<String>,
        #[arg(short = 'o', long = "origin")]
        origin: Option<String>,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    Remote {
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        #[command(subcommand)]
        action: Option<RemoteAction>,
    },
    Blame {
        #[arg(short = 'L', value_name = "START,END")]
        line_range: Option<String>,
        #[arg(required = true, num_args = 1..=2)]
        args: Vec<String>,
    },
    Annotate {
        #[arg(short = 'L', value_name = "START,END")]
        line_range: Option<String>,
        #[arg(required = true, num_args = 1..=2)]
        args: Vec<String>,
    },
    Whatchanged {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    Archive {
        #[arg(short = 'f', long = "format")]
        format: Option<String>,
        #[arg(short = 'l', long = "list")]
        list: bool,
        #[arg(long = "prefix")]
        prefix: Option<String>,
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        #[arg(value_name = "TREE-ISH")]
        tree_ish: Option<String>,
        #[arg(value_name = "PATH")]
        paths: Vec<String>,
    },
    Reflog {
        #[command(subcommand)]
        action: Option<ReflogAction>,
    },
    Gc {
        #[arg(long = "prune", default_value = "14")]
        prune: String,
        #[arg(long = "reflog-expire", default_value = "90")]
        reflog_expire: String,
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
    },
    Prune {
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        /// Accepted for compatibility; prune shows no progress
        #[arg(long = "progress")]
        progress: bool,
        #[arg(long = "expire", value_name = "TIME")]
        expire: Option<String>,
        #[arg(value_name = "HEAD")]
        heads: Vec<String>,
    },
    #[command(visible_alias = "fsck-objects")]
    Fsck {
        #[arg(long = "connectivity-only")]
        connectivity_only: bool,
        #[arg(long = "no-dangling")]
        no_dangling: bool,
        #[arg(long = "unreachable")]
        unreachable: bool,
        #[arg(long = "no-reflogs")]
        no_reflogs: bool,
    },
    Grep {
        #[arg(short = 'i', long = "ignore-case")]
        ignore_case: bool,
        #[arg(short = 'v', long = "invert-match")]
        invert_match: bool,
        #[arg(short = 'n', long = "line-number")]
        line_number: bool,
        #[arg(short = 'c', long = "count")]
        count: bool,
        #[arg(short = 'l', long = "files-with-matches")]
        files_with_matches: bool,
        #[arg(short = 'L', long = "files-without-match")]
        files_without_match: bool,
        #[arg(short = 'F', long = "fixed-strings")]
        fixed_strings: bool,
        #[arg(long = "cached")]
        cached: bool,
        #[arg(short = 'e')]
        pattern_flag: Option<String>,
        args: Vec<String>,
        #[arg(last = true)]
        path_specs: Vec<String>,
    },
    CountObjects {
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        #[arg(short = 'H', long = "human-readable")]
        human_readable: bool,
    },
    RevParse {
        #[arg(long = "git-dir")]
        git_dir: bool,
        #[arg(long = "show-toplevel")]
        show_toplevel: bool,
        #[arg(long = "is-inside-work-tree")]
        is_inside_work_tree: bool,
        #[arg(long = "is-bare-repository")]
        is_bare_repository: bool,
        #[arg(long = "abbrev-ref")]
        abbrev_ref: bool,
        #[arg(long = "symbolic-full-name")]
        symbolic_full_name: bool,
        #[arg(long = "verify")]
        verify: bool,
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        #[arg(long = "short", num_args = 0..=1, default_missing_value = "7", require_equals = true, value_name = "N")]
        short: Option<usize>,
        #[arg(long = "sq")]
        sq: bool,
        revs: Vec<String>,
    },
    Shortlog {
        #[arg(short = 's', long = "summary")]
        summary: bool,
        #[arg(short = 'n', long = "numbered")]
        numbered: bool,
        #[arg(short = 'e', long = "email")]
        email: bool,
        #[arg(long = "all")]
        all: bool,
        rev: Option<String>,
    },
    LsFiles {
        #[arg(short = 'c', long = "cached")]
        cached: bool,
        #[arg(short = 'd', long = "deleted")]
        deleted: bool,
        #[arg(short = 'm', long = "modified")]
        modified: bool,
        #[arg(short = 'o', long = "others")]
        others: bool,
        #[arg(short = 'i', long = "ignored")]
        ignored: bool,
        #[arg(long = "exclude-standard")]
        exclude_standard: bool,
        #[arg(short = 's', long = "stage")]
        stage: bool,
        #[arg(short = 'u', long = "unmerged")]
        unmerged: bool,
        #[arg(short = 't')]
        show_tags: bool,
        #[arg(short = 'z')]
        null_terminated: bool,
        #[arg(value_name = "PATHSPEC")]
        pathspecs: Vec<String>,
    },
    CheckIgnore {
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        #[arg(short = 'n', long = "non-matching")]
        non_matching: bool,
        #[arg(long = "no-index")]
        no_index: bool,
        #[arg(long = "stdin")]
        stdin: bool,
        #[arg(short = 'z')]
        null_terminated: bool,
        #[arg(value_name = "PATHNAME")]
        paths: Vec<String>,
    },
    CheckRefFormat {
        #[arg(long = "normalize", visible_alias = "print")]
        normalize: bool,
        #[arg(long = "allow-onelevel", overrides_with = "no_allow_onelevel")]
        allow_onelevel: bool,
        #[arg(long = "no-allow-onelevel", overrides_with = "allow_onelevel")]
        no_allow_onelevel: bool,
        #[arg(long = "refspec-pattern")]
        refspec_pattern: bool,
        #[arg(
            long = "branch",
            value_name = "BRANCHNAME",
            allow_hyphen_values = true,
            conflicts_with_all = ["normalize", "allow_onelevel", "no_allow_onelevel", "refspec_pattern", "refname"]
        )]
        branch: Option<String>,
        #[arg(value_name = "REFNAME", required_unless_present = "branch")]
        refname: Option<String>,
    },
    Apply {
        #[arg(long = "check")]
        check: bool,
        #[arg(long = "stat")]
        stat: bool,
        #[arg(long = "numstat")]
        numstat: bool,
        #[arg(long = "summary")]
        summary: bool,
        #[arg(long = "apply")]
        apply: bool,
        #[arg(long = "cached")]
        cached: bool,
        #[arg(long = "index")]
        index: bool,
        #[arg(short = 'R', long = "reverse")]
        reverse: bool,
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        #[arg(short = 'p', value_name = "N", default_value = "1")]
        strip: usize,
        #[arg(value_name = "PATCH")]
        patches: Vec<String>,
    },
    #[command(allow_negative_numbers = true)]
    FormatPatch {
        #[arg(long = "max-count")]
        max_count: Option<usize>,
        #[arg(long = "root")]
        root: bool,
        #[arg(long = "stdout")]
        stdout: bool,
        #[arg(short = 'o', long = "output-directory")]
        output_directory: Option<String>,
        #[arg(short = 'n', long = "numbered", conflicts_with = "no_numbered")]
        numbered: bool,
        #[arg(short = 'N', long = "no-numbered")]
        no_numbered: bool,
        #[arg(long = "start-number", default_value = "1")]
        start_number: usize,
        #[arg(long = "subject-prefix", default_value = "PATCH")]
        subject_prefix: String,
        #[arg(short = 'k', long = "keep-subject", conflicts_with = "numbered")]
        keep_subject: bool,
        #[arg(long = "signature")]
        signature: Option<String>,
        #[arg(long = "no-signature")]
        no_signature: bool,
        #[arg(value_name = "REVISION")]
        revs: Vec<String>,
    },
    Am {
        #[arg(long = "continue", short = 'r', alias = "resolved", conflicts_with_all = ["skip", "abort", "quit", "show_current_patch"])]
        cont: bool,
        #[arg(long = "skip", conflicts_with_all = ["abort", "quit", "show_current_patch"])]
        skip: bool,
        #[arg(long = "abort", conflicts_with_all = ["quit", "show_current_patch"])]
        abort: bool,
        #[arg(long = "quit", conflicts_with = "show_current_patch")]
        quit: bool,
        #[arg(long = "show-current-patch", num_args = 0..=1, default_missing_value = "raw", require_equals = true, value_name = "raw|diff")]
        show_current_patch: Option<String>,
        #[arg(short = 'k', long = "keep-subject")]
        keep_subject: bool,
        #[arg(value_name = "MBOX")]
        mboxes: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    RevList {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    ShowRef {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    SymbolicRef {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    UpdateRef {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    PackRefs {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    ForEachRef {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    MergeTree {
        #[arg(long = "write-tree")]
        write_tree: bool,
        #[arg(long = "name-only")]
        name_only: bool,
        #[arg(long = "messages", overrides_with = "no_messages")]
        messages: bool,
        #[arg(long = "no-messages", overrides_with = "messages")]
        no_messages: bool,
        #[arg(short = 'z')]
        nul: bool,
        #[arg(long = "merge-base", value_name = "TREE-ISH")]
        merge_base: Option<String>,
        #[arg(long = "allow-unrelated-histories")]
        allow_unrelated_histories: bool,
        #[arg(long = "explain")]
        explain: bool,
        branch1: String,
        branch2: String,
    },
    Cherry {
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        #[arg(long = "abbrev", value_name = "N", num_args = 0..=1, require_equals = true, default_missing_value = "7")]
        abbrev: Option<usize>,
        upstream: Option<String>,
        head: Option<String>,
        limit: Option<String>,
    },
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
    Bundle {
        #[command(subcommand)]
        action: crate::objects::BundleAction,
    },
    #[command(disable_help_flag = true)]
    PatchId {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Mailsplit {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Mailinfo {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    InterpretTrailers {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    CheckMailmap {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    FmtMergeMsg {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    RequestPull {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Replace {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Notes {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    /// List unresolved conflicts with who changed what; `--explain <file>` shows each one side by side.
    #[command(disable_help_flag = true)]
    Conflicts {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    ShowBranch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    LastModified {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Repo {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    RangeDiff {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Hook {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Maintenance {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    ForEachRepo {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Replay {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Rerere {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Stripspace {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Column {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Var {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Version {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Help {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    NameRev {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Refs {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    MergeFile {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Mktree {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Mktag {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    UnpackFile {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    GetTarCommitId {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    UpdateIndex {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    CheckoutIndex {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    ReadTree {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    DiffTree {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    DiffIndex {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    DiffFiles {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    ShowIndex {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    VerifyPack {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    IndexPack {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    UnpackObjects {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    PrunePacked {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    PackObjects {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Repack {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    CommitGraph {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    MultiPackIndex {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    FastExport {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    FastImport {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    SparseCheckout {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Submodule {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
}

/// The arguments after `subcommand` exactly as typed. clap drops a leading `--`, but `log`,
/// `show` and `rev-list` need it to tell paths from revisions, and `show-ref` and
/// `for-each-ref` to tell patterns from options.
fn raw_args_after(subcommand: &str, parsed: Vec<String>) -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().position(|a| a == subcommand) {
        Some(pos) => args[pos + 1..].to_vec(),
        None => parsed,
    }
}

pub fn handle_commands() -> Result<()> {
    let cli = Cli::parse();
    use crate::objects::Verbosity;
    crate::ui::init(if cli.plain { Some(Verbosity::Quiet) } else if cli.teach { Some(Verbosity::Teach) } else if cli.explain { Some(Verbosity::Explain) } else { None });
    crate::ui::begin();

    // These commands copy, verify or delete objects by hash, so they must not see replacements.
    if matches!(
        *cli.command,
        Commands::Fsck { .. } | Commands::Gc { .. } | Commands::Prune { .. } | Commands::CountObjects { .. } | Commands::Bundle { .. } | Commands::Push { .. } | Commands::Fetch { .. } | Commands::Clone { .. } | Commands::Pull { .. } | Commands::Replace { .. }
    ) {
        crate::helpers::objects::disable_replace_objects();
    }

    match *cli.command {
        Commands::Init => {
            crate::commands::init()?;
        }
        Commands::HashObject { write, file} => {
            crate::commands::hash_object(write, file)?;
        }
        Commands::CatFile { pretty_print, object_hash } => {
            crate::commands::cat_file(pretty_print, object_hash)?;
        }
        Commands::WriteTree => {
            crate::commands::write_tree()?;
        }
        Commands::Add { patch: true, paths } => {
            crate::commands::add_patch(paths.iter().map(|p| p.to_string_lossy().to_string()).collect())?;
        }
        Commands::Add { patch: false, paths } => {
            crate::commands::add(paths)?;
        }
        Commands::LsTree { name_only, tree_hash } => {
            crate::commands::ls_tree(name_only, tree_hash)?;
        }
        Commands::CommitTree { tree_hash, parent_hash, message } => {
            crate::commands::commit_tree(tree_hash, parent_hash, message)?;
        }
        Commands::Commit {
            messages,
            file,
            reuse_message,
            reedit_message,
            all,
            amend,
            edit,
            no_edit,
            author,
            date,
            reset_author,
            allow_empty,
            allow_empty_message,
            signoff,
            cleanup,
            quiet,
        } => {
            let opts = crate::objects::CommitOptions {
                messages,
                file,
                reedit: reedit_message.is_some(),
                reuse_message: reuse_message.or(reedit_message),
                all,
                amend,
                edit,
                no_edit,
                author,
                date,
                reset_author,
                allow_empty,
                allow_empty_message,
                signoff,
                cleanup_strip: cleanup.as_deref() == Some("strip"),
                quiet,
            };
            crate::commands::commit(opts)?;
        }
        Commands::Log { args } => {
            crate::commands::log(raw_args_after("log", args))?;
        }
        Commands::Status { short, branch, porcelain, nul, untracked } => {
            if let Some(v) = porcelain.as_deref().filter(|v| *v != "v1") {
                anyhow::bail!("fatal: unsupported porcelain version '{}'", v);
            }
            let untracked = untracked.unwrap_or_else(|| "normal".to_string());
            if !["no", "normal", "all"].contains(&untracked.as_str()) {
                anyhow::bail!("fatal: Invalid untracked files mode '{}'", untracked);
            }
            // `-z` implies the porcelain format, like git.
            crate::commands::status(crate::objects::StatusOptions { short: short || porcelain.is_some() || nul, branch, nul, untracked })?;
        }
        Commands::Branch { args } => {
            crate::commands::branch(raw_args_after("branch", args))?;
        }
        Commands::Switch { branch, create, detach, force } => {
            crate::commands::die_if_operation_in_progress()?;
            crate::commands::switch(branch, create, detach, force)?;
        }
        Commands::Checkout { patch: true, .. } => {
            crate::commands::checkout_patch(raw_args_after("checkout", Vec::new()))?;
        }
        Commands::Checkout { target, create_branch: None, ours, theirs, merge, conflict, paths, .. } if !paths.is_empty() => {
            let stage = crate::commands::writeout_stage(ours, theirs, merge, conflict)?;
            crate::commands::checkout_paths(target, stage, paths.iter().map(PathBuf::from).collect())?;
        }
        Commands::Checkout { target, create_branch, detach, force, ours, theirs, merge, conflict, .. } => {
            if ours || theirs || merge || conflict.is_some() {
                anyhow::bail!("fatal: --ours, --theirs, --merge and --conflict need paths after '--' (e.g. 'rgit checkout --theirs -- <file>')");
            }
            crate::commands::checkout(target, create_branch, detach, force)?;
        }
        Commands::Restore { patch: true, files, staged, worktree, source, .. } => {
            crate::commands::restore_patch(files.iter().map(|p| p.to_string_lossy().to_string()).collect(), staged, worktree, source)?;
        }
        Commands::Restore { patch: false, files, staged, worktree, source, ours, theirs, merge, conflict } => {
            let stage = crate::commands::writeout_stage(ours, theirs, merge, conflict)?;
            crate::commands::restore(files, staged, worktree, source, stage)?;
        }
        Commands::Diff { args } => {
            crate::commands::diff(raw_args_after("diff", args))?;
        }
        Commands::Merge { ff: _, no_ff, ff_only, squash, no_squash: _, commit, no_commit, messages, file, strategy, strategy_options, abort, cont, quit, no_stat, stat: _, quiet, no_edit: _, allow_unrelated, commits } => {
            use crate::objects::{FastForwardMode, MergeOptions};
            let ff = if no_ff { FastForwardMode::Never } else if ff_only { FastForwardMode::Only } else { FastForwardMode::Allow };
            let commit = if commit { Some(true) } else if no_commit { Some(false) } else { None };
            crate::commands::merge(MergeOptions { commits, ff, squash, commit, messages, file, strategy, strategy_options, abort, cont, quit, no_stat, quiet, allow_unrelated })?;
        }
        Commands::MergeBase { all, is_ancestor, octopus, commits } => {
            crate::commands::merge_base(all, is_ancestor, octopus, commits)?;
        }
        Commands::Rm { files, force, cached, recursive } => {
            crate::commands::rm(files, force, cached, recursive)?;
        }
        Commands::Reset { patch: true, soft, hard, .. } => {
            if soft || hard {
                anyhow::bail!("fatal: options '--patch' and '--{}' cannot be used together", if soft { "soft" } else { "hard" });
            }
            crate::commands::reset_patch(raw_args_after("reset", Vec::new()))?;
        }
        Commands::Reset { commit, soft, mixed, hard, paths, .. } => {
            crate::commands::reset(commit, soft, mixed, hard, paths)?;
        }
        Commands::Clean { dry_run, force, dirs, ignored, only_ignored } => {
            crate::commands::clean(dry_run, force, dirs, ignored, only_ignored)?;
        }
        Commands::CherryPick { commits, no_commit, commit: _, record_origin, signoff, mainline, ff, allow_empty, allow_empty_message, keep_redundant, cont, skip, abort, quit } => {
            crate::commands::cherry_pick(crate::objects::CherryPickOptions { commits, no_commit, record_origin, signoff, mainline, ff, allow_empty, allow_empty_message, keep_redundant, cont, skip, abort, quit })?;
        }
        Commands::BananaSplit { commit, by_dir, dry_run } => {
            crate::commands::banana_split(commit, by_dir, dry_run)?;
        }
        Commands::Revert { commit, no_commit, cont, abort } => {
            crate::commands::revert(commit, no_commit, cont, abort)?;
        }
        Commands::Stash { patch: true, action: None, .. } => {
            crate::commands::stash_patch(None, Vec::new())?;
        }
        Commands::Stash { action: None, include_untracked, all, .. } if include_untracked || all => {
            crate::commands::stash(Some(crate::objects::StashAction::Push { message: None, patch: false, include_untracked, all, paths: Vec::new() }))?;
        }
        Commands::Stash { action, .. } => {
            crate::commands::stash(action)?;
        }
        Commands::Bisect { action } => {
            crate::commands::bisect(action)?;
        }
        Commands::Show { args } => {
            crate::commands::show(raw_args_after("show", args))?;
        }
        Commands::Tag { list, delete, annotate, force, messages, file, edit, lines, contains, points_at, mut args } => {
            // git's `-n` only takes an attached number (`-n2`); a separate word after it is a pattern.
            let lines = lines.map(|value| match value.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    args.insert(0, value);
                    1
                }
            });
            let opts = crate::objects::TagOptions { list, delete, annotate, force, messages, file, edit, lines, contains, points_at, args };
            crate::commands::tag(opts)?;
        }
        Commands::Describe { tags, always, long, abbrev, exact_match, commits } => {
            let opts = crate::objects::DescribeOptions { tags, always, long, abbrev, exact_match, commits };
            crate::commands::describe(opts)?;
        }
        Commands::Mv { source, destination, force} => {
            crate::commands::mv(source, destination, force)?;
        }
        Commands::Config { key, value, global, list, unset } => {
            crate::commands::config(key, value, global, list, unset)?;
        }
        Commands::Rebase { upstream, branch, onto, keep_base, reapply_cherry_picks, no_reapply_cherry_picks, interactive, cont, abort, skip, edit_todo } => {
            let reapply_cherry_picks = if reapply_cherry_picks { Some(true) } else if no_reapply_cherry_picks { Some(false) } else { None };
            crate::commands::rebase(crate::objects::RebaseOptions { upstream, branch, onto, keep_base, reapply_cherry_picks, interactive, cont, abort, skip, edit_todo })?;
        }
        Commands::Push { remote, refspec, force, set_upstream } => {
            crate::commands::push(remote, refspec, force, set_upstream)?;
        }
        Commands::Fetch { remote, branch } => {
            crate::commands::fetch(remote, branch)?;
        }
        Commands::Clone { repository, directory, branch, origin } => {
            crate::commands::clone(repository, directory, branch, origin)?;
        }
        Commands::Pull { remote, branch } => {
            crate::commands::pull(remote, branch)?;
        }
        Commands::Remote { verbose, action } => {
            crate::commands::remote(verbose, action)?;
        }
        Commands::Blame { line_range, args } => {
            crate::commands::blame(args, line_range)?;
        }
        Commands::Annotate { line_range, args } => {
            crate::commands::annotate(args, line_range)?;
        }
        Commands::Whatchanged { args } => {
            crate::commands::whatchanged(raw_args_after("whatchanged", args))?;
        }
        Commands::Archive { format, list, prefix, output, tree_ish, paths } => {
            crate::commands::archive(format, list, prefix, output, tree_ish, paths)?;
        }
        Commands::Reflog { action } => {
            crate::commands::reflog(action)?;
        }
        Commands::Gc { prune, reflog_expire, dry_run } => {
            crate::commands::gc(prune, reflog_expire, dry_run)?;
        }
        Commands::Prune { dry_run, verbose, progress: _, expire, heads } => {
            crate::commands::prune(dry_run, verbose, expire, heads)?;
        }
        Commands::Fsck { connectivity_only, no_dangling, unreachable, no_reflogs } => {
            crate::commands::fsck(connectivity_only, no_dangling, unreachable, no_reflogs)?;
        }
        Commands::Grep {
            ignore_case,
            invert_match,
            line_number,
            count,
            files_with_matches,
            files_without_match,
            fixed_strings,
            cached,
            pattern_flag,
            args,
            path_specs,
        } => {
            let (pattern, remaining_args) = if let Some(p) = pattern_flag {
                (p, args)
            } else {
                if args.is_empty() {
                    anyhow::bail!("fatal: no pattern given");
                }
                (args[0].clone(), args[1..].to_vec())
            };

            let mut revisions = Vec::new();
            let mut final_path_specs = path_specs;

            if final_path_specs.is_empty() {
                for arg in remaining_args {
                    if crate::helpers::resolve_commit_from_source(&arg).is_ok() {
                        revisions.push(arg);
                    } else {
                        final_path_specs.push(arg);
                    }
                }
            } else {
                revisions.extend(remaining_args);
            }

            let options = crate::objects::GrepOptions {
                ignore_case,
                invert_match,
                line_number,
                count,
                files_with_matches,
                files_without_match,
                fixed_strings,
                cached,
            };

            crate::commands::grep(pattern, revisions, final_path_specs, options)?;
        }
        Commands::CountObjects { verbose, human_readable } => {
            crate::commands::count_objects(verbose, human_readable)?;
        }
        Commands::RevParse {
            git_dir,
            show_toplevel,
            is_inside_work_tree,
            is_bare_repository,
            abbrev_ref,
            symbolic_full_name,
            verify,
            quiet,
            short,
            sq,
            revs,
        } => {
            let opts = crate::objects::RevParseOptions {
                git_dir,
                show_toplevel,
                is_inside_work_tree,
                is_bare_repository,
                abbrev_ref,
                symbolic_full_name,
                verify,
                quiet,
                short,
                sq,
                revs,
            };
            crate::commands::rev_parse(opts)?;
        }
        Commands::Shortlog {
            summary,
            numbered,
            email,
            all,
            rev,
        } => {
            let opts = crate::objects::ShortlogOptions {
                summary,
                numbered,
                email,
                all,
                rev,
            };
            crate::commands::shortlog(opts)?;
        }
        Commands::LsFiles {
            cached,
            deleted,
            modified,
            others,
            ignored,
            exclude_standard,
            stage,
            unmerged,
            show_tags,
            null_terminated,
            pathspecs,
        } => {
            let opts = crate::objects::LsFilesOptions {
                cached,
                deleted,
                modified,
                others,
                ignored,
                exclude_standard,
                // Unmerged entries only make sense with their stage numbers.
                stage: stage || unmerged,
                unmerged,
                show_tags,
                null_terminated,
                pathspecs,
            };
            crate::commands::ls_files(opts)?;
        }
        Commands::CheckIgnore {
            quiet,
            verbose,
            non_matching,
            no_index,
            stdin,
            null_terminated,
            paths,
        } => {
            let opts = crate::objects::CheckIgnoreOptions {
                quiet,
                verbose,
                non_matching,
                no_index,
                stdin,
                null_terminated,
                paths,
            };
            crate::commands::check_ignore(opts)?;
        }
        Commands::CheckRefFormat {
            normalize,
            allow_onelevel,
            no_allow_onelevel: _,
            refspec_pattern,
            branch,
            refname,
        } => {
            let opts = crate::objects::CheckRefFormatOptions {
                normalize,
                allow_onelevel,
                refspec_pattern,
                branch,
                refname,
            };
            crate::commands::check_ref_format(opts)?;
        }
        Commands::Apply {
            check,
            stat,
            numstat,
            summary,
            apply,
            cached,
            index,
            reverse,
            verbose,
            strip,
            patches,
        } => {
            let opts = crate::objects::ApplyOptions {
                check,
                stat,
                numstat,
                summary,
                apply,
                cached,
                index,
                reverse,
                verbose,
                strip,
                patches,
            };
            crate::commands::apply(opts)?;
        }
        Commands::FormatPatch {
            max_count,
            root,
            stdout,
            output_directory,
            numbered,
            no_numbered,
            start_number,
            subject_prefix,
            keep_subject,
            signature,
            no_signature,
            revs,
        } => {
            let opts = crate::objects::FormatPatchOptions {
                revs,
                max_count,
                root,
                stdout,
                output_directory,
                numbered,
                no_numbered,
                start_number,
                subject_prefix,
                keep_subject,
                signature,
                no_signature,
            };
            crate::commands::format_patch(opts)?;
        }
        Commands::Am {
            cont,
            skip,
            abort,
            quit,
            show_current_patch,
            keep_subject,
            mboxes,
        } => {
            let opts = crate::objects::AmOptions {
                mboxes,
                cont,
                skip,
                abort,
                quit,
                show_current_patch,
                keep_subject,
            };
            crate::commands::am(opts)?;
        }
        Commands::RevList { args } => {
            crate::commands::rev_list(raw_args_after("rev-list", args))?;
        }
        Commands::ShowRef { args } => {
            crate::commands::show_ref(raw_args_after("show-ref", args))?;
        }
        Commands::SymbolicRef { args } => {
            crate::commands::symbolic_ref(raw_args_after("symbolic-ref", args))?;
        }
        Commands::UpdateRef { args } => {
            crate::commands::update_ref(raw_args_after("update-ref", args))?;
        }
        Commands::PackRefs { args } => {
            crate::commands::pack_refs(raw_args_after("pack-refs", args))?;
        }
        Commands::ForEachRef { args } => {
            crate::commands::for_each_ref(raw_args_after("for-each-ref", args))?;
        }
        Commands::MergeTree {
            write_tree: _,
            name_only,
            messages,
            no_messages,
            nul,
            merge_base,
            allow_unrelated_histories,
            explain,
            branch1,
            branch2,
        } => {
            let opts = crate::objects::MergeTreeOptions {
                name_only,
                messages: if messages { Some(true) } else if no_messages { Some(false) } else { None },
                nul,
                merge_base,
                allow_unrelated_histories,
                explain,
                branch1,
                branch2,
            };
            crate::commands::merge_tree(opts)?;
        }
        Commands::Cherry {
            verbose,
            abbrev,
            upstream,
            head,
            limit,
        } => {
            let opts = crate::objects::CherryOptions {
                verbose,
                abbrev,
                upstream,
                head,
                limit,
            };
            crate::commands::cherry(opts)?;
        }
        Commands::Worktree { action } => {
            crate::commands::worktree(action)?;
        }
        Commands::Bundle { action } => {
            crate::commands::bundle(action)?;
        }
        Commands::PatchId { args } => {
            crate::commands::patch_id(raw_args_after("patch-id", args))?;
        }
        Commands::Mailsplit { args } => {
            crate::commands::mailsplit(raw_args_after("mailsplit", args))?;
        }
        Commands::Mailinfo { args } => {
            crate::commands::mailinfo(raw_args_after("mailinfo", args))?;
        }
        Commands::InterpretTrailers { args } => {
            crate::commands::interpret_trailers(raw_args_after("interpret-trailers", args))?;
        }
        Commands::CheckMailmap { args } => {
            crate::commands::check_mailmap(raw_args_after("check-mailmap", args))?;
        }
        Commands::FmtMergeMsg { args } => {
            crate::commands::fmt_merge_msg(raw_args_after("fmt-merge-msg", args))?;
        }
        Commands::RequestPull { args } => {
            crate::commands::request_pull(raw_args_after("request-pull", args))?;
        }
        Commands::Replace { args } => {
            crate::commands::replace(raw_args_after("replace", args))?;
        }
        Commands::Notes { args } => {
            crate::commands::notes(raw_args_after("notes", args))?;
        }
        Commands::Conflicts { args } => {
            crate::commands::conflicts(raw_args_after("conflicts", args))?;
        }
        Commands::ShowBranch { args } => {
            crate::commands::show_branch(raw_args_after("show-branch", args))?;
        }
        Commands::LastModified { args } => {
            crate::commands::last_modified(raw_args_after("last-modified", args))?;
        }
        Commands::Repo { args } => {
            crate::commands::repo(raw_args_after("repo", args))?;
        }
        Commands::RangeDiff { args } => {
            crate::commands::range_diff(raw_args_after("range-diff", args))?;
        }
        Commands::Hook { args } => {
            crate::commands::hook(raw_args_after("hook", args))?;
        }
        Commands::Maintenance { args } => {
            crate::commands::maintenance(raw_args_after("maintenance", args))?;
        }
        Commands::ForEachRepo { args } => {
            crate::commands::for_each_repo(raw_args_after("for-each-repo", args))?;
        }
        Commands::Replay { args } => {
            crate::commands::replay(raw_args_after("replay", args))?;
        }
        Commands::Rerere { args } => {
            crate::commands::rerere(raw_args_after("rerere", args))?;
        }
        Commands::Stripspace { args } => {
            crate::commands::stripspace(raw_args_after("stripspace", args))?;
        }
        Commands::Column { args } => {
            crate::commands::column(raw_args_after("column", args))?;
        }
        Commands::Var { args } => {
            crate::commands::var(raw_args_after("var", args))?;
        }
        Commands::Version { args } => {
            crate::commands::version(raw_args_after("version", args))?;
        }
        Commands::Help { args } => {
            crate::commands::help(raw_args_after("help", args))?;
        }
        Commands::NameRev { args } => {
            crate::commands::name_rev(raw_args_after("name-rev", args))?;
        }
        Commands::Refs { args } => {
            crate::commands::refs(raw_args_after("refs", args))?;
        }
        Commands::MergeFile { args } => {
            crate::commands::merge_file(raw_args_after("merge-file", args))?;
        }
        Commands::Mktree { args } => {
            crate::commands::mktree(raw_args_after("mktree", args))?;
        }
        Commands::Mktag { args } => {
            crate::commands::mktag(raw_args_after("mktag", args))?;
        }
        Commands::UnpackFile { args } => {
            crate::commands::unpack_file(raw_args_after("unpack-file", args))?;
        }
        Commands::GetTarCommitId { args } => {
            crate::commands::get_tar_commit_id(raw_args_after("get-tar-commit-id", args))?;
        }
        Commands::UpdateIndex { args } => {
            crate::commands::update_index(raw_args_after("update-index", args))?;
        }
        Commands::CheckoutIndex { args } => {
            crate::commands::checkout_index(raw_args_after("checkout-index", args))?;
        }
        Commands::ReadTree { args } => {
            crate::commands::read_tree(raw_args_after("read-tree", args))?;
        }
        Commands::DiffTree { args } => {
            crate::commands::diff_tree(raw_args_after("diff-tree", args))?;
        }
        Commands::DiffIndex { args } => {
            crate::commands::diff_index(raw_args_after("diff-index", args))?;
        }
        Commands::DiffFiles { args } => {
            crate::commands::diff_files(raw_args_after("diff-files", args))?;
        }
        Commands::ShowIndex { args } => {
            crate::commands::show_index(raw_args_after("show-index", args))?;
        }
        Commands::VerifyPack { args } => {
            crate::commands::verify_pack(raw_args_after("verify-pack", args))?;
        }
        Commands::IndexPack { args } => {
            crate::commands::index_pack(raw_args_after("index-pack", args))?;
        }
        Commands::UnpackObjects { args } => {
            crate::commands::unpack_objects(raw_args_after("unpack-objects", args))?;
        }
        Commands::PrunePacked { args } => {
            crate::commands::prune_packed(raw_args_after("prune-packed", args))?;
        }
        Commands::PackObjects { args } => {
            crate::commands::pack_objects(raw_args_after("pack-objects", args))?;
        }
        Commands::Repack { args } => {
            crate::commands::repack(raw_args_after("repack", args))?;
        }
        Commands::CommitGraph { args } => {
            crate::commands::commit_graph(raw_args_after("commit-graph", args))?;
        }
        Commands::MultiPackIndex { args } => {
            crate::commands::multi_pack_index(raw_args_after("multi-pack-index", args))?;
        }
        Commands::FastExport { args } => {
            crate::commands::fast_export(raw_args_after("fast-export", args))?;
        }
        Commands::FastImport { args } => {
            crate::commands::fast_import(raw_args_after("fast-import", args))?;
        }
        Commands::SparseCheckout { args } => {
            crate::commands::sparse_checkout(raw_args_after("sparse-checkout", args))?;
        }
        Commands::Submodule { args } => {
            crate::commands::submodule(raw_args_after("submodule", args))?;
        }
    }

    Ok(())
}
