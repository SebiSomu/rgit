use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub enum HeadState {
    Branch(String),
    Detached(String),
}

pub fn resolve_head() -> Result<HeadState> {
    let head_content = fs::read_to_string(".git/HEAD")
        .context("Failed to read .git/HEAD — is this an rgit repository?")?;
    let trimmed = head_content.trim();

    if let Some(ref_path) = trimmed.strip_prefix("ref: ") {
        let branch_name = ref_path
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_path)
            .to_string();
        Ok(HeadState::Branch(branch_name))
    } else {
        Ok(HeadState::Detached(trimmed.to_string()))
    }
}

pub fn current_branch_ref() -> Result<String> {
    let head_content = fs::read_to_string(".git/HEAD")
        .context("Failed to read .git/HEAD — is this an rgit repository?")?;

    head_content
        .trim()
        .strip_prefix("ref: ")
        .map(|s| s.to_string())
        .context("HEAD is not a branch ref (currently in detached HEAD state)")
}

pub fn resolve_head_commit() -> Result<Option<String>> {
    match resolve_head()? {
        HeadState::Detached(hash) => Ok(Some(hash)),
        HeadState::Branch(branch) => {
            let ref_path = format!("refs/heads/{}", branch);
            read_ref(&ref_path)
        }
    }
}

pub fn set_head_detached(hash: &str) -> Result<()> {
    fs::write(".git/HEAD", format!("{}\n", hash))
        .context("Failed to write detached HEAD")?;
    Ok(())
}

pub fn read_ref(ref_name: &str) -> Result<Option<String>> {
    let path = format!(".git/{}", ref_name);

    if !Path::new(&path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read ref: {}", ref_name))?;
    Ok(Some(content.trim().to_string()))
}

pub fn write_ref(ref_name: &str, hash: &str) -> Result<()> {
    let path = format!(".git/{}", ref_name);

    if let Some(parent) = Path::new(&path).parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, format!("{}\n", hash))
        .with_context(|| format!("Failed to write ref: {}", ref_name))?;

    Ok(())
}

pub fn list_branches() -> Result<Vec<String>> {
    let heads_dir = ".git/refs/heads";

    if !Path::new(heads_dir).exists() {
        return Ok(Vec::new());
    }

    let mut branches = Vec::new();
    collect_branches(Path::new(heads_dir), "", &mut branches)?;
    branches.sort();
    Ok(branches)
}

fn collect_branches(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };

        if entry.metadata()?.is_dir() {
            collect_branches(&entry.path(), &full_name, out)?;
        } else {
            out.push(full_name);
        }
    }
    Ok(())
}

pub fn create_branch(name: &str, commit_hash: &str) -> Result<()> {
    if !is_valid_branch_name(name) {
        anyhow::bail!("fatal: '{}' is not a valid branch name", name);
    }

    let ref_name = format!("refs/heads/{}", name);
    let path = format!(".git/{}", ref_name);

    if Path::new(&path).exists() {
        anyhow::bail!("fatal: a branch named '{}' already exists", name);
    }

    let mut ancestor = Path::new(&path).parent();
    while let Some(p) = ancestor {
        if p == Path::new(".git/refs/heads") {
            break;
        }
        if p.is_file() {
            let conflict = p
                .strip_prefix(".git/refs/heads/")
                .unwrap_or(p)
                .display()
                .to_string();
            anyhow::bail!(
                "fatal: cannot lock ref 'refs/heads/{}': '{}' exists; \
                 cannot create '{}' inside it",
                name, conflict, name
            );
        }
        ancestor = p.parent();
    }

    write_ref(&ref_name, commit_hash)?;
    Ok(())
}


pub fn set_head(branch_name: &str) -> Result<()> {
    fs::write(".git/HEAD", format!("ref: refs/heads/{}\n", branch_name))
        .context("Failed to update HEAD")?;
    Ok(())
}

pub fn delete_branch(name: &str) -> Result<()> {
    let ref_path = format!("refs/heads/{}", name);
    let fs_path = format!(".git/{}", ref_path);

    if !Path::new(&fs_path).exists() {
        anyhow::bail!("error: branch '{}' not found.", name);
    }

    fs::remove_file(&fs_path)
        .with_context(|| format!("Failed to delete branch '{}'", name))?;

    let mut parent = Path::new(&fs_path).parent();
    while let Some(p) = parent {
        if p == Path::new(".git/refs/heads") || p == Path::new(".git/refs") {
            break;
        }
        let is_empty = fs::read_dir(p)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(p);
        } else {
            break;
        }
        parent = p.parent();
    }

    Ok(())
}

pub fn rename_branch(old: &str, new: &str) -> Result<()> {
    if !is_valid_branch_name(new) {
        anyhow::bail!("fatal: '{}' is not a valid branch name", new);
    }

    let old_ref = format!("refs/heads/{}", old);
    let new_ref = format!("refs/heads/{}", new);
    let old_path = format!(".git/{}", old_ref);
    let new_path = format!(".git/{}", new_ref);

    if !Path::new(&old_path).exists() {
        anyhow::bail!("error: branch '{}' not found.", old);
    }
    if Path::new(&new_path).exists() {
        anyhow::bail!("fatal: a branch named '{}' already exists.", new);
    }

    if let Some(parent) = Path::new(&new_path).parent() {
        fs::create_dir_all(parent)?;
    }

    fs::rename(&old_path, &new_path).with_context(|| format!("Failed to rename branch '{}' to '{}'", old, new))?;

    let head_content = fs::read_to_string(".git/HEAD").unwrap_or_default();
    let current_ref = format!("ref: {}", old_ref);
    if head_content.trim() == current_ref {
        set_head(new)?;
    }

    Ok(())
}

pub fn is_valid_branch_name(name: &str) -> bool {
    if name.is_empty() || name.ends_with(".lock") || name.ends_with('.') || name == "@" || name.starts_with('.') {
        return false;
    }

    let mut prev_char = '\0';
    for c in name.chars() {
        if c.is_ascii_control() || (c as u32) < 32 || c == '\u{7f}' {
            return false;
        }
        if c == ' ' || c == '~' || c == '^' || c == ':' || c == '?' || c == '*' 
            || c == '[' || c == '\\' {
            return false;
        }
        if c == '.' && prev_char == '.' {
            return false;
        }
        if c == '/' && prev_char == '/' {
            return false;
        }
        if c == '{' && prev_char == '@' {
            return false;
        }
        prev_char = c;
    }

    if name.starts_with('/') || name.ends_with('/') {
        return false;
    }

    for component in name.split('/') {
        if component.is_empty() {
            return false;
        }
        if component.starts_with('.') {
            return false;
        }
        if component.ends_with(".lock") {
            return false;
        }
    }

    true
}

