use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::helpers::{build_merge_commit, read_object, tree_hash_of_commit, write_object};
use crate::index::read_index;
use crate::refs;

pub fn build_commit(tree_hash: String, parent_hash: Option<String>, message: &str) -> anyhow::Result<String> {
    let mut content = format!("tree {}\n", tree_hash);
    if let Some(parent) = parent_hash {
        content.push_str(&format!("parent {}\n", parent));
    }

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let author = "rgit <rgit@example.com>";

    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));

    write_object("commit", content.as_bytes())
}

pub fn commit_tree(tree_hash: String, parent_hash: Option<String>, message: String) -> anyhow::Result<()> {
    println!("{}", build_commit(tree_hash, parent_hash, &message)?);
    Ok(())
}

pub fn commit(message: String) -> anyhow::Result<()> {
    let entries = read_index().unwrap_or_default();
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    let head_state = refs::resolve_head()?;
    let parent_hash = refs::resolve_head_commit()?;

    let second_parent = if Path::new(".git/MERGE_HEAD").exists() {
        let content = fs::read_to_string(".git/MERGE_HEAD")?;
        Some(content.trim().to_string())
    } else {
        None
    };

    if let (Some(parent), None) = (&parent_hash, &second_parent) {
        if tree_hash_of_commit(parent)? == tree_hash {
            anyhow::bail!("nothing to commit (working tree matches HEAD)");
        }
    }

    let commit_hash = if let (Some(p1), Some(p2)) = (&parent_hash, &second_parent) {
        build_merge_commit(tree_hash, p1, p2, &message)?
    } else {
        build_commit(tree_hash, parent_hash.clone(), &message)?
    };

    if Path::new(".git/MERGE_HEAD").exists() {
        let _ = fs::remove_file(".git/MERGE_HEAD");
    }
    if Path::new(".git/MERGE_MSG").exists() {
        let _ = fs::remove_file(".git/MERGE_MSG");
    }

    let short_hash = &commit_hash[..7];

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &commit_hash)?;
            if parent_hash.is_none() {
                println!("[{} (root-commit) {}] {}", branch_name, short_hash, message);
            } else {
                println!("[{} {}] {}", branch_name, short_hash, message);
            }
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&commit_hash)?;
            if parent_hash.is_none() {
                println!("[(detached HEAD) (root-commit) {}] {}", short_hash, message);
            } else {
                println!("[(detached HEAD) {}] {}", short_hash, message);
            }
            eprintln!("warning: You are in a detached HEAD state.");
        }
    }

    Ok(())
}

pub fn log(oneline: bool) -> anyhow::Result<()> {
    let head_state = refs::resolve_head()?;

    let (label, mut current_hash) = match &head_state {
        refs::HeadState::Branch(b) => {
            let ref_path = format!("refs/heads/{}", b);
            (b.clone(), refs::read_ref(&ref_path)?)
        }
        refs::HeadState::Detached(h) => ("HEAD".to_string(), Some(h.clone())),
    };

    if current_hash.is_none() {
        anyhow::bail!(
            "fatal: your current branch '{}' does not have any commits yet",
            label
        );
    }

    let mut first = true;

    while let Some(hash) = current_hash {
        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            anyhow::bail!("{} is not a commit object", hash);
        }

        let text = String::from_utf8_lossy(&content);
        let mut parent: Option<String> = None;
        let mut author_line: Option<String> = None;
        let mut message_lines: Vec<String> = Vec::new();
        let mut in_message = false;

        for line in text.lines() {
            if in_message {
                message_lines.push(line.to_string());
            } else if line.is_empty() {
                in_message = true;
            } else if let Some(p) = line.strip_prefix("parent ") {
                parent = Some(p.to_string());
            } else if let Some(a) = line.strip_prefix("author ") {
                author_line = Some(a.to_string());
            }
        }

        if oneline {
            let first_line = message_lines.first().map(|s| s.as_str()).unwrap_or("");
            println!("{} {}", &hash[..7], first_line);
        } else {
            if !first {
                println!();
            }

            println!("commit {}", hash);

            if let Some(author) = &author_line {
                println!("Author: {}", author);
            }

            println!();
            for line in &message_lines {
                println!("    {}", line);
            }
        }

        first = false;
        current_hash = parent;
    }

    Ok(())
}