//! Command-line interface of `rgit branch`, `rgit switch`, `rgit checkout`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

use std::path::PathBuf;

// Command-line arguments of `rgit branch`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/branch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_branch`
#[derive(clap::Args, Debug)]
pub struct BranchArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit branch` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_branch(args: BranchArgs) -> anyhow::Result<()> {
    let BranchArgs { args } = args;
    crate::workspace::branch::branch(crate::cli::raw_args_after("branch", args))?;
    Ok(())
}

// Command-line arguments of `rgit switch`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/branch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_switch`
#[derive(clap::Args, Debug)]
pub struct SwitchArgs {
    branch: String,
    #[arg(short = 'c', long = "create", conflicts_with = "force")]
    create: bool,
    #[arg(long = "detach", conflicts_with = "create")]
    detach: bool,
    #[arg(short = 'f', long = "force", conflicts_with = "create")]
    force: bool
}

/// Runs `rgit switch` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_switch(args: SwitchArgs) -> anyhow::Result<()> {
    let SwitchArgs { branch, create, detach, force } = args;
    crate::workspace::branch::die_if_operation_in_progress()?;
    crate::workspace::branch::switch(branch, create, detach, force)?;
    Ok(())
}

// Command-line arguments of `rgit checkout`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/branch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_checkout`
#[derive(clap::Args, Debug)]
pub struct CheckoutArgs {
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
}

/// Runs `rgit checkout` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_checkout(args: CheckoutArgs) -> anyhow::Result<()> {
    match args {
        CheckoutArgs { patch: true, .. } => {
            crate::workspace::patch::checkout_patch(crate::cli::raw_args_after("checkout", Vec::new()))?;
        }
        CheckoutArgs { target, create_branch: None, ours, theirs, merge, conflict, paths, .. } if !paths.is_empty() => {
            let stage = crate::workspace::restore::writeout_stage(ours, theirs, merge, conflict)?;
            crate::workspace::restore::checkout_paths(target, stage, paths.iter().map(PathBuf::from).collect())?;
        }
        CheckoutArgs { target, create_branch, detach, force, ours, theirs, merge, conflict, .. } => {
            if ours || theirs || merge || conflict.is_some() {
                anyhow::bail!("fatal: --ours, --theirs, --merge and --conflict need paths after '--' (e.g. 'rgit checkout --theirs -- <file>')");
            }
            crate::workspace::branch::checkout(target, create_branch, detach, force)?;
        }
    }
    Ok(())
}
