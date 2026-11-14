use anyhow::Result;

use crate::helpers::hook::{find_hook, run_hook, HOOK_USAGE};

/// `rgit hook run [--ignore-missing] [--to-stdin=<path>] <hook-name> [-- <hook-args>]`
///
/// Runs the hook script `<hook-name>` from `.git/hooks` (or `core.hooksPath`) with the
/// arguments after `--`, exactly as a git command would when it reaches that hook, and
/// exits with the hook's status. A hook that doesn't exist is an error (exit 1) unless
/// `--ignore-missing`, in which case nothing happens and the status is 0. `--to-stdin`
/// feeds a file to the hook's standard input. rgit's own commands do not run hooks yet;
/// this is the building block for that and for scripts. Exit 129 on usage errors.
pub fn hook(args: Vec<String>) -> Result<()> {
    let usage = |message: &str| -> ! {
        if !message.is_empty() {
            eprintln!("{}", message);
        }
        eprint!("{}", HOOK_USAGE);
        std::process::exit(129);
    };
    if args.first().map(String::as_str) != Some("run") || args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        usage("");
    }
    if !crate::repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        std::process::exit(128);
    }

    let (mut ignore_missing, mut to_stdin) = (false, None);
    let mut name: Option<&String> = None;
    let mut hook_args: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        match arg {
            "--" => {
                hook_args = args[i..].to_vec();
                break;
            }
            "--ignore-missing" => ignore_missing = true,
            "--to-stdin" => {
                to_stdin = Some(args.get(i).cloned().unwrap_or_else(|| usage("error: option `to-stdin' requires a value")));
                i += 1;
            }
            _ if arg.starts_with("--to-stdin=") => to_stdin = Some(arg["--to-stdin=".len()..].to_string()),
            _ if arg.starts_with('-') => usage(&format!("error: unknown option `{}'", arg.trim_start_matches('-'))),
            _ if name.is_none() => name = Some(&args[i - 1]),
            _ => usage(""),
        }
    }
    let Some(name) = name else { usage("") };

    let Some(script) = find_hook(name) else {
        if ignore_missing {
            return Ok(());
        }
        eprintln!("error: cannot find a hook named {}", name);
        std::process::exit(1);
    };
    match run_hook(&script, &hook_args, to_stdin.as_deref()) {
        Ok(0) => Ok(()),
        Ok(status) => std::process::exit(status),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}
