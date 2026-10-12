use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};
use anyhow::Context;
use flate2::Compression;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::helpers::commit::resolve_commit_from_source;
use crate::helpers::github::format_unix_timestamp_iso8601;
use crate::helpers::objects::{flatten_tree, read_object, tree_hash_of_commit};
use crate::objects::ArchiveFormat;

/// Returns the slice of supported archive format names.
// Used by the `archive` command.
pub fn list_archive_formats() -> &'static [&'static str] {
    &["tar", "tgz", "tar.gz", "zip"]
}

/// Resolves a tree-ish reference (commit, branch, tag, HEAD, or tree hash)
/// to its tree object hash and the commit timestamp if available.
// Used by the `archive` command.
pub fn resolve_tree_ish(tree_ish: &str) -> anyhow::Result<(String, u64)> {
    // If it is directly a tree object hash:
    if let Ok((ref obj_type, _)) = read_object(tree_ish) {
        if obj_type == "tree" {
            return Ok((tree_ish.to_string(), 0));
        }
    }

    // Otherwise, resolve commit and look up its tree and timestamp:
    let commit_hash = resolve_commit_from_source(tree_ish)
        .with_context(|| format!("fatal: Not a valid object name: {}", tree_ish))?;
    let tree_hash = tree_hash_of_commit(&commit_hash)?;

    let (_, commit_content) = read_object(&commit_hash)?;
    let commit_text = String::from_utf8_lossy(&commit_content);
    let timestamp = commit_text
        .lines()
        .find(|line| line.starts_with("committer ") || line.starts_with("author "))
        .and_then(|line| {
            let mut parts = line.split_whitespace().rev();
            let _tz = parts.next()?;
            let ts_str = parts.next()?;
            ts_str.parse::<u64>().ok()
        })
        .unwrap_or(0);

    Ok((tree_hash, timestamp))
}

/// Checks if a relative entry path matches the provided path filter prefixes.
// Used by `create_archive`.
pub fn path_matches_filter(entry_path: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    for filter in filters {
        let clean = filter.trim_matches('/');
        if clean.is_empty() {
            return true;
        }
        if entry_path == clean || entry_path.starts_with(&format!("{}/", clean)) {
            return true;
        }
    }
    false
}

/// Collects intermediate and prefix directory paths in hierarchical order.
// Used by `write_tar_archive` and `write_zip_archive`.
pub fn collect_directory_paths(
    file_paths: impl IntoIterator<Item = impl AsRef<str>>,
    prefix: Option<&str>,
) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();

    if let Some(pfx) = prefix {
        let trimmed = pfx.trim_end_matches('/');
        if !trimmed.is_empty() {
            let mut current = String::new();
            for part in trimmed.split('/') {
                if !part.is_empty() {
                    if !current.is_empty() {
                        current.push('/');
                    }
                    current.push_str(part);
                    dirs.insert(format!("{}/", current));
                }
            }
        }
    }

    for path in file_paths {
        let full = match prefix {
            Some(p) => format!("{}{}", p, path.as_ref()),
            None => path.as_ref().to_string(),
        };

        if let Some(last_slash) = full.rfind('/') {
            let dir_part = &full[..last_slash];
            let mut current = String::new();
            for part in dir_part.split('/') {
                if !part.is_empty() {
                    if !current.is_empty() {
                        current.push('/');
                    }
                    current.push_str(part);
                    dirs.insert(format!("{}/", current));
                }
            }
        }
    }

    dirs
}

/// Writes tree entries as a tar or compressed tar.gz archive to the destination writer.
// Used by `create_archive`.
pub fn write_tar_archive<W: Write>(tree_entries: &BTreeMap<String, ([u8; 20], u32)>,timestamp: u64,prefix: Option<&str>,gzip: bool,writer: W) -> anyhow::Result<()> {
    if gzip {
        let gz = GzEncoder::new(writer, Compression::default());
        let mut builder = Builder::new(gz);
        write_tar_entries(&mut builder, tree_entries, timestamp, prefix)?;
        let gz = builder.into_inner()?;
        gz.finish()?;
    } else {
        let mut builder = Builder::new(writer);
        write_tar_entries(&mut builder, tree_entries, timestamp, prefix)?;
        builder.finish()?;
    }

    Ok(())
}

/// Appends directory headers and file blobs into an active tar builder.
// Used by `write_tar_archive`.
fn write_tar_entries<W: Write>(builder: &mut Builder<W>,tree_entries: &BTreeMap<String, ([u8; 20], u32)>,timestamp: u64,prefix: Option<&str>,) -> anyhow::Result<()> {
    let dirs = collect_directory_paths(tree_entries.keys(), prefix);
    for dir_path in dirs {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o40755);
        header.set_mtime(timestamp);
        header.set_cksum();
        builder.append_data(&mut header, &dir_path, std::io::empty())?;
    }

    for (rel_path, (hash, mode)) in tree_entries {
        let (obj_type, content) = read_object(&hex::encode(hash))?;
        if obj_type != "blob" {
            continue;
        }

        let archive_path = match prefix {
            Some(p) => format!("{}{}", p, rel_path),
            None => rel_path.clone(),
        };

        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_size(content.len() as u64);
        header.set_mode(*mode);
        header.set_mtime(timestamp);
        header.set_cksum();
        builder.append_data(&mut header, &archive_path, &content[..])?;
    }

    Ok(())
}

/// Converts a Unix timestamp in seconds to a zip DateTime instance.
// Used by `write_zip_archive`.
pub fn unix_timestamp_to_zip_datetime(timestamp: u64) -> zip::DateTime {
    if timestamp < 315532800 {
        return zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap_or_default();
    }

    let iso = format_unix_timestamp_iso8601(timestamp);
    let year: u16 = iso[0..4].parse().unwrap_or(1980).max(1980);
    let month: u8 = iso[5..7].parse().unwrap_or(1).clamp(1, 12);
    let day: u8 = iso[8..10].parse().unwrap_or(1).clamp(1, 31);
    let hour: u8 = iso[11..13].parse().unwrap_or(0).clamp(0, 23);
    let minute: u8 = iso[14..16].parse().unwrap_or(0).clamp(0, 59);
    let second: u8 = iso[17..19].parse().unwrap_or(0).clamp(0, 59);

    zip::DateTime::from_date_and_time(year, month, day, hour, minute, second).unwrap_or_default()
}

/// Writes tree entries as a ZIP archive to the destination writer.
// Used by `create_archive`.
pub fn write_zip_archive<W: Write>(tree_entries: &BTreeMap<String, ([u8; 20], u32)>,timestamp: u64,prefix: Option<&str>,mut writer: W,) -> anyhow::Result<()> {
    let mut buffer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buffer);
    let zip_dt = unix_timestamp_to_zip_datetime(timestamp);

    let dir_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755)
        .last_modified_time(zip_dt);

    let dirs = collect_directory_paths(tree_entries.keys(), prefix);
    for dir_path in dirs {
        zip.add_directory(&dir_path, dir_options)?;
    }

    for (rel_path, (hash, mode)) in tree_entries {
        let (obj_type, content) = read_object(&hex::encode(hash))?;
        if obj_type != "blob" {
            continue;
        }

        let archive_path = match prefix {
            Some(p) => format!("{}{}", p, rel_path),
            None => rel_path.clone(),
        };

        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(*mode)
            .last_modified_time(zip_dt);

        zip.start_file(&archive_path, file_options)?;
        zip.write_all(&content)?;
    }

    zip.finish()?;
    let data = buffer.into_inner();
    writer.write_all(&data)?;
    Ok(())
}

/// Creates an archive in the specified format from tree entries and writes it to `writer`.
// Used by the `archive` command.
pub fn create_archive<W: Write>(tree_hash: &str,timestamp: u64,format: ArchiveFormat,prefix: Option<&str>,paths: &[String],writer: W,) -> anyhow::Result<()> {
    let mut all_entries = BTreeMap::new();
    flatten_tree(tree_hash, "", &mut all_entries)?;

    let filtered_entries: BTreeMap<String, ([u8; 20], u32)> = all_entries
        .into_iter()
        .filter(|(p, _)| path_matches_filter(p, paths))
        .collect();

    match format {
        ArchiveFormat::Tar => write_tar_archive(&filtered_entries, timestamp, prefix, false, writer),
        ArchiveFormat::TarGz => write_tar_archive(&filtered_entries, timestamp, prefix, true, writer),
        ArchiveFormat::Zip => write_zip_archive(&filtered_entries, timestamp, prefix, writer),
    }
}