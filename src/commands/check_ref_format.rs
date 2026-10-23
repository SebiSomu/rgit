use anyhow::Result;

use crate::helpers::check_ref_format::{check_refname_format, collapse_ref_slashes, is_valid_branch_shorthand};
use crate::objects::CheckRefFormatOptions;

/// `rgit check-ref-format [--normalize] [--[no-]allow-onelevel] [--refspec-pattern] <refname>`
/// `rgit check-ref-format --branch <branchname>`
///
/// Validates a ref name against git's naming rules. Prints nothing and exits 0 if the name is
/// valid, 1 if not; with `--normalize` (alias `--print`) leading slashes are stripped and
/// repeated ones collapsed first, and the normalized name is printed when valid. One-level
/// names like `main` are rejected unless `--allow-onelevel` is given. `--branch` checks a
/// branch name (as `refs/heads/<name>`, also rejecting `HEAD` and a leading `-`), prints it
/// when valid, and fails with exit code 128 otherwise. Works outside a repository.
pub fn check_ref_format(opts: CheckRefFormatOptions) -> Result<()> {
    if let Some(branch) = &opts.branch {
        if !is_valid_branch_shorthand(branch) {
            eprintln!("fatal: '{}' is not a valid branch name", branch);
            std::process::exit(128);
        }
        println!("{}", branch);
        return Ok(());
    }

    let refname = opts.refname.as_deref().unwrap_or_default();
    let refname = if opts.normalize { collapse_ref_slashes(refname) } else { refname.to_string() };

    if !check_refname_format(&refname, opts.allow_onelevel, opts.refspec_pattern) {
        std::process::exit(1);
    }
    if opts.normalize {
        println!("{}", refname);
    }

    Ok(())
}
