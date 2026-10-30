use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::helpers::rev_list::list_all_refs;
use crate::helpers::show_ref::{matches_show_ref_pattern, parse_show_ref_args, resolve_show_ref, show_ref_exclude_existing, show_ref_exists, show_ref_line};
use crate::objects::ShowRefOptions;

/// `rgit show-ref [--head] [-d] [-s|--hash[=<n>]] [--abbrev[=<n>]] [--heads] [--tags] [--] [<pattern>...]`
/// `rgit show-ref --verify [-q] [-d] [-s|--hash[=<n>]] [--abbrev[=<n>]] [--] <ref>...`
/// `rgit show-ref --exclude-existing[=<pattern>]`, `rgit show-ref --exists <ref>`
///
/// Lists refs (loose and packed, in name order) with the objects they point at. Patterns match
/// whole trailing path components (`main` finds `refs/heads/main` and `refs/remotes/origin/main`);
/// `--heads`/`--tags` restrict to branches and tags, `--head` adds HEAD, and `-d` adds the peeled
/// `^{}` line for annotated tags. `--verify` takes exact names (`refs/...` or `HEAD`) and fails on
/// the first missing one; `--exists` checks one ref without resolving it; `--exclude-existing`
/// filters refs read from stdin down to those missing locally. Exit status, like git: 0 found,
/// 1 nothing matched (or `--verify -q` failed, or `--exists` hit a directory), 2 `--exists`
/// found nothing, 128 fatal, 129 usage.
pub fn show_ref(args: Vec<String>) -> Result<()> {
    let opts = match parse_show_ref_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(129);
        }
    };
    match run_show_ref(&opts) {
        Ok(0) => Ok(()),
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}

fn run_show_ref(opts: &ShowRefOptions) -> Result<i32> {
    let modes = [opts.exclude_existing.is_some(), opts.verify, opts.exists];
    if modes.iter().filter(|on| **on).count() > 1 {
        anyhow::bail!("fatal: only one of '--exclude-existing', '--verify' or '--exists' can be given");
    }

    if let Some(pattern) = &opts.exclude_existing {
        show_ref_exclude_existing(pattern)?;
        return Ok(0);
    }

    if opts.exists {
        match opts.patterns.as_slice() {
            [] => anyhow::bail!("fatal: --exists requires a reference"),
            [name] => match show_ref_exists(name) {
                Ok(true) => return Ok(0),
                Ok(false) => {
                    eprintln!("error: reference does not exist");
                    return Ok(2);
                }
                Err(message) => {
                    eprintln!("{}", message);
                    return Ok(1);
                }
            },
            _ => anyhow::bail!("fatal: --exists requires exactly one reference"),
        }
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    if opts.verify {
        if opts.patterns.is_empty() {
            anyhow::bail!("fatal: --verify requires a reference");
        }
        for name in &opts.patterns {
            let resolved = if name.starts_with("refs/") || name == "HEAD" { resolve_show_ref(name) } else { None };
            match resolved {
                Some(hash) => show_ref_line(&mut out, name, &hash, opts)?,
                None if opts.quiet => return Ok(1),
                None => {
                    out.flush()?;
                    anyhow::bail!("fatal: '{}' - not a valid ref", name);
                }
            }
        }
        out.flush()?;
        return Ok(0);
    }

    let mut found = 0;
    if opts.head {
        if let Some(hash) = resolve_show_ref("HEAD") {
            show_ref_line(&mut out, "HEAD", &hash, opts)?;
            found += 1;
        }
    }
    for (refname, hash) in list_all_refs() {
        let in_scope = !(opts.heads || opts.tags)
            || (opts.heads && refname.starts_with("refs/heads/"))
            || (opts.tags && refname.starts_with("refs/tags/"));
        if in_scope && matches_show_ref_pattern(&refname, &opts.patterns) {
            show_ref_line(&mut out, &refname, &hash, opts)?;
            found += 1;
        }
    }
    out.flush()?;
    Ok(if found == 0 { 1 } else { 0 })
}
