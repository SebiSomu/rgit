use anyhow::Result;

use crate::helpers::refs_verify::{verify_refs, REFS_USAGE};
use crate::core::repo;

/// `rgit refs list [<options>] [<pattern>...]` / `rgit refs exists <ref>` /
/// `rgit refs verify [--strict] [--verbose]` / `rgit refs optimize [<options>]`
///
/// The umbrella command newer gits (2.46 and later) have for low-level ref work:
/// - `list` is `for-each-ref` (same options, formats and patterns);
/// - `exists <ref>` is `show-ref --exists`: exit 0 when the ref exists, 2 when it does not,
///   1 on a lookup error;
/// - `verify` checks the ref store itself: loose ref names and contents (a full hash, or
///   `ref: <name>`) and the `packed-refs` file (well-formed, sorted lines). Problems are
///   printed as `error: <ref>: <kind>: <detail>`, harmless oddities (no final newline, text
///   after the hash) as `warning:`; `--strict` turns those into errors, `--verbose` lists
///   what is checked. Exit 0 when there are no errors, 1 otherwise;
/// - `optimize` is `pack-refs` (same options).
/// `refs migrate` (switching to the reftable format) is not supported: rgit only has the
/// files backend. Exit 129 for a usage error.
pub fn refs(args: Vec<String>) -> Result<()> {
    let usage = |message: Option<String>| -> ! {
        match message {
            Some(message) => eprint!("{}\n{}", message, REFS_USAGE),
            None => eprint!("{}", REFS_USAGE),
        }
        crate::ui::exit(129);
    };
    let Some(action) = args.first().map(String::as_str) else { usage(Some("error: need a subcommand".to_string())) };
    let rest = args[1..].to_vec();
    match action {
        "-h" => {
            print!("{}", REFS_USAGE);
            crate::ui::exit(129);
        }
        "list" => crate::commands::for_each_ref::for_each_ref(rest),
        "optimize" => crate::commands::pack_refs::pack_refs(rest),
        "exists" => {
            if rest.iter().any(|a| a == "-h") {
                println!("usage: rgit refs exists <ref>");
                crate::ui::exit(129);
            }
            if rest.len() != 1 {
                eprintln!("fatal: 'rgit refs exists' requires a reference");
                crate::ui::exit(128);
            }
            let mut forwarded = vec!["--exists".to_string()];
            forwarded.extend(rest);
            crate::commands::show_ref::show_ref(forwarded)
        }
        "verify" => {
            let mut strict = false;
            let mut verbose = false;
            for arg in &rest {
                match arg.as_str() {
                    "--strict" => strict = true,
                    "--no-strict" => strict = false,
                    "--verbose" => verbose = true,
                    "--no-verbose" => verbose = false,
                    "-h" => {
                        println!("usage: rgit refs verify [--strict] [--verbose]");
                        crate::ui::exit(129);
                    }
                    other if other.starts_with('-') => usage(Some(format!("error: unknown option `{}'", other.trim_start_matches('-')))),
                    _ => {
                        eprintln!("fatal: 'rgit refs verify' takes no arguments");
                        crate::ui::exit(128);
                    }
                }
            }
            if !repo::exists() {
                eprintln!("fatal: not a git repository (or any of the parent directories): .git");
                crate::ui::exit(128);
            }
            let (checked, found) = verify_refs(strict);
            if verbose {
                eprintln!("Checking references consistency");
                for name in &checked {
                    eprintln!("Checking {}", name);
                }
            }
            for (is_error, message) in &found {
                eprintln!("{}: {}", if *is_error { "error" } else { "warning" }, message);
            }
            if found.iter().any(|(is_error, _)| *is_error) {
                crate::ui::exit(1);
            }
            Ok(())
        }
        "migrate" => {
            eprintln!("fatal: 'refs migrate' is not supported: rgit only has the files ref backend");
            crate::ui::exit(128);
        }
        other => usage(Some(format!("error: unknown subcommand: `{}'", other))),
    }
}
