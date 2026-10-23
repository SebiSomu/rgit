use anyhow::Result;
use std::process;

use crate::helpers::rev_parse::{
    abbrev_ref_symbolic, find_work_tree_root, is_inside_work_tree, resolve_rev,
    symbolic_full_name_symbolic,
};
use crate::objects::RevParseOptions;

/// `rgit rev-parse [--git-dir|--show-toplevel|--is-inside-work-tree|--is-bare-repository]`
/// `rgit rev-parse [--abbrev-ref|--symbolic-full-name] [<rev>...]`
/// `rgit rev-parse [--verify [-q]] <rev>`
/// `rgit rev-parse [--short[=N]] [--sq] <rev>...`
///
/// In flag-query mode one of the `--git-dir`, `--show-toplevel`, etc. flags is set and no
/// revision arguments are needed: the command prints the requested property and exits.
///
/// In revision-resolution mode each argument in `revs` is resolved to a 40-hex object
/// hash (or a symbolic name with `--abbrev-ref` / `--symbolic-full-name`) and printed on
/// its own line.  With `--short[=N]` the hash is truncated to N characters (default 7).
/// With `--sq` all outputs are combined into a single shell-safe quoted string.
/// With `--verify` a failure to resolve exits with code 1; `-q` suppresses the error message.
pub fn rev_parse(opts: RevParseOptions) -> Result<()> {
    let mut handled_flag = false;

    if opts.git_dir {
        match find_work_tree_root() {
            Ok(root) if root.join(".git").is_file() => {
                // A linked worktree: its git dir is `.git/worktrees/<id>` of the main repository.
                let git_dir = crate::helpers::worktree::with_current_dir(&root, || Ok(crate::repo::git_dir()))?;
                println!("{}", crate::repo::to_git_path_string(&git_dir));
            }
            Ok(root) => {
                if let Ok(cur) = std::env::current_dir() {
                    if cur == root {
                        println!(".git");
                    } else {
                        println!("{}", root.join(".git").display());
                    }
                } else {
                    println!("{}", root.join(".git").display());
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                process::exit(128);
            }
        }
        handled_flag = true;
    }

    if opts.show_toplevel {
        match find_work_tree_root() {
            Ok(root) => {
                let canonical = root
                    .canonicalize()
                    .unwrap_or(root);
                println!("{}", canonical.display());
            }
            Err(e) => {
                eprintln!("{}", e);
                process::exit(128);
            }
        }
        handled_flag = true;
    }

    if opts.is_inside_work_tree {
        println!("{}", is_inside_work_tree());
        handled_flag = true;
    }

    if opts.is_bare_repository {
        println!("false");
        handled_flag = true;
    }
    if handled_flag && opts.revs.is_empty() {
        return Ok(());
    }
    if opts.revs.is_empty() && !handled_flag {
        anyhow::bail!("usage: rgit rev-parse [<flags>] [<rev>...]");
    }

    if opts.verify && opts.revs.len() != 1 {
        if !opts.quiet {
            eprintln!("fatal: --verify requires exactly one revision");
        }
        process::exit(1);
    }

    let mut outputs: Vec<String> = Vec::new();

    for rev in &opts.revs {
        let resolved = if opts.abbrev_ref {
            match abbrev_ref_symbolic(rev) {
                Ok(s) => s,
                Err(e) => {
                    if opts.verify {
                        if !opts.quiet {
                            eprintln!("{}", e);
                        }
                        process::exit(1);
                    }
                    return Err(e);
                }
            }
        } else if opts.symbolic_full_name {
            match symbolic_full_name_symbolic(rev) {
                Ok(s) => s,
                Err(e) => {
                    if opts.verify {
                        if !opts.quiet {
                            eprintln!("{}", e);
                        }
                        process::exit(1);
                    }
                    return Err(e);
                }
            }
        } else {
            match resolve_rev(rev) {
                Ok(hash) => {
                    if let Some(n) = opts.short {
                        let n = n.max(1);
                        hash[..hash.len().min(n)].to_string()
                    } else {
                        hash
                    }
                }
                Err(e) => {
                    if opts.verify {
                        if !opts.quiet {
                            eprintln!("{}", e);
                        }
                        process::exit(1);
                    }
                    return Err(e);
                }
            }
        };

        outputs.push(resolved);
    }

    if opts.sq {
        let quoted: Vec<String> = outputs
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
            .collect();
        println!("{}", quoted.join(" "));
    } else {
        for out in outputs {
            println!("{}", out);
        }
    }

    Ok(())
}
