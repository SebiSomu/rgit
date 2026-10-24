use std::fs;
use anyhow::Result;

use crate::helpers::cherry::abbreviate_hash_min;
use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::commit::{cleanup_commit_message, is_reachable};
use crate::helpers::format_patch::resolve_commit_rev;
use crate::helpers::gitignore::glob_match;
use crate::helpers::rev_list::abbreviate_hash;
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::helpers::tag::{delete_tag_ref, describe_commit, list_tag_refs, tag_message_lines, write_tag_object};
use crate::objects::{DescribeOptions, TagOptions};
use crate::refs;

/// `rgit tag [-l] [-n[<num>]] [--contains <commit>] [--points-at <object>] [<pattern>...]`
/// `rgit tag [-a] [-f] [-m <msg>... | -F <file>] [-e] <tagname> [<object>]`
/// `rgit tag -d <tagname>...`
///
/// Lists, creates, or deletes tags, as git does. Listing (the default with no arguments, or
/// with `-l`) prints tag names in order, filtered by glob patterns, `--contains` (tags whose
/// commit has the given commit in its history) and `--points-at`, with `-n` adding the first
/// lines of each tag's message. Creating makes a lightweight tag, or an annotated tag object
/// (tagger, date, message) with `-a`, `-m`, or `-F`; the editor opens for `-a` without a
/// message. `-f` replaces an existing tag. `-d` deletes loose and packed tags. Tags work on
/// any object (commits, trees, blobs, other tags).
pub fn tag(opts: TagOptions) -> Result<()> {
    if opts.delete {
        let mut failed = false;
        let tags = list_tag_refs();
        for name in &opts.args {
            match tags.get(name) {
                Some(old) => {
                    delete_tag_ref(name)?;
                    println!("Deleted tag '{}' (was {})", name, abbreviate_hash(old));
                }
                None => {
                    eprintln!("error: tag '{}' not found.", name);
                    failed = true;
                }
            }
        }
        if failed {
            std::process::exit(1);
        }
        return Ok(());
    }

    let creating = !opts.list && opts.contains.is_none() && opts.points_at.is_none() && opts.lines.is_none() && !opts.args.is_empty();
    if !creating {
        return list_tags(&opts);
    }

    let name = &opts.args[0];
    if name.starts_with('-') || !check_refname_format(&format!("refs/tags/{}", name), false, false) {
        fail(&format!("fatal: '{}' is not a valid tag name.", name));
    }
    let object_name = opts.args.get(1).map(String::as_str).unwrap_or("HEAD");
    let object = resolve_rev(object_name).unwrap_or_else(|_| fail(&format!("fatal: Failed to resolve '{}' as a valid ref.", object_name)));
    let existing = list_tag_refs().get(name).cloned();
    if existing.is_some() && !opts.force {
        fail(&format!("fatal: tag '{}' already exists", name));
    }

    let annotated = opts.annotate || !opts.messages.is_empty() || opts.file.is_some();
    let value = if annotated {
        let message = tag_message(&opts, name)?;
        if crate::helpers::read_object(&object).is_ok_and(|(kind, _)| kind == "tag") {
            eprintln!(
                "hint: You have created a nested tag. The object referred to by your new tag is\nhint: already a tag. If you meant to tag the object that it points to, use:\nhint:\nhint: \trgit tag -f {} {}^{{}}\nhint: Disable this message with \"rgit config advice.nestedTag false\"",
                name, object_name
            );
        }
        write_tag_object(name, &object, &message)?
    } else {
        object
    };
    refs::write_ref(&format!("refs/tags/{}", name), &value)?;
    if let Some(old) = existing.filter(|old| *old != value) {
        println!("Updated tag '{}' (was {})", name, abbreviate_hash(&old));
    }
    Ok(())
}

fn fail(message: &str) -> ! {
    eprintln!("{}", message);
    std::process::exit(128);
}

/// The message of an annotated tag: `-m` paragraphs, `-F` file contents, or what was typed
/// into the editor (also opened by `-e`), cleaned up like a commit message.
fn tag_message(opts: &TagOptions, name: &str) -> Result<String> {
    let given = if !opts.messages.is_empty() {
        Some(opts.messages.join("\n\n"))
    } else if let Some(file) = &opts.file {
        Some(fs::read_to_string(file).map_err(|_| anyhow::anyhow!("fatal: could not open or read '{}'", file))?)
    } else {
        None
    };
    let message = if given.is_none() || opts.edit {
        let path = crate::repo::path(".git/TAG_EDITMSG");
        let initial = given.clone().unwrap_or_default();
        fs::write(&path, format!("{}\n#\n# Write a message for tag:\n#   {}\n# Lines starting with '#' will be ignored.\n", initial.trim_end(), name))?;
        crate::helpers::rebase::run_editor(&path, false)?;
        cleanup_commit_message(&fs::read_to_string(&path)?, true)
    } else {
        // Unlike commit, git strips `#` lines from every tag message, however it was given.
        cleanup_commit_message(given.as_deref().unwrap_or_default(), true)
    };
    if message.is_empty() && given.is_none() {
        fail("fatal: no tag message?");
    }
    Ok(message)
}

fn list_tags(opts: &TagOptions) -> Result<()> {
    let mut patterns = opts.args.clone();
    let lines = opts.lines;
    let contains = match &opts.contains {
        Some(rev) => Some(resolve_commit_rev(rev).unwrap_or_else(|_| fail(&format!("error: malformed object name {}", rev)))),
        None => None,
    };
    let points_at = match &opts.points_at {
        Some(rev) => Some(resolve_rev(rev).unwrap_or_else(|_| fail(&format!("error: malformed object name {}", rev)))),
        None => None,
    };
    patterns.retain(|p| !p.is_empty());

    for (name, object) in list_tag_refs() {
        if !patterns.is_empty() && !patterns.iter().any(|p| glob_match(p, &name)) {
            continue;
        }
        if let Some(target) = &points_at {
            if object != *target && peel_to_type(object.clone(), "commit").ok().as_ref() != Some(target) {
                continue;
            }
        }
        if let Some(commit) = &contains {
            match peel_to_type(object.clone(), "commit") {
                Ok(tagged) if tagged == *commit || is_reachable(&tagged, commit)? => {}
                _ => continue,
            }
        }
        match lines {
            None => println!("{}", name),
            Some(count) => {
                let message = tag_message_lines(&object);
                let mut message = message.into_iter().take(count.max(1));
                println!("{:<15} {}", name, message.next().unwrap_or_default());
                for line in message {
                    println!("    {}", line);
                }
            }
        }
    }
    Ok(())
}

/// `rgit describe [--tags] [--always] [--long] [--abbrev=<n>] [--exact-match] [<commit>...]`
///
/// Names each commit (default `HEAD`) after the nearest tag: the tag itself when it points at
/// the commit, otherwise `<tag>-<n>-g<hash>` where `<n>` counts the commits since the tag.
/// Only annotated tags count unless `--tags` is given. `--long` always uses the long form,
/// `--abbrev=<n>` sets the hash length (0 prints only the tag), `--always` falls back to the
/// hash, and `--exact-match` refuses anything but an exact tag. Errors use git's wording.
pub fn describe(opts: DescribeOptions) -> Result<()> {
    let commits = if opts.commits.is_empty() { vec!["HEAD".to_string()] } else { opts.commits.clone() };
    for name in &commits {
        let target = resolve_commit_rev(name).unwrap_or_else(|_| fail(&format!("fatal: Not a valid object name {}", name)));
        let abbrev = |hash: &str| match opts.abbrev {
            Some(n) => abbreviate_hash_min(hash, n),
            None => abbreviate_hash(hash),
        };
        let found = describe_commit(&target, opts.tags)?;
        match found {
            Some((tag, 0)) if !opts.long => println!("{}", tag),
            Some((tag, 0)) if opts.abbrev != Some(0) => {
                // git's quirk: an exact `--long` match shows the object the tag points at
                // directly (for a tag of a tag, that inner tag), not the commit.
                let object = list_tag_refs().get(&tag).cloned().unwrap_or_else(|| target.clone());
                let shown = match crate::helpers::read_object(&object) {
                    Ok((kind, content)) if kind == "tag" => String::from_utf8_lossy(&content)
                        .lines()
                        .find_map(|l| l.strip_prefix("object ").map(str::to_string))
                        .unwrap_or_else(|| target.clone()),
                    _ => target.clone(),
                };
                println!("{}-0-g{}", tag, abbrev(&shown));
            }
            Some((tag, depth)) if opts.exact_match && depth > 0 => {
                let _ = tag;
                fail(&format!("fatal: no tag exactly matches '{}'", target));
            }
            Some((tag, _)) if opts.abbrev == Some(0) => println!("{}", tag),
            Some((tag, depth)) => println!("{}-{}-g{}", tag, depth, abbrev(&target)),
            None if opts.exact_match => fail(&format!("fatal: no tag exactly matches '{}'", target)),
            None if opts.always => println!("{}", abbrev(&target)),
            None => {
                if list_tag_refs().is_empty() {
                    fail("fatal: No names found, cannot describe anything.");
                }
                if !opts.tags && describe_commit(&target, true)?.is_some() {
                    fail(&format!("fatal: No annotated tags can describe '{}'.\nHowever, there were unannotated tags: try --tags.", target));
                }
                fail(&format!("fatal: No tags can describe '{}'.\nTry --always, or create some tags.", target));
            }
        }
    }
    Ok(())
}
