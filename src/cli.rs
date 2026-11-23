use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "rgit", version, about, long_about = None, disable_help_subcommand = true)]
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
    HashObject(crate::plumbing::basic::cli::HashObjectArgs),
    CatFile(crate::plumbing::basic::cli::CatFileArgs),
    WriteTree,
    #[command(visible_alias = "stage")]
    Add(crate::workspace::staging::cli::AddArgs),
    LsTree(crate::plumbing::basic::cli::LsTreeArgs),
    CommitTree(crate::workspace::commit::cli::CommitTreeArgs),
    Commit(crate::workspace::commit::cli::CommitArgs),
    Log(crate::history::log::cli::LogArgs),
    Status(crate::workspace::staging::cli::StatusArgs),
    #[command(disable_help_flag = true)]
    Branch(crate::workspace::branch::cli::BranchArgs),
    Switch(crate::workspace::branch::cli::SwitchArgs),
    Checkout(crate::workspace::branch::cli::CheckoutArgs),
    Restore(crate::workspace::restore::cli::RestoreArgs),
    Diff(crate::diff::diff::cli::DiffArgs),
    Merge(crate::merge::merge::cli::MergeArgs),
    MergeBase(crate::merge::merge::cli::MergeBaseArgs),
    Rm(crate::workspace::staging::cli::RmArgs),
    Reset(crate::workspace::reset::cli::ResetArgs),
    Clean(crate::workspace::clean::cli::CleanArgs),
    CherryPick(crate::merge::cherry_pick::cli::CherryPickArgs),
    BananaSplit(crate::merge::banana_split::cli::BananaSplitArgs),
    Revert(crate::merge::revert::cli::RevertArgs),
    Stash(crate::workspace::stash::cli::StashArgs),
    Bisect(crate::history::bisect::cli::BisectArgs),
    Show(crate::history::log::cli::ShowArgs),
    Tag(crate::references::tag::cli::TagArgs),
    Describe(crate::references::tag::cli::DescribeArgs),
    Mv(crate::workspace::staging::cli::MvArgs),
    Config(crate::plumbing::config::cli::ConfigArgs),
    Rebase(crate::merge::rebase::cli::RebaseArgs),
    Push(crate::remote::push::cli::PushArgs),
    Fetch(crate::remote::fetch::cli::FetchArgs),
    Clone(crate::remote::clone::cli::CloneArgs),
    Pull(crate::remote::pull::cli::PullArgs),
    Remote(crate::remote::remote::cli::RemoteArgs),
    Blame(crate::history::blame::cli::BlameArgs),
    Annotate(crate::history::blame::cli::AnnotateArgs),
    Whatchanged(crate::history::log::cli::WhatchangedArgs),
    Archive(crate::export::archive::cli::ArchiveArgs),
    Reflog(crate::references::reflog::cli::ReflogArgs),
    Gc(crate::storage::gc::cli::GcArgs),
    Prune(crate::storage::prune::cli::PruneArgs),
    #[command(visible_alias = "fsck-objects")]
    Fsck(crate::storage::fsck::cli::FsckArgs),
    Grep(crate::history::grep::cli::GrepArgs),
    CountObjects(crate::storage::count_objects::cli::CountObjectsArgs),
    RevParse(crate::plumbing::rev_parse::cli::RevParseArgs),
    Shortlog(crate::history::shortlog::cli::ShortlogArgs),
    LsFiles(crate::workspace::ls_files::cli::LsFilesArgs),
    CheckIgnore(crate::workspace::check_ignore::cli::CheckIgnoreArgs),
    CheckRefFormat(crate::references::check_ref_format::cli::CheckRefFormatArgs),
    Apply(crate::mail::apply::cli::ApplyArgs),
    #[command(allow_negative_numbers = true)]
    FormatPatch(crate::mail::format_patch::cli::FormatPatchArgs),
    Am(crate::mail::am::cli::AmArgs),
    #[command(disable_help_flag = true)]
    RevList(crate::history::rev_list::cli::RevListArgs),
    #[command(disable_help_flag = true)]
    ShowRef(crate::references::show_ref::cli::ShowRefArgs),
    #[command(disable_help_flag = true)]
    SymbolicRef(crate::references::symbolic_ref::cli::SymbolicRefArgs),
    #[command(disable_help_flag = true)]
    UpdateRef(crate::references::update_ref::cli::UpdateRefArgs),
    #[command(disable_help_flag = true)]
    PackRefs(crate::references::pack_refs::cli::PackRefsArgs),
    #[command(disable_help_flag = true)]
    ForEachRef(crate::references::for_each_ref::cli::ForEachRefArgs),
    MergeTree(crate::merge::merge_tree::cli::MergeTreeArgs),
    Cherry(crate::history::cherry::cli::CherryArgs),
    Worktree(crate::workspace::worktree::cli::WorktreeArgs),
    Bundle(crate::remote::bundle::cli::BundleArgs),
    #[command(disable_help_flag = true)]
    PatchId(crate::diff::patch_id::cli::PatchIdArgs),
    #[command(disable_help_flag = true)]
    Mailsplit(crate::mail::mailsplit::cli::MailsplitArgs),
    #[command(disable_help_flag = true)]
    Mailinfo(crate::mail::mailinfo::cli::MailinfoArgs),
    #[command(disable_help_flag = true)]
    InterpretTrailers(crate::mail::interpret_trailers::cli::InterpretTrailersArgs),
    #[command(disable_help_flag = true)]
    CheckMailmap(crate::mail::check_mailmap::cli::CheckMailmapArgs),
    #[command(disable_help_flag = true)]
    FmtMergeMsg(crate::merge::fmt_merge_msg::cli::FmtMergeMsgArgs),
    #[command(disable_help_flag = true)]
    RequestPull(crate::mail::request_pull::cli::RequestPullArgs),
    #[command(disable_help_flag = true)]
    Replace(crate::references::replace::cli::ReplaceArgs),
    #[command(disable_help_flag = true)]
    Notes(crate::references::notes::cli::NotesArgs),
    #[command(disable_help_flag = true)]
    /// List unresolved conflicts with who changed what; `--explain <file>` shows each one side by side.
    #[command(disable_help_flag = true)]
    Conflicts(crate::merge::conflicts::cli::ConflictsArgs),
    ShowBranch(crate::history::show_branch::cli::ShowBranchArgs),
    #[command(disable_help_flag = true)]
    LastModified(crate::history::last_modified::cli::LastModifiedArgs),
    #[command(disable_help_flag = true)]
    Repo(crate::plumbing::repo_info::cli::RepoArgs),
    #[command(disable_help_flag = true)]
    RangeDiff(crate::history::range_diff::cli::RangeDiffArgs),
    #[command(disable_help_flag = true)]
    Hook(crate::plumbing::hook::cli::HookArgs),
    #[command(disable_help_flag = true)]
    Maintenance(crate::storage::maintenance::cli::MaintenanceArgs),
    #[command(disable_help_flag = true)]
    ForEachRepo(crate::storage::maintenance::cli::ForEachRepoArgs),
    #[command(disable_help_flag = true)]
    Replay(crate::merge::replay::cli::ReplayArgs),
    #[command(disable_help_flag = true)]
    Rerere(crate::merge::rerere::cli::RerereArgs),
    #[command(disable_help_flag = true)]
    Stripspace(crate::plumbing::stripspace::cli::StripspaceArgs),
    #[command(disable_help_flag = true)]
    Column(crate::plumbing::column::cli::ColumnArgs),
    #[command(disable_help_flag = true)]
    Var(crate::plumbing::var::cli::VarArgs),
    #[command(disable_help_flag = true)]
    Version(crate::plumbing::version::cli::VersionArgs),
    #[command(disable_help_flag = true)]
    Help(crate::plumbing::help::cli::HelpArgs),
    #[command(disable_help_flag = true)]
    NameRev(crate::history::name_rev::cli::NameRevArgs),
    #[command(disable_help_flag = true)]
    Refs(crate::references::refs_cmd::cli::RefsArgs),
    #[command(disable_help_flag = true)]
    MergeFile(crate::merge::merge_file::cli::MergeFileCliArgs),
    #[command(disable_help_flag = true)]
    Mktree(crate::plumbing::mktree::cli::MktreeArgs),
    #[command(disable_help_flag = true)]
    Mktag(crate::plumbing::mktag::cli::MktagArgs),
    #[command(disable_help_flag = true)]
    UnpackFile(crate::plumbing::unpack_file::cli::UnpackFileArgs),
    #[command(disable_help_flag = true)]
    GetTarCommitId(crate::export::get_tar_commit_id::cli::GetTarCommitIdArgs),
    #[command(disable_help_flag = true)]
    UpdateIndex(crate::workspace::update_index::cli::UpdateIndexArgs),
    #[command(disable_help_flag = true)]
    CheckoutIndex(crate::workspace::checkout_index::cli::CheckoutIndexArgs),
    #[command(disable_help_flag = true)]
    ReadTree(crate::workspace::read_tree::cli::ReadTreeArgs),
    #[command(disable_help_flag = true)]
    DiffTree(crate::diff::diff_tree::cli::DiffTreeArgs),
    #[command(disable_help_flag = true)]
    DiffIndex(crate::diff::diff_index::cli::DiffIndexArgs),
    #[command(disable_help_flag = true)]
    DiffFiles(crate::diff::diff_files::cli::DiffFilesArgs),
    #[command(disable_help_flag = true)]
    ShowIndex(crate::storage::show_index::cli::ShowIndexArgs),
    #[command(disable_help_flag = true)]
    VerifyPack(crate::storage::verify_pack::cli::VerifyPackArgs),
    #[command(disable_help_flag = true)]
    IndexPack(crate::storage::index_pack::cli::IndexPackArgs),
    #[command(disable_help_flag = true)]
    UnpackObjects(crate::storage::unpack_objects::cli::UnpackObjectsArgs),
    #[command(disable_help_flag = true)]
    PrunePacked(crate::storage::prune_packed::cli::PrunePackedArgs),
    #[command(disable_help_flag = true)]
    PackObjects(crate::storage::pack_objects::cli::PackObjectsArgs),
    #[command(disable_help_flag = true)]
    Repack(crate::storage::repack::cli::RepackArgs),
    #[command(disable_help_flag = true)]
    CommitGraph(crate::storage::commit_graph::cli::CommitGraphArgs),
    #[command(disable_help_flag = true)]
    MultiPackIndex(crate::storage::multi_pack_index::cli::MultiPackIndexArgs),
    #[command(disable_help_flag = true)]
    FastExport(crate::export::fast_export::cli::FastExportArgs),
    #[command(disable_help_flag = true)]
    FastImport(crate::export::fast_import::cli::FastImportArgs),
    #[command(disable_help_flag = true)]
    SparseCheckout(crate::workspace::sparse_checkout::cli::SparseCheckoutArgs),
    #[command(disable_help_flag = true)]
    Submodule(crate::workspace::submodule::cli::SubmoduleArgs),
    /// Undo the last rgit operation (commit, merge, reset --hard, rebase, ...); `--list` shows the log.
    Undo(crate::workspace::undo::cli::UndoArgs),
    /// Bring back the operation most recently undone.
    Redo(crate::workspace::undo::cli::RedoArgs),
    /// "I just did something wrong": lists the most likely fixes; `rgit oops <n>` runs one.
    Oops(crate::workspace::oops::cli::OopsArgs),
}

/// The arguments after `subcommand` exactly as typed. clap drops a leading `--`, but `log`,
/// `show` and `rev-list` need it to tell paths from revisions, and `show-ref` and
/// `for-each-ref` to tell patterns from options.
pub(crate) fn raw_args_after(subcommand: &str, parsed: Vec<String>) -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().position(|a| a == subcommand) {
        Some(pos) => args[pos + 1..].to_vec(),
        None => parsed,
    }
}

pub fn handle_commands() -> Result<()> {
    let cli = Cli::parse();
    use crate::ui::types::Verbosity;
    crate::ui::init(if cli.plain { Some(Verbosity::Quiet) } else if cli.teach { Some(Verbosity::Teach) } else if cli.explain { Some(Verbosity::Explain) } else { None });
    crate::ui::begin();
    crate::workspace::undo::engine::begin_recording();

    // These commands copy, verify or delete objects by hash, so they must not see replacements.
    if matches!(
        *cli.command,
        Commands::Fsck(..) | Commands::Gc(..) | Commands::Prune(..) | Commands::CountObjects(..) | Commands::Bundle(..) | Commands::Push(..) | Commands::Fetch(..) | Commands::Clone(..) | Commands::Pull(..) | Commands::Replace(..)
    ) {
        crate::core::object_store::disable_replace_objects();
    }

    match *cli.command {
        Commands::Init => {
            crate::plumbing::basic::init()?;
        }
        Commands::HashObject(args) => crate::plumbing::basic::cli::dispatch_hash_object(args)?,
        Commands::CatFile(args) => crate::plumbing::basic::cli::dispatch_cat_file(args)?,
        Commands::WriteTree => {
            crate::plumbing::basic::write_tree()?;
        }
        Commands::Add(args) => crate::workspace::staging::cli::dispatch_add(args)?,
        Commands::LsTree(args) => crate::plumbing::basic::cli::dispatch_ls_tree(args)?,
        Commands::CommitTree(args) => crate::workspace::commit::cli::dispatch_commit_tree(args)?,
        Commands::Commit(args) => crate::workspace::commit::cli::dispatch_commit(args)?,
        Commands::Log(args) => crate::history::log::cli::dispatch_log(args)?,
        Commands::Status(args) => crate::workspace::staging::cli::dispatch_status(args)?,
        Commands::Branch(args) => crate::workspace::branch::cli::dispatch_branch(args)?,
        Commands::Switch(args) => crate::workspace::branch::cli::dispatch_switch(args)?,
        Commands::Checkout(args) => crate::workspace::branch::cli::dispatch_checkout(args)?,
        Commands::Restore(args) => crate::workspace::restore::cli::dispatch_restore(args)?,
        Commands::Diff(args) => crate::diff::diff::cli::dispatch_diff(args)?,
        Commands::Merge(args) => crate::merge::merge::cli::dispatch_merge(args)?,
        Commands::MergeBase(args) => crate::merge::merge::cli::dispatch_merge_base(args)?,
        Commands::Rm(args) => crate::workspace::staging::cli::dispatch_rm(args)?,
        Commands::Reset(args) => crate::workspace::reset::cli::dispatch_reset(args)?,
        Commands::Clean(args) => crate::workspace::clean::cli::dispatch_clean(args)?,
        Commands::CherryPick(args) => crate::merge::cherry_pick::cli::dispatch_cherry_pick(args)?,
        Commands::BananaSplit(args) => crate::merge::banana_split::cli::dispatch_banana_split(args)?,
        Commands::Revert(args) => crate::merge::revert::cli::dispatch_revert(args)?,
        Commands::Stash(args) => crate::workspace::stash::cli::dispatch_stash(args)?,
        Commands::Bisect(args) => crate::history::bisect::cli::dispatch_bisect(args)?,
        Commands::Show(args) => crate::history::log::cli::dispatch_show(args)?,
        Commands::Tag(args) => crate::references::tag::cli::dispatch_tag(args)?,
        Commands::Describe(args) => crate::references::tag::cli::dispatch_describe(args)?,
        Commands::Mv(args) => crate::workspace::staging::cli::dispatch_mv(args)?,
        Commands::Config(args) => crate::plumbing::config::cli::dispatch_config(args)?,
        Commands::Rebase(args) => crate::merge::rebase::cli::dispatch_rebase(args)?,
        Commands::Push(args) => crate::remote::push::cli::dispatch_push(args)?,
        Commands::Fetch(args) => crate::remote::fetch::cli::dispatch_fetch(args)?,
        Commands::Clone(args) => crate::remote::clone::cli::dispatch_clone(args)?,
        Commands::Pull(args) => crate::remote::pull::cli::dispatch_pull(args)?,
        Commands::Remote(args) => crate::remote::remote::cli::dispatch_remote(args)?,
        Commands::Blame(args) => crate::history::blame::cli::dispatch_blame(args)?,
        Commands::Annotate(args) => crate::history::blame::cli::dispatch_annotate(args)?,
        Commands::Whatchanged(args) => crate::history::log::cli::dispatch_whatchanged(args)?,
        Commands::Archive(args) => crate::export::archive::cli::dispatch_archive(args)?,
        Commands::Reflog(args) => crate::references::reflog::cli::dispatch_reflog(args)?,
        Commands::Gc(args) => crate::storage::gc::cli::dispatch_gc(args)?,
        Commands::Prune(args) => crate::storage::prune::cli::dispatch_prune(args)?,
        Commands::Fsck(args) => crate::storage::fsck::cli::dispatch_fsck(args)?,
        Commands::Grep(args) => crate::history::grep::cli::dispatch_grep(args)?,
        Commands::CountObjects(args) => crate::storage::count_objects::cli::dispatch_count_objects(args)?,
        Commands::RevParse(args) => crate::plumbing::rev_parse::cli::dispatch_rev_parse(args)?,
        Commands::Shortlog(args) => crate::history::shortlog::cli::dispatch_shortlog(args)?,
        Commands::LsFiles(args) => crate::workspace::ls_files::cli::dispatch_ls_files(args)?,
        Commands::CheckIgnore(args) => crate::workspace::check_ignore::cli::dispatch_check_ignore(args)?,
        Commands::CheckRefFormat(args) => crate::references::check_ref_format::cli::dispatch_check_ref_format(args)?,
        Commands::Apply(args) => crate::mail::apply::cli::dispatch_apply(args)?,
        Commands::FormatPatch(args) => crate::mail::format_patch::cli::dispatch_format_patch(args)?,
        Commands::Am(args) => crate::mail::am::cli::dispatch_am(args)?,
        Commands::RevList(args) => crate::history::rev_list::cli::dispatch_rev_list(args)?,
        Commands::ShowRef(args) => crate::references::show_ref::cli::dispatch_show_ref(args)?,
        Commands::SymbolicRef(args) => crate::references::symbolic_ref::cli::dispatch_symbolic_ref(args)?,
        Commands::UpdateRef(args) => crate::references::update_ref::cli::dispatch_update_ref(args)?,
        Commands::PackRefs(args) => crate::references::pack_refs::cli::dispatch_pack_refs(args)?,
        Commands::ForEachRef(args) => crate::references::for_each_ref::cli::dispatch_for_each_ref(args)?,
        Commands::MergeTree(args) => crate::merge::merge_tree::cli::dispatch_merge_tree(args)?,
        Commands::Cherry(args) => crate::history::cherry::cli::dispatch_cherry(args)?,
        Commands::Worktree(args) => crate::workspace::worktree::cli::dispatch_worktree(args)?,
        Commands::Bundle(args) => crate::remote::bundle::cli::dispatch_bundle(args)?,
        Commands::PatchId(args) => crate::diff::patch_id::cli::dispatch_patch_id(args)?,
        Commands::Mailsplit(args) => crate::mail::mailsplit::cli::dispatch_mailsplit(args)?,
        Commands::Mailinfo(args) => crate::mail::mailinfo::cli::dispatch_mailinfo(args)?,
        Commands::InterpretTrailers(args) => crate::mail::interpret_trailers::cli::dispatch_interpret_trailers(args)?,
        Commands::CheckMailmap(args) => crate::mail::check_mailmap::cli::dispatch_check_mailmap(args)?,
        Commands::FmtMergeMsg(args) => crate::merge::fmt_merge_msg::cli::dispatch_fmt_merge_msg(args)?,
        Commands::RequestPull(args) => crate::mail::request_pull::cli::dispatch_request_pull(args)?,
        Commands::Replace(args) => crate::references::replace::cli::dispatch_replace(args)?,
        Commands::Notes(args) => crate::references::notes::cli::dispatch_notes(args)?,
        Commands::Conflicts(args) => crate::merge::conflicts::cli::dispatch_conflicts(args)?,
        Commands::ShowBranch(args) => crate::history::show_branch::cli::dispatch_show_branch(args)?,
        Commands::LastModified(args) => crate::history::last_modified::cli::dispatch_last_modified(args)?,
        Commands::Repo(args) => crate::plumbing::repo_info::cli::dispatch_repo(args)?,
        Commands::RangeDiff(args) => crate::history::range_diff::cli::dispatch_range_diff(args)?,
        Commands::Hook(args) => crate::plumbing::hook::cli::dispatch_hook(args)?,
        Commands::Maintenance(args) => crate::storage::maintenance::cli::dispatch_maintenance(args)?,
        Commands::ForEachRepo(args) => crate::storage::maintenance::cli::dispatch_for_each_repo(args)?,
        Commands::Replay(args) => crate::merge::replay::cli::dispatch_replay(args)?,
        Commands::Rerere(args) => crate::merge::rerere::cli::dispatch_rerere(args)?,
        Commands::Stripspace(args) => crate::plumbing::stripspace::cli::dispatch_stripspace(args)?,
        Commands::Column(args) => crate::plumbing::column::cli::dispatch_column(args)?,
        Commands::Var(args) => crate::plumbing::var::cli::dispatch_var(args)?,
        Commands::Version(args) => crate::plumbing::version::cli::dispatch_version(args)?,
        Commands::Help(args) => crate::plumbing::help::cli::dispatch_help(args)?,
        Commands::NameRev(args) => crate::history::name_rev::cli::dispatch_name_rev(args)?,
        Commands::Refs(args) => crate::references::refs_cmd::cli::dispatch_refs(args)?,
        Commands::MergeFile(args) => crate::merge::merge_file::cli::dispatch_merge_file(args)?,
        Commands::Mktree(args) => crate::plumbing::mktree::cli::dispatch_mktree(args)?,
        Commands::Mktag(args) => crate::plumbing::mktag::cli::dispatch_mktag(args)?,
        Commands::UnpackFile(args) => crate::plumbing::unpack_file::cli::dispatch_unpack_file(args)?,
        Commands::GetTarCommitId(args) => crate::export::get_tar_commit_id::cli::dispatch_get_tar_commit_id(args)?,
        Commands::UpdateIndex(args) => crate::workspace::update_index::cli::dispatch_update_index(args)?,
        Commands::CheckoutIndex(args) => crate::workspace::checkout_index::cli::dispatch_checkout_index(args)?,
        Commands::ReadTree(args) => crate::workspace::read_tree::cli::dispatch_read_tree(args)?,
        Commands::DiffTree(args) => crate::diff::diff_tree::cli::dispatch_diff_tree(args)?,
        Commands::DiffIndex(args) => crate::diff::diff_index::cli::dispatch_diff_index(args)?,
        Commands::DiffFiles(args) => crate::diff::diff_files::cli::dispatch_diff_files(args)?,
        Commands::ShowIndex(args) => crate::storage::show_index::cli::dispatch_show_index(args)?,
        Commands::VerifyPack(args) => crate::storage::verify_pack::cli::dispatch_verify_pack(args)?,
        Commands::IndexPack(args) => crate::storage::index_pack::cli::dispatch_index_pack(args)?,
        Commands::UnpackObjects(args) => crate::storage::unpack_objects::cli::dispatch_unpack_objects(args)?,
        Commands::PrunePacked(args) => crate::storage::prune_packed::cli::dispatch_prune_packed(args)?,
        Commands::PackObjects(args) => crate::storage::pack_objects::cli::dispatch_pack_objects(args)?,
        Commands::Repack(args) => crate::storage::repack::cli::dispatch_repack(args)?,
        Commands::CommitGraph(args) => crate::storage::commit_graph::cli::dispatch_commit_graph(args)?,
        Commands::MultiPackIndex(args) => crate::storage::multi_pack_index::cli::dispatch_multi_pack_index(args)?,
        Commands::FastExport(args) => crate::export::fast_export::cli::dispatch_fast_export(args)?,
        Commands::FastImport(args) => crate::export::fast_import::cli::dispatch_fast_import(args)?,
        Commands::SparseCheckout(args) => crate::workspace::sparse_checkout::cli::dispatch_sparse_checkout(args)?,
        Commands::Submodule(args) => crate::workspace::submodule::cli::dispatch_submodule(args)?,
        Commands::Undo(args) => crate::workspace::undo::cli::dispatch_undo(args)?,
        Commands::Redo(args) => crate::workspace::undo::cli::dispatch_redo(args)?,
        Commands::Oops(args) => crate::workspace::oops::cli::dispatch_oops(args)?,
    }

    Ok(())
}
