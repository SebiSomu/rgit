//! Command-line interface of `rgit notes`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit notes`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/notes/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_notes`
#[derive(clap::Args, Debug)]
pub struct NotesArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit notes` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_notes(args: NotesArgs) -> anyhow::Result<()> {
    let NotesArgs { args } = args;
    crate::references::notes::notes(crate::cli::raw_args_after("notes", args))?;
    Ok(())
}
