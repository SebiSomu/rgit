pub mod cli;
pub mod engine;
pub mod types;

use crate::remote::pr_summary::engine::{changed_files, default_base, title_from_branch};

/// `rgit pr-summary [<base> [<head>]] [--title <text>]`: a Markdown draft of a pull request
/// description for the commits `head` (HEAD) has that `base` doesn't (base: the remote's
/// default branch, else main / master): a title (the commit's subject for one commit, else
/// made from the branch name), the totals, the commits (oldest first, with the body of a
/// single commit as the description), the files with their lines, and whether it merges
/// cleanly into the base (an in-memory merge). Printed to stdout, ready to paste or pipe.
pub fn pr_summary(base: Option<String>, head: Option<String>, title: Option<String>) -> anyhow::Result<()> {
    let resolve = |rev: &str| match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)) {
        Ok(hash) => hash,
        Err(_) => crate::ui::die_fatal(format!("'{}' is not a branch or commit", rev)),
    };
    let head_name = match &head {
        Some(h) => h.clone(),
        None => match crate::core::refs::resolve_head()? {
            crate::core::refs::HeadState::Branch(b) => b,
            crate::core::refs::HeadState::Detached(_) => "HEAD".to_string(),
        },
    };
    let head_hash = resolve(head.as_deref().unwrap_or("HEAD"));
    let (base_name, base_hash) = match &base {
        Some(b) => (b.clone(), resolve(b)),
        None => {
            let remotes = crate::remote::remote::engine::list_remote_names().unwrap_or_default();
            let remote = remotes.iter().find(|r| *r == "origin").or(remotes.first()).cloned().unwrap_or_else(|| "origin".to_string());
            match default_base(&remote) {
                Some(found) => found,
                None => crate::ui::die_fatal("no base branch found (no main or master here or on the remote)\nhint: name it: `rgit pr-summary <base>`"),
            }
        }
    };
    if base_name == head_name || base_hash == head_hash {
        crate::ui::die_fatal(format!("nothing to summarize: {} and {} are the same", head_name, base_name));
    }
    let mut commits = crate::history::compare::engine::only_on(&head_hash, &base_hash)?;
    if commits.is_empty() {
        println!("Nothing to summarize: {} has no commits that {} doesn't.", head_name, base_name);
        return Ok(());
    }
    commits.reverse();
    let merge_base = crate::merge::merge::engine::find_merge_base(&head_hash, &base_hash)?.unwrap_or_else(|| base_hash.clone());
    let files = changed_files(&merge_base, &head_hash)?;
    let (added, removed) = files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));

    let single = commits.len() == 1;
    let title = title.unwrap_or_else(|| if single || head_name == "HEAD" { commits.last().map(|c| c.message.lines().next().unwrap_or("").to_string()).unwrap_or_default() } else { title_from_branch(&head_name) });
    println!("# {}", title);
    println!();
    if single {
        let body: Vec<&str> = commits[0].message.lines().skip(1).skip_while(|l| l.trim().is_empty()).collect();
        if !body.is_empty() {
            println!("{}", body.join("\n").trim_end());
            println!();
        }
    }
    println!(
        "{} commit{}, {} file{} changed (+{} -{}) against `{}`.",
        commits.len(),
        if single { "" } else { "s" },
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        added,
        removed,
        base_name
    );
    if !single {
        println!();
        println!("## Commits");
        println!();
        for c in &commits {
            let merge = if c.parents.len() > 1 { " (merge)" } else { "" };
            println!("- {} (`{}`){}", c.message.lines().next().unwrap_or(""), &c.hash[..7], merge);
        }
    }
    println!();
    println!("## Files");
    println!();
    for f in &files {
        let what = if f.binary {
            if f.new { "new, binary".to_string() } else if f.deleted { "deleted, binary".to_string() } else { "binary".to_string() }
        } else if f.new {
            format!("new, +{}", f.added)
        } else if f.deleted {
            format!("deleted, -{}", f.removed)
        } else {
            format!("+{} -{}", f.added, f.removed)
        };
        println!("- `{}` ({})", f.path, what);
    }
    println!();
    let conflicts = crate::history::compare::engine::merge_conflicts(&head_hash, &base_hash)?;
    if conflicts.is_empty() {
        println!("Merges cleanly into `{}`.", base_name);
    } else {
        println!("**Conflicts with `{}`** in {}: rebase or merge `{}` first.", base_name, conflicts.iter().map(|c| format!("`{}`", c)).collect::<Vec<_>>().join(", "), base_name);
    }
    Ok(())
}
