//! Command-line interface of `rgit add`, `rgit status`, `rgit rm`, `rgit mv`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

use std::path::PathBuf;

// Command-line arguments of `rgit add`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/staging/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_add`
#[derive(clap::Args, Debug)]
pub struct AddArgs {
    #[arg(short = 'p', long = "patch")]
    patch: bool,
    paths: Vec<PathBuf>
}

/// Runs `rgit add` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_add(args: AddArgs) -> anyhow::Result<()> {
    match args {
        AddArgs { patch: true, paths } => {
            crate::workspace::patch::add_patch(paths.iter().map(|p| p.to_string_lossy().to_string()).collect())?;
        }
        AddArgs { patch: false, paths } => {
            crate::workspace::staging::add(paths)?;
        }
    }
    Ok(())
}

// Command-line arguments of `rgit status`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/staging/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_status`
#[derive(clap::Args, Debug)]
pub struct StatusArgs {
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
}

/// Runs `rgit status` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_status(args: StatusArgs) -> anyhow::Result<()> {
    let StatusArgs { short, branch, porcelain, nul, untracked } = args;
    if let Some(v) = porcelain.as_deref().filter(|v| *v != "v1") {
        anyhow::bail!("fatal: unsupported porcelain version '{}'", v);
    }
    let untracked = untracked.unwrap_or_else(|| "normal".to_string());
    if !["no", "normal", "all"].contains(&untracked.as_str()) {
        anyhow::bail!("fatal: Invalid untracked files mode '{}'", untracked);
    }
    // `-z` implies the porcelain format, like git.
    crate::workspace::staging::status(crate::workspace::status::types::StatusOptions { short: short || porcelain.is_some() || nul, branch, nul, untracked })?;
    Ok(())
}

// Command-line arguments of `rgit rm`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/staging/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_rm`
#[derive(clap::Args, Debug)]
pub struct RmArgs {
    files: Vec<PathBuf>,
    #[arg(short = 'f', long = "force")]
    force: bool,
    #[arg(long = "cached")]
    cached: bool,
    #[arg(short = 'r', long = "recursive")]
    recursive: bool
}

/// Runs `rgit rm` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_rm(args: RmArgs) -> anyhow::Result<()> {
    let RmArgs { files, force, cached, recursive } = args;
    crate::workspace::staging::rm(files, force, cached, recursive)?;
    Ok(())
}

// Command-line arguments of `rgit mv`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/staging/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_mv`
#[derive(clap::Args, Debug)]
pub struct MvArgs {
    source: PathBuf,
    destination: PathBuf,
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit mv` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_mv(args: MvArgs) -> anyhow::Result<()> {
    let MvArgs { source, destination, force} = args;
    crate::workspace::staging::mv(source, destination, force)?;
    Ok(())
}
