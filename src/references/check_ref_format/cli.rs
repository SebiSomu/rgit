//! Command-line interface of `rgit check-ref-format`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit check-ref-format`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/check_ref_format/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_check_ref_format`
#[derive(clap::Args, Debug)]
pub struct CheckRefFormatArgs {
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
}

/// Runs `rgit check-ref-format` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_check_ref_format(args: CheckRefFormatArgs) -> anyhow::Result<()> {
    let CheckRefFormatArgs {
        normalize,
        allow_onelevel,
        no_allow_onelevel: _,
        refspec_pattern,
        branch,
        refname,
    } = args;
    let opts = crate::references::check_ref_format::types::CheckRefFormatOptions {
        normalize,
        allow_onelevel,
        refspec_pattern,
        branch,
        refname,
    };
    crate::references::check_ref_format::check_ref_format(opts)?;
    Ok(())
}
