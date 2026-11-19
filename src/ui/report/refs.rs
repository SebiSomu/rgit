//! Reports for log, tag and branch.

use crate::ui::{{detail, has, list, next, note, plural, positionals, short}, types::RepoSnapshot};

/// `log` on a branch with no commits: git only says "does not have any commits yet".
pub(super) fn report_log(args: &[String], a: &RepoSnapshot) {
    if a.head.is_some() || !positionals(args, &[]).is_empty() || has(args, &["--all"]) {
        return;
    }
    note("There is no history yet: nothing has been committed on this branch.");
    let others: Vec<String> = a.refs.keys().filter_map(|r| r.strip_prefix("refs/heads/")).map(String::from).collect();
    if !others.is_empty() {
        next(&format!("other branches have commits: {} (`rgit log {}` or `rgit log --all`).", list(&others, 4), others[0]));
    }
    if a.staged.is_empty() {
        next("`rgit add <files>`, then `rgit commit -m \"<message>\"` makes the first commit.");
    } else {
        next(&format!("{} staged; `rgit commit -m \"<message>\"` makes the first commit.", plural(a.staged.len(), "file is", "files are")));
    }
}

pub(super) fn report_tag(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    if has(args, &["-l", "--list", "-d", "--delete", "-v"]) {
        return;
    }
    let pos = positionals(args, &["-m", "-F", "--message", "--file"]);
    let Some(name) = pos.first() else { return };
    let refname = format!("refs/tags/{}", name);
    match (b.refs.get(&refname), a.refs.get(&refname)) {
        (Some(old), Some(new)) if old == new => {
            let target = crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", name)).unwrap_or_else(|_| old.clone());
            note(&format!("Tag '{}' already exists (points to {}); `rgit tag -f {}` moves it.", name, short(&target), name));
        }
        (old, Some(new)) => {
            let annotated = crate::core::object_store::read_object(new).map(|(k, _)| k == "tag").unwrap_or(false);
            let target = crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", name)).unwrap_or_else(|_| new.clone());
            let (kind, gloss) = if annotated { ("annotated tag", "it records author, date and message") } else { ("lightweight tag", "just a name for a commit") };
            let verb = if old.is_some() { "Moved" } else { "Created" };
            note(&format!("{} {} '{}' on {} ({}).", verb, kind, name, short(&target), gloss));
            detail(&format!("Tags aren't pushed by default: `rgit push origin {}` shares it.", name));
        }
        _ => {}
    }
}

pub(super) fn report_branch(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    for (name, hash) in &b.refs {
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            if !a.refs.contains_key(name) && !a.refs.keys().any(|n| n.starts_with("refs/heads/") && a.refs[n] == *hash && b.refs.get(n).is_none()) {
                note(&format!("Undo: `rgit branch {} {}` brings it back (its commits stay recoverable until the reflog expires).", branch, short(hash)));
            }
        }
    }
    let renaming = has(args, &["-m", "-M", "--move", "-c", "-C", "--copy"]);
    for (name, hash) in &a.refs {
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            if !b.refs.contains_key(name) && !renaming {
                note(&format!("Created '{}' at {}; you're still on '{}' (`rgit switch {}` to move there).", branch, short(hash), a.head_branch.clone().unwrap_or_else(|| "a detached HEAD".into()), branch));
            }
        }
    }
}
