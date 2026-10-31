use anyhow::Result;

use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::for_each_ref::shorten_unambiguous_ref_with;
use crate::helpers::symbolic_ref::{
    delete_symbolic_ref, parse_symbolic_ref_args, resolve_ref_unsafe, write_symbolic_ref, SYMBOLIC_REF_USAGE,
};
use crate::objects::SymbolicRefOptions;
use crate::repo;

/// `rgit symbolic-ref [-m <reason>] <name> <ref>`
/// `rgit symbolic-ref [-q] [--short] [--no-recurse] <name>`
/// `rgit symbolic-ref --delete [-q] <name>`
///
/// Reads, creates, updates or deletes a symbolic ref (a ref file holding `ref: <target>`,
/// like HEAD or `refs/remotes/origin/HEAD`). With one name it prints the ref the name points
/// at, following chains of symbolic refs unless `--no-recurse`, and shortened with `--short`;
/// a target that doesn't exist yet (an unborn branch) still prints. With a name and a target
/// it rewrites the name's own file (HEAD may only point inside `refs/`) and logs the move,
/// with `-m` as the reflog message. `--delete` removes a symbolic ref other than HEAD. Exit status, like git: 0 success,
/// 1 not a symbolic ref with `-q` (or a failed update/delete), 128 fatal, 129 usage.
pub fn symbolic_ref(args: Vec<String>) -> Result<()> {
    let before_dashdash = args.iter().take_while(|a| a.as_str() != "--");
    if before_dashdash.clone().any(|a| a == "-h" || a == "--help") {
        print!("{}", SYMBOLIC_REF_USAGE);
        std::process::exit(129);
    }

    let opts = match parse_symbolic_ref_args(&args) {
        Ok(opts) => opts,
        Err(e) => usage_exit(&e.to_string()),
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        std::process::exit(128);
    }

    match run_symbolic_ref(&opts) {
        Ok(0) => Ok(()),
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}

fn usage_exit(message: &str) -> ! {
    eprint!("{}", message);
    std::process::exit(129);
}

fn run_symbolic_ref(opts: &SymbolicRefOptions) -> Result<i32> {
    if opts.message.as_deref() == Some("") {
        anyhow::bail!("fatal: Refusing to perform update with empty message");
    }

    if opts.delete {
        let [name] = opts.args.as_slice() else { usage_exit(SYMBOLIC_REF_USAGE) };
        match resolve_ref_unsafe(name, false, false) {
            None => anyhow::bail!("fatal: No such ref: {}", name),
            Some(resolved) if !resolved.is_symref => anyhow::bail!("fatal: Cannot delete {}, not a symbolic ref", name),
            Some(_) => {}
        }
        if name == "HEAD" {
            anyhow::bail!("fatal: deleting '{}' is not allowed", name);
        }
        return Ok(report_failure(delete_symbolic_ref(name)));
    }

    match opts.args.as_slice() {
        [name] => {
            let Some(resolved) = resolve_ref_unsafe(name, opts.recurse, false) else {
                anyhow::bail!("fatal: No such ref: {}", name);
            };
            if !resolved.is_symref {
                if opts.quiet {
                    return Ok(1);
                }
                anyhow::bail!("fatal: ref {} is not a symbolic ref", name);
            }
            let shown = if opts.short { shorten_unambiguous_ref_with(&resolved.name, false) } else { resolved.name };
            println!("{}", shown);
            Ok(0)
        }
        [name, target] => {
            if name == "HEAD" && !target.starts_with("refs/") {
                anyhow::bail!("fatal: Refusing to point HEAD outside of refs/");
            }
            if !check_refname_format(target, true, false) {
                anyhow::bail!("fatal: Refusing to set '{}' to invalid ref '{}'", name, target);
            }
            Ok(report_failure(write_symbolic_ref(name, target, opts.message.as_deref())))
        }
        _ => usage_exit(SYMBOLIC_REF_USAGE),
    }
}

/// Prints a failed update's `error: ...` message and turns it into exit status 1, as git does
/// for ref-transaction failures.
fn report_failure(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{:#}", e);
            1
        }
    }
}
