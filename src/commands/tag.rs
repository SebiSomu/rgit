use std::collections::{HashMap, HashSet, VecDeque};
use crate::helpers::{read_object, resolve_commit_from_source};
use crate::refs;

/// `rgit tag` / `rgit tag <name> [<commit>]` / `rgit tag -d <name>` / `rgit tag -l [<pattern>]`
// ///
// /// Lightweight tags only
pub fn tag(name: Option<String>, commit: Option<String>, delete: bool, list: bool) -> anyhow::Result<()> {
    if delete {
        let tag_name = name.ok_or_else(|| {
            anyhow::anyhow!("fatal: tag name required for delete")
        })?;
        refs::delete_tag(&tag_name)?;
        println!("Deleted tag '{}'", tag_name);
        return Ok(());
    }

    if list || name.is_none() {
        let tags = refs::list_tags()?;
        let pattern = if list { name.as_deref() } else { None };

        for t in &tags {
            if let Some(p) = pattern {
                if !t.contains(p) {
                    continue;
                }
            }
            println!("{}", t);
        }
        return Ok(());
    }

    let tag_name = name.unwrap();

    let target_commit = match commit {
        Some(c) => resolve_commit_from_source(&c)?,
        None => refs::resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("fatal: cannot create tag — no commits yet")
        })?,
    };

    refs::create_tag(&tag_name, &target_commit)?;
    println!("Tag '{}' created at {}", tag_name, &target_commit[..7]);

    Ok(())
}

pub fn describe(commit: Option<String>) -> anyhow::Result<()> {
    let source = commit.as_deref().unwrap_or("HEAD");
    let target = resolve_commit_from_source(source)?;

    let tags = refs::list_tags()?;
    if tags.is_empty() {
        anyhow::bail!(
            "fatal: No tags can describe '{}'.\nTry --always, or create some tags.",
            source
        );
    }

    let mut tag_commits: HashMap<String, String> = HashMap::new();
    for tag_name in &tags {
        let ref_path = format!("refs/tags/{}", tag_name);
        if let Some(hash) = refs::read_ref(&ref_path)? {
            tag_commits.insert(hash, tag_name.clone());
        }
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((target.clone(), 0usize));

    while let Some((hash, distance)) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        if let Some(tag_name) = tag_commits.get(&hash) {
            if distance == 0 {
                println!("{}", tag_name);
            } else {
                println!("{}-{}-g{}", tag_name, distance, &target[..target.len().min(7)]);
            }
            return Ok(());
        }

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent) = line.strip_prefix("parent ") {
                queue.push_back((parent.to_string(), distance + 1));
            }
        }
    }

    anyhow::bail!(
        "fatal: No tags can describe '{}'.\nTry --always, or create some tags.",
        source
    );
}