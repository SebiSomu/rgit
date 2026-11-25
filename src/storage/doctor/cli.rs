//! Command-line interface of `rgit doctor`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit doctor`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/doctor/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_doctor`
#[derive(clap::Args, Debug)]
pub struct DoctorArgs {
    /// Apply the safe fixes: remove stale lock files, prune broken worktrees, make hooks executable.
    #[arg(long = "fix")]
    fix: bool,
    /// Skip the object check (the slow part on a big repository).
    #[arg(long = "quick")]
    quick: bool,
    /// Size from which an untracked file counts as huge (`500K`, `50M`, `1G`; default 50M).
    #[arg(long = "large", value_name = "SIZE", default_value = "50M")]
    large: String,
}

/// Runs `rgit doctor` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_doctor(args: DoctorArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    let Some(large) = crate::storage::doctor::engine::parse_size(&args.large) else {
        crate::ui::die_fatal(format!("'{}' is not a size (try 500K, 50M or 1G)", args.large));
    };
    crate::storage::doctor::doctor(args.fix, args.quick, large)
}
