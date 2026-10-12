use std::io::Write;
use std::path::PathBuf;
use anyhow::Context;

use crate::helpers::archive::{create_archive, list_archive_formats, resolve_tree_ish};
use crate::objects::ArchiveFormat;

/// Entry point for `rgit archive`.
///
/// Exports files from a named tree, commit, tag, or reference into a tar,
/// tar.gz, or zip archive.
pub fn archive(format: Option<String>, list: bool, prefix: Option<String>, output: Option<PathBuf>, tree_ish: Option<String>, paths: Vec<String>,) -> anyhow::Result<()> {
    if list {
        for fmt in list_archive_formats() {
            println!("{}", fmt);
        }
        return Ok(());
    }

    let tree_ish_str = tree_ish.ok_or_else(|| {
        anyhow::anyhow!("fatal: You must specify a tree-ish to export.")
    })?;

    let archive_format = match format {
        Some(fmt) => ArchiveFormat::parse(&fmt)?,
        None => match &output {
            Some(path) => ArchiveFormat::from_path(path).unwrap_or(ArchiveFormat::Tar),
            None => ArchiveFormat::Tar,
        },
    };

    let (tree_hash, timestamp) = resolve_tree_ish(&tree_ish_str)?;
    let prefix_ref = prefix.as_deref();

    if let Some(out_path) = output {
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = std::fs::File::create(&out_path)
            .with_context(|| format!("fatal: could not create output archive '{}'", out_path.display()))?;
        let mut buf_writer = std::io::BufWriter::new(file);
        create_archive(&tree_hash, timestamp, archive_format, prefix_ref, &paths, &mut buf_writer)?;
        buf_writer.flush()?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        create_archive(&tree_hash, timestamp, archive_format, prefix_ref, &paths, &mut handle)?;
        handle.flush()?;
    }

    Ok(())
}
