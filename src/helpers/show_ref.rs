use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::read_object;
use crate::helpers::cherry::abbreviate_hash_min;
use crate::helpers::rev_list::list_all_refs;
use crate::helpers::rev_parse::peel_to_type;
use crate::objects::ShowRefOptions;

/// git's usage text for `show-ref`, printed after option errors (exit 129).
pub const SHOW_REF_USAGE: &str = "usage: rgit show-ref [--head] [-d | --dereference]
                     [-s | --hash[=<n>]] [--abbrev[=<n>]] [--tags]
                     [--heads] [--] [<pattern>...]
   or: rgit show-ref --verify [-q | --quiet] [-d | --dereference]
                     [-s | --hash[=<n>]] [--abbrev[=<n>]]
                     [--] [<ref>...]
   or: rgit show-ref --exclude-existing[=<pattern>]
   or: rgit show-ref --exists <ref>";

/// Parses `show-ref` arguments like git's option parser: options may follow patterns,
/// `-d`/`-q` can be bundled (`-dq`), `-s` takes the rest of its word as an optional length
/// (so `-sd` is an error, as in git), `--hash[=<n>]` and `--abbrev[=<n>]` take an optional
/// length (clamped to 4..=40, `0` meaning full), and everything after `--` is a pattern.
/// Errors containing the usage text mean exit 129.
// Used by `show-ref`.
pub fn parse_show_ref_args(args: &[String]) -> anyhow::Result<ShowRefOptions> {
    let mut opts = ShowRefOptions::default();
    let usage_error = |message: String| anyhow::anyhow!("{}\n{}", message, SHOW_REF_USAGE);
    let length = |name: &str, value: &str| -> anyhow::Result<Option<usize>> {
        let n: usize = value.parse().map_err(|_| usage_error(format!("error: option `{}' expects a numerical value", name)))?;
        Ok(if n == 0 { None } else { Some(n.clamp(4, 40)) })
    };

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            opts.patterns.extend(args[i + 1..].iter().cloned());
            break;
        }
        match arg {
            "--head" => opts.head = true,
            "--dereference" => opts.dereference = true,
            "--hash" => opts.hash_only = true,
            "--abbrev" => opts.abbrev = Some(7),
            "--no-abbrev" => opts.abbrev = None,
            "--tags" => opts.tags = true,
            "--heads" => opts.heads = true,
            "--no-tags" => opts.tags = false,
            "--no-heads" => opts.heads = false,
            "--verify" => opts.verify = true,
            "--exists" => opts.exists = true,
            "--quiet" => opts.quiet = true,
            "--exclude-existing" => opts.exclude_existing = Some(String::new()),
            _ if arg.starts_with("--hash=") => {
                opts.hash_only = true;
                opts.abbrev = length("hash", &arg["--hash=".len()..])?;
            }
            _ if arg.starts_with("--abbrev=") => opts.abbrev = length("abbrev", &arg["--abbrev=".len()..])?,
            _ if arg.starts_with("--exclude-existing=") => opts.exclude_existing = Some(arg["--exclude-existing=".len()..].to_string()),
            _ if arg.starts_with("--") => {
                let name = arg.trim_start_matches('-').split('=').next().unwrap_or("");
                return Err(usage_error(format!("error: unknown option `{}'", name)));
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                let switches = &arg[1..];
                for (pos, c) in switches.char_indices() {
                    match c {
                        'd' => opts.dereference = true,
                        'q' => opts.quiet = true,
                        's' => {
                            opts.hash_only = true;
                            let value = &switches[pos + 1..];
                            if !value.is_empty() {
                                opts.abbrev = length("hash", value)?;
                            }
                            break;
                        }
                        other => return Err(usage_error(format!("error: unknown switch `{}'", other))),
                    }
                }
            }
            _ => opts.patterns.push(arg.to_string()),
        }
        i += 1;
    }
    Ok(opts)
}

/// Checks a ref name against `show-ref` patterns: a pattern matches when it equals the whole
/// name or a trailing run of complete path components (`main` matches `refs/heads/main` and
/// `refs/remotes/origin/main`, but not `refs/heads/domain`). No patterns match everything.
// Used by `show-ref`.
pub fn matches_show_ref_pattern(refname: &str, patterns: &[String]) -> bool {
    patterns.is_empty()
        || patterns.iter().any(|p| {
            refname == p || (refname.len() > p.len() && refname.ends_with(p.as_str()) && refname.as_bytes()[refname.len() - p.len() - 1] == b'/')
        })
}

/// Resolves a ref (`HEAD`, `refs/...`) to the object it names: a loose ref file (following
/// `ref: ` symbolic refs up to 5 levels), or else a `packed-refs` entry.
// Used by `show-ref --verify` and `--head`.
pub fn resolve_show_ref(name: &str) -> Option<String> {
    let mut current = name.to_string();
    for _ in 0..5 {
        let path = crate::repo::path(format!(".git/{}", current));
        if Path::new(&path).is_file() {
            let content = fs::read_to_string(&path).ok()?;
            let value = content.trim();
            match value.strip_prefix("ref: ") {
                Some(target) => current = target.trim().to_string(),
                None if value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit()) => return Some(value.to_string()),
                None => return None,
            }
        } else {
            return packed_ref(&current);
        }
    }
    None
}

/// Checks whether a ref exists without resolving it, like git's raw ref lookup: `Ok(true)` for a
/// loose ref file (even a symbolic ref to a missing target) or a `packed-refs` entry,
/// `Ok(false)` when there is none, and an error message when the name is a directory such as
/// `refs/heads` (git reports "Is a directory" and exits 1 rather than 2).
// Used by `show-ref --exists`.
pub fn show_ref_exists(name: &str) -> Result<bool, String> {
    let path = crate::repo::path(format!(".git/{}", name));
    if Path::new(&path).is_file() || packed_ref(name).is_some() {
        Ok(true)
    } else if Path::new(&path).is_dir() {
        Err("error: failed to look up reference: Is a directory".to_string())
    } else {
        Ok(false)
    }
}

fn packed_ref(name: &str) -> Option<String> {
    let packed = fs::read_to_string(crate::repo::path(".git/packed-refs")).ok()?;
    packed.lines().find_map(|line| match line.split_once(' ') {
        Some((hash, refname)) if refname == name && hash.len() == 40 && !line.starts_with('#') => Some(hash.to_string()),
        _ => None,
    })
}

/// Prints one `show-ref` result: `<hash> <ref>` (or just the hash with `-s`), abbreviated with
/// `--hash=<n>`/`--abbrev`, followed with `-d` by `<peeled> <ref>^{}` when the ref names an
/// annotated tag (git prints that line with the ref name even under `-s`). Nothing is printed
/// with `-q`. Fails like git when the object is missing.
// Used by `show-ref`.
pub fn show_ref_line(out: &mut impl Write, refname: &str, hash: &str, opts: &ShowRefOptions) -> anyhow::Result<()> {
    let object_type = match read_object(hash) {
        Ok((object_type, _)) => object_type,
        Err(_) => anyhow::bail!("fatal: git show-ref: bad ref {} ({})", refname, hash),
    };
    if opts.quiet {
        return Ok(());
    }
    let show = |h: &str| opts.abbrev.map(|n| abbreviate_hash_min(h, n)).unwrap_or_else(|| h.to_string());

    if opts.hash_only {
        writeln!(out, "{}", show(hash))?;
    } else {
        writeln!(out, "{} {}", show(hash), refname)?;
    }
    if opts.dereference && object_type == "tag" {
        let peeled = peel_to_type(hash.to_string(), "")?;
        writeln!(out, "{} {}^{{}}", show(&peeled), refname)?;
    }
    Ok(())
}

/// Runs `show-ref --exclude-existing[=<pattern>]` over standard input: each line's last
/// whitespace-separated word (with a trailing `^{}` dropped) is a ref name; lines whose name
/// doesn't start with `pattern` are skipped, names that aren't valid ref names are warned about,
/// and lines naming refs that don't exist locally are printed.
// Used by `show-ref --exclude-existing`.
pub fn show_ref_exclude_existing(pattern: &str) -> anyhow::Result<()> {
    let existing: HashSet<String> = list_all_refs().into_keys().collect();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let line = line.strip_suffix("^{}").unwrap_or(&line);
        let name = line.rsplit(|c: char| c.is_whitespace()).next().unwrap_or("");
        if !name.starts_with(pattern) {
            continue;
        }
        if !check_refname_format(name, false, false) {
            eprintln!("warning: ref '{}' ignored", name);
            continue;
        }
        if !existing.contains(name) {
            writeln!(out, "{}", line)?;
        }
    }
    Ok(())
}
