//! Command-line interface of `rgit format-patch`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit format-patch`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/format_patch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_format_patch`
#[derive(clap::Args, Debug)]
pub struct FormatPatchArgs {
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
}

/// Runs `rgit format-patch` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_format_patch(args: FormatPatchArgs) -> anyhow::Result<()> {
    let FormatPatchArgs {
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
    } = args;
    let opts = crate::mail::format_patch::types::FormatPatchOptions {
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
    crate::mail::format_patch::format_patch(opts)?;
    Ok(())
}
