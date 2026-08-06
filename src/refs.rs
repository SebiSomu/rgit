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
        .context("HEAD is not a branch ref (detached HEAD isn't supported yet)")
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
    let ref_name = format!("refs/heads/{}", name);
    let path = format!(".git/{}", ref_name);

    if Path::new(&path).exists() {
        anyhow::bail!("fatal: a branch named '{}' already exists", name);
    }

    write_ref(&ref_name, commit_hash)?;
    Ok(())
}

pub fn set_head(branch_name: &str) -> Result<()> {
    fs::write(".git/HEAD", format!("ref: refs/heads/{}\n", branch_name))
        .context("Failed to update HEAD")?;
    Ok(())
}
