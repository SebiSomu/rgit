use anyhow::Result;

use crate::core::object_store::read_object;
use crate::helpers::replace::{create_replace_ref, delete_replace_refs, edit_object_copy, format_replace_list, parse_replace_args, write_grafted_commit, REPLACE_USAGE};
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::objects::ReplaceMode;
use crate::core::repo;

/// `rgit replace [-f] <object> <replacement>` / `--graft <commit> [<parent>...]` /
/// `--edit <object>` / `-d <object>...` / `[--format=<format>] [-l [<pattern>]]`
///
/// Manages `refs/replace/<object>` refs. While such a ref exists, every command that reads
/// `<object>` gets the replacement's content instead, without any hash changing, so history
/// can be corrected or joined locally: `log`, `show`, `cat-file`, `blame`, `merge-base` and
/// the rest all follow the replacement. Commands that copy, check or delete objects
/// (`fsck`, `gc`, `prune`, `count-objects`, `bundle`, `push`, `fetch`, `clone`, `pull`) keep
/// seeing the real objects, and setting `GIT_NO_REPLACE_OBJECTS` turns replacement off
/// everywhere. The plain form replaces one object with another of the same type (`-f`
/// allows a different type and overwriting an existing ref). `--graft` creates a copy of a
/// commit with the given parents (none makes it a root) and replaces the commit with it.
/// `--edit` opens the object in the editor (trees as `ls-tree` lines unless `--raw`) and
/// replaces it with the edited version. `-d` deletes replace refs. `-l` lists them,
/// optionally filtered by a glob, as `short` (object), `medium` (`object -> replacement`)
/// or `long` (with types). Unlike git, an object can't be made to replace itself. Exit
/// status, like git: 0 success, 1 a `-d` name without a replace ref, 255 other failures,
/// 129 usage.
pub fn replace(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", REPLACE_USAGE);
        crate::ui::exit(129);
    }
    let opts = match parse_replace_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            eprint!("{}", e);
            crate::ui::exit(129);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }

    let resolve = |name: &str| resolve_rev(name).map_err(|_| anyhow::anyhow!("error: failed to resolve '{}' as a valid ref", name));
    let result: Result<()> = (|| match opts.mode {
        ReplaceMode::List => {
            print!("{}", format_replace_list(opts.args.first().map(String::as_str), opts.format.as_deref())?);
            Ok(())
        }
        ReplaceMode::Delete => {
            let (messages, had_error) = delete_replace_refs(&opts.args);
            for message in messages.iter().filter(|m| m.starts_with("error:")) {
                eprintln!("{}", message);
            }
            for message in messages.iter().filter(|m| !m.starts_with("error:")) {
                println!("{}", message);
            }
            if had_error {
                crate::ui::exit(1);
            }
            Ok(())
        }
        ReplaceMode::Replace => {
            let (object, replacement) = (resolve(&opts.args[0])?, resolve(&opts.args[1])?);
            create_replace_ref(&opts.args[0], &object, &opts.args[1], &replacement, opts.force)
        }
        ReplaceMode::Graft => {
            let name = &opts.args[0];
            let commit = resolve_rev(name).map_err(|_| anyhow::anyhow!("error: not a valid object name: '{}'", name))?;
            let commit = peel_to_type(commit.clone(), "commit").map_err(|_| {
                let kind = read_object(&commit).map(|(kind, _)| kind).unwrap_or_default();
                anyhow::anyhow!("error: object {} is a {}, not a commit\nerror: could not parse {}", commit, kind, name)
            })?;
            let new_commit = write_grafted_commit(&commit, &opts.args[1..])?;
            if new_commit == commit {
                anyhow::bail!("error: new commit is the same as the old one: '{}'", commit);
            }
            create_replace_ref(name, &commit, "replacement", &new_commit, opts.force)
        }
        ReplaceMode::Edit => {
            let name = &opts.args[0];
            let object = resolve_rev(name).map_err(|_| anyhow::anyhow!("error: not a valid object name: '{}'", name))?;
            if read_object(&object).is_err() {
                anyhow::bail!("error: unable to get object type for {}", object);
            }
            let new_object = edit_object_copy(&object, opts.raw)?;
            if new_object == object {
                anyhow::bail!("error: new object is the same as the old one: '{}'", object);
            }
            create_replace_ref(name, &object, "replacement", &new_object, opts.force)
        }
    })();
    if let Err(e) = result {
        eprintln!("{}", e);
        crate::ui::exit(255);
    }
    Ok(())
}
