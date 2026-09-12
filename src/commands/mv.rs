use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Context;
use crate::helpers::normalize_path;
use crate::index::{build_entry, read_index, write_index};

pub fn mv(source: PathBuf, destination: PathBuf, force: bool) -> anyhow::Result<()> {
    let src_rel = normalize_path(&source);

    let mut index_entries = read_index().unwrap_or_default();
    let (src_hash, src_mode) = index_entries
        .iter()
        .find(|e| e.path == src_rel)
        .map(|e| (e.hash, e.mode))
        .ok_or_else(|| anyhow::anyhow!("fatal: not under version control: '{}'", src_rel))?;

    if !Path::new(&src_rel).exists() {
        anyhow::bail!("fatal: bad source, source={} is missing from the working tree", src_rel);
    }

    let dest_rel = if Path::new(&destination).is_dir() {
        let file_name = Path::new(&src_rel)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("fatal: '{}' has no file name component", src_rel))?;
        normalize_path(&Path::new(&destination).join(file_name))
    } else {
        normalize_path(&destination)
    };

    if src_rel == dest_rel {
        anyhow::bail!("fatal: '{}' and '{}' are the same file", src_rel, dest_rel);
    }

    let dest_already_tracked = index_entries.iter().any(|e| e.path == dest_rel);
    let dest_exists_on_disk = Path::new(&dest_rel).exists();

    if (dest_already_tracked || dest_exists_on_disk) && !force {
        anyhow::bail!(
            "fatal: destination '{}' already exists; use -f to overwrite",
            dest_rel
        );
    }

    if let Some(parent) = Path::new(&dest_rel).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory for '{}'", dest_rel))?;
        }
    }

    fs::rename(&src_rel, &dest_rel)
        .with_context(|| format!("failed to rename '{}' to '{}'", src_rel, dest_rel))?;
    
    let mut parent = Path::new(&src_rel).parent();
    while let Some(p) = parent {
        if p == Path::new("") || p == Path::new(".") {
            break;
        }
        if p.exists() {
            if fs::read_dir(p)?.next().is_none() {
                fs::remove_dir(p)?;
            } else {
                break;
            }
        }
        parent = p.parent();
    }

    index_entries.retain(|e| e.path != dest_rel && e.path != src_rel);

    let metadata = fs::metadata(&dest_rel)
        .with_context(|| format!("failed to stat '{}' after move", dest_rel))?;
    let mut new_entry = build_entry(&dest_rel, src_hash, &metadata);
    new_entry.mode = src_mode;
    index_entries.push(new_entry);

    write_index(&mut index_entries)?;

    println!("renamed '{}' -> '{}'", src_rel, dest_rel);

    Ok(())
}