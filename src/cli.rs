use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
pub(crate) use crate::objects::{BisectAction, ReflogAction, RemoteAction, StashAction, WorktreeAction};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Box<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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
    Add {
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
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
    },
    Log {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
        args: Vec<String>,
    },
    Status,
    Branch {
        name: Option<String>,
        #[arg(short = 'd', long = "delete", conflicts_with = "force_delete")]
        delete: bool,
        #[arg(short = 'D', conflicts_with = "delete")]
        force_delete: bool,
        #[arg(short = 'm', long = "move", value_name = "NEW")]
        rename: Option<String>
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
        force: bool
    },
    Restore {
        files: Vec<PathBuf>,
        #[arg(short = 'S', long = "staged")]
        staged: bool,
        #[arg(short = 'W', long = "worktree")]
        worktree: bool,
        #[arg(long = "source")]
        source: Option<String>
    },
    Diff {
        #[arg(long = "staged", alias = "cached")]
        staged: bool,
        commits: Vec<String>,
        #[arg(last = true)]
        paths: Vec<PathBuf>
    },
    Merge {
        branch: String
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
        commit: Option<String>,
        #[arg(short = 'n', long = "no-commit", conflicts_with_all = ["cont", "abort"])]
        no_commit: bool,
        #[arg(long = "continue", conflicts_with_all = ["abort", "no_commit"])]
        cont: bool,
        #[arg(long = "abort", conflicts_with_all = ["cont", "no_commit"])]
        abort: bool
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
}

/// The arguments after `subcommand` exactly as typed. clap drops a leading `--`, but `log`,
/// `show` and `rev-list` need it to tell paths from revisions.
fn raw_args_after(subcommand: &str, parsed: Vec<String>) -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().position(|a| a == subcommand) {
        Some(pos) => args[pos + 1..].to_vec(),
        None => parsed,
    }
}

pub fn handle_commands() -> Result<()> {
    let cli = Cli::parse();

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
        Commands::Add { paths } => {
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
                quiet,
            };
            crate::commands::commit(opts)?;
        }
        Commands::Log { args } => {
            crate::commands::log(raw_args_after("log", args))?;
        }
        Commands::Status => {
            crate::commands::status()?;
        }
        Commands::Branch { name, delete, force_delete, rename } => {
            crate::commands::branch(name, delete, force_delete, rename)?;
        }
        Commands::Switch { branch, create, detach, force } => {
            crate::commands::switch(branch, create, detach, force)?;
        }
        Commands::Checkout { target, create_branch, detach, force } => {
            crate::commands::checkout(target, create_branch, detach, force)?;
        }
        Commands::Restore { files, staged, worktree, source } => {
            crate::commands::restore(files, staged, worktree, source)?;
        }
        Commands::Diff { staged, commits, paths } => {
            let (commit_a, commit_b) = match commits.len() {
                0 => (None, None),
                1 => (Some(commits[0].clone()), None),
                2 => (Some(commits[0].clone()), Some(commits[1].clone())),
                _ => anyhow::bail!("usage: rgit diff [<commit> [<commit>]] [-- <path>...]"),
            };
            crate::commands::diff(staged, commit_a, commit_b, paths)?;
        }
        Commands::Merge { branch } => {
            crate::commands::merge(branch)?;
        }
        Commands::MergeBase { all, is_ancestor, octopus, commits } => {
            crate::commands::merge_base(all, is_ancestor, octopus, commits)?;
        }
        Commands::Rm { files, force, cached, recursive } => {
            crate::commands::rm(files, force, cached, recursive)?;
        }
        Commands::Reset { commit, soft, mixed, hard, paths } => {
            crate::commands::reset(commit, soft, mixed, hard, paths)?;
        }
        Commands::Clean { dry_run, force, dirs, ignored, only_ignored } => {
            crate::commands::clean(dry_run, force, dirs, ignored, only_ignored)?;
        }
        Commands::CherryPick { commit, no_commit, cont, abort } => {
            crate::commands::cherry_pick(commit, no_commit, cont, abort)?;
        }
        Commands::BananaSplit { commit, by_dir, dry_run } => {
            crate::commands::banana_split(commit, by_dir, dry_run)?;
        }
        Commands::Revert { commit, no_commit, cont, abort } => {
            crate::commands::revert(commit, no_commit, cont, abort)?;
        }
        Commands::Stash { action } => {
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
       Commands::Rebase { upstream, interactive, cont, abort, skip, edit_todo } => {
            crate::commands::rebase(upstream, interactive, cont, abort, skip, edit_todo)?;
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
                stage,
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
    }

    Ok(())
}
