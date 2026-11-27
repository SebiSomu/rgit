//! Command-line interface of `rgit grep`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit grep`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/grep/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_grep`
#[derive(clap::Args, Debug)]
pub struct GrepArgs {
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
    /// `--color[=<when>]`: color the output (`always`, `never`, `auto`).
    #[arg(long = "color", num_args = 0..=1, require_equals = true, default_missing_value = "always")]
    color: Option<String>,
    #[arg(long = "no-color")]
    no_color: bool,
    #[arg(short = 'e')]
    pattern_flag: Option<String>,
    args: Vec<String>,
    #[arg(last = true)]
    path_specs: Vec<String>,
}

/// Runs `rgit grep` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_grep(args: GrepArgs) -> anyhow::Result<()> {
    let GrepArgs {
        ignore_case,
        invert_match,
        line_number,
        count,
        files_with_matches,
        files_without_match,
        fixed_strings,
        cached,
        color,
        no_color,
        pattern_flag,
        args,
        path_specs,
    } = args;
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
            if crate::core::commit::resolve_commit_from_source(&arg).is_ok() {
                revisions.push(arg);
            } else {
                final_path_specs.push(arg);
            }
        }
    } else {
        revisions.extend(remaining_args);
    }

    let options = crate::history::grep::types::GrepOptions {
        ignore_case,
        invert_match,
        line_number,
        count,
        files_with_matches,
        files_without_match,
        fixed_strings,
        cached,
        color: {
            if no_color {
                crate::core::color::set_override(Some(false));
            } else if let Some(when) = color {
                crate::core::color::set_override(crate::core::color::parse_color_flag(&format!("--color={}", when)).flatten());
            }
            crate::core::color::enabled("grep")
        },
    };

    crate::history::grep::grep(pattern, revisions, final_path_specs, options)?;
    Ok(())
}
