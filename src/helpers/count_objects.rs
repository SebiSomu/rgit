use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::objects::ObjectCountReport;

/// Checks if a string consists entirely of ASCII hexadecimal digits.
// Used by: `collect_object_counts`
fn is_hex_str(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Recursively computes the total size in bytes of all files in a directory.
// Used by: `collect_object_counts`
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            } else if path.is_dir() {
                total += dir_size(&path);
            }
        }
    }
    total
}

/// Parses a Git `.idx` file (version 1 or 2) and extracts the total object count and all object hashes.
// Used by: `collect_object_counts`
pub fn read_pack_idx(path: &Path) -> Result<(usize, Vec<String>)> {
    let data = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if data.len() < 1024 {
        anyhow::bail!("Index file is too small to be valid: {}", path.display());
    }

    if data.starts_with(b"\xfftOc") {
        // Pack index version 2+
        if data.len() < 1032 {
            anyhow::bail!("Invalid pack index header: {}", path.display());
        }
        let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
        if version != 2 {
            anyhow::bail!("Unsupported pack index version {}: {}", version, path.display());
        }
        let num_objects = u32::from_be_bytes(data[1028..1032].try_into().unwrap()) as usize;
        let hashes_end = 1032 + num_objects * 20;
        if data.len() < hashes_end {
            anyhow::bail!("Truncated pack index file: {}", path.display());
        }
        let mut hashes = Vec::with_capacity(num_objects);
        for i in 0..num_objects {
            let start = 1032 + i * 20;
            let end = start + 20;
            hashes.push(hex::encode(&data[start..end]));
        }
        Ok((num_objects, hashes))
    } else {
        // Pack index version 1
        let num_objects = u32::from_be_bytes(data[1020..1024].try_into().unwrap()) as usize;
        let entries_end = 1024 + num_objects * 24;
        if data.len() < entries_end {
            anyhow::bail!("Truncated pack index v1 file: {}", path.display());
        }
        let mut hashes = Vec::with_capacity(num_objects);
        for i in 0..num_objects {
            let start = 1024 + i * 24 + 4;
            let end = start + 20;
            hashes.push(hex::encode(&data[start..end]));
        }
        Ok((num_objects, hashes))
    }
}

/// Formats a byte count according to Git's human-readable size convention (bytes, KiB, MiB, GiB).
// Used by: src/commands/count_objects.rs
pub fn format_human_readable_size(bytes: u64) -> String {
    if bytes > 1 << 30 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes > 1 << 20 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes > 1 << 10 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Scans the repository's `.git/objects` directory, gathering counts and sizes for loose
/// objects, packfiles, indexed objects, and garbage files.
// Used by: the `count-objects` command
pub fn collect_object_counts(verbose: bool) -> Result<ObjectCountReport> {
    if !crate::repo::exists() {
        anyhow::bail!("fatal: not a git repository (or any of the parent directories): .git");
    }

    let objects_dir = &crate::repo::path(".git/objects");
    if !objects_dir.is_dir() {
        return Ok(ObjectCountReport::default());
    }

    let mut report = ObjectCountReport::default();
    let mut packed_hashes = HashSet::new();

    // 1. Process pack files in .git/objects/pack
    let pack_dir = objects_dir.join("pack");
    if pack_dir.is_dir() {
        let mut packs_by_stem: HashMap<String, (PathBuf, u64)> = HashMap::new();
        let mut idxs_by_stem: HashMap<String, (PathBuf, u64)> = HashMap::new();
        let mut other_packdir_files: Vec<(PathBuf, u64, String)> = Vec::new();

        if let Ok(entries) = fs::read_dir(&pack_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if meta.is_dir() {
                    if verbose {
                        eprintln!("warning: garbage found: {}", path.to_string_lossy().replace('\\', "/"));
                    }
                    report.garbage += 1;
                    report.size_garbage_bytes += dir_size(&path);
                    continue;
                }

                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

                match ext.as_str() {
                    "pack" => {
                        packs_by_stem.insert(stem, (path, meta.len()));
                    }
                    "idx" => {
                        idxs_by_stem.insert(stem, (path, meta.len()));
                    }
                    _ => {
                        other_packdir_files.push((path, meta.len(), ext));
                    }
                }
            }
        }

        // Check packs and matching idxs
        for (stem, (path, size)) in &packs_by_stem {
            if idxs_by_stem.contains_key(stem) {
                report.packs += 1;
                report.size_pack_bytes += size;
            } else {
                if verbose {
                    eprintln!("warning: no corresponding .idx: {}", path.to_string_lossy().replace('\\', "/"));
                }
                report.garbage += 1;
                report.size_garbage_bytes += size;
            }
        }

        // Check idxs and matching packs
        for (stem, (path, size)) in &idxs_by_stem {
            if packs_by_stem.contains_key(stem) {
                report.size_pack_bytes += size;
                match read_pack_idx(path) {
                    Ok((count, hashes)) => {
                        report.in_pack += count;
                        packed_hashes.extend(hashes);
                    }
                    Err(_) => {
                        if verbose {
                            eprintln!("warning: garbage found: {}", path.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
            } else {
                if verbose {
                    eprintln!("warning: no corresponding .pack: {}", path.to_string_lossy().replace('\\', "/"));
                }
                report.garbage += 1;
                report.size_garbage_bytes += size;
            }
        }

        // Check other files in pack dir (like .rev, .bitmap, or stray files)
        for (path, size, ext) in other_packdir_files {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if (ext == "rev" || ext == "bitmap" || ext == "mtimes") && packs_by_stem.contains_key(stem) {
                report.size_pack_bytes += size;
            } else {
                if verbose {
                    eprintln!("warning: garbage found: {}", path.to_string_lossy().replace('\\', "/"));
                }
                report.garbage += 1;
                report.size_garbage_bytes += size;
            }
        }
    }

    // 2. Process loose object directories (.git/objects/??) and stray entries
    let obj_entries = fs::read_dir(objects_dir)
        .with_context(|| format!("Failed to read {}", objects_dir.display()))?;

    for entry in obj_entries {
        let entry = entry?;
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata()?;

        if meta.is_dir() {
            if name == "info" || name == "pack" {
                continue;
            }

            if name.len() == 2 && is_hex_str(&name) {
                let sub_entries = fs::read_dir(&entry_path)
                    .with_context(|| format!("Failed to read {}", entry_path.display()))?;

                for sub in sub_entries {
                    let sub = sub?;
                    let sub_path = sub.path();
                    let sub_name = sub.file_name().to_string_lossy().to_string();
                    let sub_meta = sub.metadata()?;

                    if sub_meta.is_file() && sub_name.len() == 38 && is_hex_str(&sub_name) {
                        report.count += 1;
                        report.size_bytes += sub_meta.len();
                        let hash = format!("{}{}", name, sub_name).to_ascii_lowercase();
                        if packed_hashes.contains(&hash) {
                            report.prune_packable += 1;
                        }
                    } else {
                        if verbose {
                            eprintln!("warning: garbage found: {}", sub_path.to_string_lossy().replace('\\', "/"));
                        }
                        report.garbage += 1;
                        report.size_garbage_bytes += if sub_meta.is_dir() {
                            dir_size(&sub_path)
                        } else {
                            sub_meta.len()
                        };
                    }
                }
            } else {
                // Non-standard directory in .git/objects
                if verbose {
                    eprintln!("warning: garbage found: {}", entry_path.to_string_lossy().replace('\\', "/"));
                }
                report.garbage += 1;
                report.size_garbage_bytes += dir_size(&entry_path);
            }
        } else {
            // Stray file directly under .git/objects
            if verbose {
                eprintln!("warning: garbage found: {}", entry_path.to_string_lossy().replace('\\', "/"));
            }
            report.garbage += 1;
            report.size_garbage_bytes += meta.len();
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_human_readable_size() {
        assert_eq!(format_human_readable_size(0), "0 bytes");
        assert_eq!(format_human_readable_size(31), "31 bytes");
        assert_eq!(format_human_readable_size(1024), "1024 bytes");
        assert_eq!(format_human_readable_size(1025), "1.00 KiB");
        assert_eq!(format_human_readable_size(1366), "1.33 KiB");
        assert_eq!(format_human_readable_size(1048576), "1024.00 KiB");
        assert_eq!(format_human_readable_size(1048577), "1.00 MiB");
        assert_eq!(format_human_readable_size(1073741824), "1024.00 MiB");
        assert_eq!(format_human_readable_size(1073741825), "1.00 GiB");
    }

    #[test]
    fn test_read_pack_idx_v2() {
        // Construct a minimal valid pack index v2 header with 1 object
        let mut data = Vec::new();
        // Magic
        data.extend_from_slice(b"\xfftOc");
        // Version 2
        data.extend_from_slice(&2u32.to_be_bytes());
        // 256 fanout entries (4 bytes each)
        for _ in 0..255 {
            data.extend_from_slice(&0u32.to_be_bytes());
        }
        // Last fanout entry: 1 object
        data.extend_from_slice(&1u32.to_be_bytes());
        // 1 SHA-1 hash (20 bytes of 0xab)
        data.extend_from_slice(&[0xab; 20]);

        let temp_dir = std::env::temp_dir().join(format!("rgit_test_idx_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let idx_path = temp_dir.join("test.idx");
        fs::write(&idx_path, &data).unwrap();

        let (num_objects, hashes) = read_pack_idx(&idx_path).unwrap();
        assert_eq!(num_objects, 1);
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0], "abababababababababababababababababababab");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
