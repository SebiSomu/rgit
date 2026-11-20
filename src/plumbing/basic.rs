pub mod cli;

use std::fs;
use std::path::PathBuf;
use anyhow::Context;
use sha1::{Digest, Sha1};
use crate::core::object_store::{read_object, write_object, write_tree_from_index_prefix};
use crate::core::index::read_index;

pub fn init() -> anyhow::Result<()> {
    fs::create_dir_all(".git/objects")?;
    fs::create_dir_all(".git/refs/heads")?;
    fs::write(".git/HEAD", "ref: refs/heads/main\n")?;
    println!("Initialized empty git repository in .git/");
    Ok(())
}

pub fn hash_object(write: bool, file: PathBuf) -> anyhow::Result<()> {
    let mut content = fs::read(&file)?;
    if crate::core::repo::exists() {
        content = crate::core::eol::clean(&crate::core::repo::to_git_path_string(&file), content);
    }
    if write {
        println!("{}", write_object("blob", &content)?);
    } else {
        let header = format!("blob {}\0", content.len());
        let mut store = header.into_bytes();
        store.extend_from_slice(&content);
        let mut hasher = Sha1::new();
        hasher.update(&store);
        println!("{}", hex::encode(hasher.finalize()));
    }
    Ok(())
}

pub fn write_tree() -> anyhow::Result<()> {
    let entries = read_index().unwrap_or_default();
    // A tree can't hold a conflict: every unmerged entry is reported and nothing is written.
    if entries.iter().any(|e| e.stage != 0) {
        // git lists the first ten, then "...".
        for (n, entry) in entries.iter().filter(|e| e.stage != 0).enumerate() {
            if n == 10 {
                eprintln!("...");
                break;
            }
            eprintln!("{}: unmerged ({})", entry.path, hex::encode(entry.hash));
        }
        eprintln!("fatal: rgit-write-tree: error building trees");
        crate::ui::exit(128);
    }
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    println!("{}", tree_hash);
    Ok(())
}

/// Which part of an object `rgit cat-file` prints: `-p` its contents, `-t` its type, `-s` its
/// size, `-e` nothing (only the exit status says whether it exists).
// Used by: src/plumbing/basic.rs, src/plumbing/basic/cli.rs
// Used by command/helper: `cat_file` and `dispatch_cat_file`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatFileMode {
    None,
    Pretty,
    Type,
    Size,
    Exists,
}

/// Resolves the object name given to `cat-file` / `ls-tree`: any revision (`HEAD`, `main~1`,
/// `HEAD:path`, an abbreviated hash). A full 40-digit hash that is not in the store is kept as
/// it is, so the caller can tell "missing object" from "not an object name".
// Used by `cat_file` and `ls_tree`.
fn resolve_object_name(name: &str) -> Option<String> {
    if let Ok(hash) = crate::core::rev_parse::resolve_rev(name) {
        return Some(hash);
    }
    if name.len() == 40 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(name.to_ascii_lowercase());
    }
    None
}

/// Splits a tree object into its `(mode, name, hash)` entries.
// Used by `cat_file_resolved` and `ls_tree_walk`.
fn tree_entries(content: &[u8]) -> anyhow::Result<Vec<(String, String, String)>> {
    let mut entries = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..].iter().position(|&b| b == b' ').context("Missing mode separator")? + pos;
        let mode = String::from_utf8_lossy(&content[pos..space_pos]).to_string();

        let null_pos = content[space_pos..].iter().position(|&b| b == 0).context("Missing terminator")? + space_pos;
        let name = String::from_utf8_lossy(&content[space_pos + 1..null_pos]).to_string();

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        let sha_hex = hex::encode(content.get(hash_start..hash_end).context("Truncated tree entry")?);
        entries.push((mode, name, sha_hex));
        pos = hash_end;
    }
    Ok(entries)
}

/// The object type shown for a tree entry's mode.
// Used by `cat_file_resolved` and `ls_tree_walk`.
fn entry_type(mode: &str) -> &'static str {
    if mode == "40000" { "tree" } else if mode == "160000" { "commit" } else { "blob" }
}

/// `rgit cat-file (-p | -t | -s | -e) <object>`
///
/// `<object>` is any revision (`HEAD`, `HEAD:path`, an abbreviated hash). `-p` prints the
/// contents (a tree as `ls-tree` lines), `-t` the type, `-s` the size in bytes, and `-e` only
/// sets the exit status: 0 if the object exists, 1 if a well-formed name is missing.
pub fn cat_file(mode: CatFileMode, object_hash: String) -> anyhow::Result<()> {
    let Some(resolved) = resolve_object_name(&object_hash) else {
        if matches!(mode, CatFileMode::Pretty | CatFileMode::None) {
            // As before: the read reports the missing object.
            return cat_file_resolved(mode, object_hash);
        }
        crate::ui::die_fatal(format!("Not a valid object name {}", object_hash));
    };
    cat_file_resolved(mode, resolved)
}

/// `cat_file` once the object name is a hash.
// Used by `cat_file`.
fn cat_file_resolved(mode: CatFileMode, object_hash: String) -> anyhow::Result<()> {
    // An object with a `refs/replace/` entry shows its replacement, as in git.
    let object_hash = crate::core::object_store::replacement_object(&object_hash).unwrap_or(object_hash);
    let object = crate::core::object_store::read_object(&object_hash);
    let (object_type, content) = match (mode, object) {
        (CatFileMode::Exists, Ok(_)) => return Ok(()),
        (CatFileMode::Exists, Err(_)) => crate::ui::exit(1),
        (CatFileMode::Type | CatFileMode::Size, Err(_)) => crate::ui::die_fatal("rgit cat-file: could not get object info"),
        (_, result) => result.context("Failed to read object file")?,
    };

    match mode {
        CatFileMode::Type => println!("{}", object_type),
        CatFileMode::Size => println!("{}", content.len()),
        CatFileMode::Pretty if object_type == "tree" => {
            for (mode, name, hash) in tree_entries(&content)? {
                println!("{:0>6} {} {}\t{}", mode, entry_type(&mode), hash, name);
            }
        }
        CatFileMode::Pretty => {
            use std::io::Write;
            std::io::stdout().write_all(&content)?;
        }
        CatFileMode::None | CatFileMode::Exists => {}
    }
    Ok(())
}

/// `rgit ls-tree [--name-only] [-r] <tree-ish> [<path>...]`
///
/// Lists a tree's entries. `<tree-ish>` is any revision that peels to a tree (a commit, a tag,
/// `HEAD:dir`). `-r` descends into subtrees and lists only their files, with full paths.
/// `<path>` arguments restrict the listing as in git: `dir` shows the entry `dir` itself,
/// `dir/` shows what is inside it.
pub fn ls_tree(name_only: bool, recursive: bool, tree_hash: String, paths: Vec<String>) -> anyhow::Result<()> {
    let resolved = resolve_object_name(&tree_hash)
        .and_then(|hash| crate::core::rev_parse::peel_to_type(hash, "tree").ok())
        .unwrap_or_else(|| tree_hash.clone());
    let (object_type, content) = read_object(&resolved)?;
    if object_type != "tree" {
        anyhow::bail!("Not a tree object: {}", tree_hash);
    }
    let mut out = String::new();
    ls_tree_walk(&content, "", name_only, recursive, &paths, &mut out)?;
    print!("{}", out);
    Ok(())
}

/// Whether the entry at `path` is selected by the `ls-tree` path arguments.
// Used by `ls_tree_walk`.
fn ls_tree_selected(path: &str, paths: &[String]) -> bool {
    paths.is_empty()
        || paths.iter().any(|f| {
            let dir = f.trim_end_matches('/');
            (path == dir && !f.ends_with('/')) || path.starts_with(&format!("{}/", dir))
        })
}

/// Lists one tree level for `ls_tree`, descending into a subtree for `-r` or when a path
/// argument points inside it.
// Used by `ls_tree`.
fn ls_tree_walk(content: &[u8], prefix: &str, name_only: bool, recursive: bool, paths: &[String], out: &mut String) -> anyhow::Result<()> {
    for (mode, name, hash) in tree_entries(content)? {
        let path = format!("{}{}", prefix, name);
        let selected = ls_tree_selected(&path, paths);
        let inside_filter = paths.iter().any(|f| f.starts_with(&format!("{}/", path)));
        if mode == "40000" && ((recursive && selected) || inside_filter) {
            let (_, sub) = read_object(&hash)?;
            ls_tree_walk(&sub, &format!("{}/", path), name_only, recursive, paths, out)?;
            continue;
        }
        if !selected {
            continue;
        }
        if name_only {
            out.push_str(&format!("{}\n", path));
        } else {
            out.push_str(&format!("{:0>6} {} {}\t{}\n", mode, entry_type(&mode), hash, path));
        }
    }
    Ok(())
}
