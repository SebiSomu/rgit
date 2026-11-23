//! Helpers of `rgit split`: taking a commit apart into pieces (hunks and whole-file changes),
//! showing them, building the tree of a subset of them, the prompts, and the commits on top of
//! the one being split.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};

use anyhow::Result;

use crate::core::commit::parse_commit_object;
use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit, write_object};
use crate::merge::split::types::{PathMap, Piece, SplitSource};
use crate::workspace::absorb::engine::{apply_hunks, hunks_of, label, lines_of};
use crate::workspace::absorb::types::StagedHunk;

/// Context lines shown around a hunk.
const CONTEXT: usize = 2;

/// Takes `commit` apart against `parent`: one piece per hunk of every text file changed in
/// both, one whole-file piece per added, deleted or binary file and per mode change.
// Used by the `split` command.
pub fn pieces_of(commit: &str, parent: &str) -> Result<SplitSource> {
    let (mut parent_tree, mut commit_tree) = (PathMap::new(), PathMap::new());
    flatten_tree(&tree_hash_of_commit(parent)?, "", &mut parent_tree)?;
    flatten_tree(&tree_hash_of_commit(commit)?, "", &mut commit_tree)?;
    let mut source = SplitSource { parent_tree, commit_tree, parent_lines: Default::default(), pieces: Vec::new() };
    let paths: BTreeSet<String> = source.parent_tree.keys().chain(source.commit_tree.keys()).cloned().collect();
    for path in paths {
        let (old, new) = (source.parent_tree.get(&path).copied(), source.commit_tree.get(&path).copied());
        if old == new {
            continue;
        }
        let whole = |what: &str| Piece::File { path: path.clone(), new, what: what.to_string() };
        let (Some((old_hash, old_mode)), Some((new_hash, new_mode))) = (old, new) else {
            source.pieces.push(whole(if old.is_none() { "new file" } else { "deleted file" }));
            continue;
        };
        if old_mode != new_mode {
            source.pieces.push(whole(&format!("mode change {:o} -> {:o}", old_mode, new_mode)));
            continue;
        }
        let (old_blob, new_blob) = (read_object(&hex::encode(old_hash))?.1, read_object(&hex::encode(new_hash))?.1);
        if old_blob.contains(&0) || new_blob.contains(&0) || old_mode == 0o160000 {
            source.pieces.push(whole(if old_mode == 0o160000 { "submodule" } else { "binary file" }));
            continue;
        }
        let (old_lines, new_lines) = (lines_of(&old_blob), lines_of(&new_blob));
        source.pieces.extend(hunks_of(&path, &old_lines, &new_lines).into_iter().map(Piece::Hunk));
        source.parent_lines.insert(path, old_lines);
    }
    Ok(source)
}

/// The tree of the parent with `chosen` applied (hunks of one file together).
// Used by the `split` command for every new commit.
pub fn tree_with(source: &SplitSource, chosen: &[&Piece]) -> Result<String> {
    let mut tree = source.parent_tree.clone();
    for (path, old_lines) in &source.parent_lines {
        let hunks: Vec<&StagedHunk> = chosen.iter().filter_map(|p| match p {
            Piece::Hunk(h) if h.path == *path => Some(h),
            _ => None,
        }).collect();
        if hunks.is_empty() {
            continue;
        }
        let blob = write_object("blob", apply_hunks(old_lines, &hunks).as_bytes())?;
        let mut raw = [0u8; 20];
        hex::decode_to_slice(&blob, &mut raw)?;
        tree.insert(path.clone(), (raw, source.commit_tree[path].1));
    }
    for piece in chosen {
        if let Piece::File { path, new, .. } = piece {
            match new {
                Some(entry) => tree.insert(path.clone(), *entry),
                None => tree.remove(path),
            };
        }
    }
    crate::workspace::state_files::build_tree_from_map(&tree)
}

/// Prints a piece: `path:lines` and the removed/added lines with a little context, or the
/// whole-file change.
// Used by the `split` command's picker.
pub fn show_piece(source: &SplitSource, piece: &Piece) {
    match piece {
        Piece::File { path, what, .. } => println!("{}  ({})", path, what),
        Piece::Hunk(h) => {
            println!("{}", label(h));
            let old = &source.parent_lines[&h.path];
            let line = |prefix: char, text: &str| println!("{}{}", prefix, text.strip_suffix('\n').unwrap_or(text));
            for text in &old[h.old_start.saturating_sub(CONTEXT)..h.old_start] {
                line(' ', text);
            }
            for text in &old[h.old_start..h.old_start + h.old_len] {
                line('-', text);
            }
            for text in &h.new_lines {
                line('+', text);
            }
            let end = h.old_start + h.old_len;
            for text in &old[end..(end + CONTEXT).min(old.len())] {
                line(' ', text);
            }
        }
    }
}

/// Prints `prompt` and reads one trimmed answer line; `None` at the end of input.
// Used by the `split` command's picker and message prompts.
pub fn ask(prompt: &str) -> Result<Option<String>> {
    print!("{}", prompt);
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer)? == 0 {
        println!();
        return Ok(None);
    }
    Ok(Some(answer.trim().to_string()))
}

/// The commits from `commit` (exclusive) up to `head` (inclusive), oldest first, following
/// first parents. Fails on a merge in between (it would have to be replayed).
// Used by the `split` command to rewrite the commits on top.
pub fn commits_above(head: &str, commit: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut current = head.to_string();
    while current != commit {
        let parsed = parse_commit_object(&current)?;
        if parsed.parents.len() != 1 {
            anyhow::bail!("{} is a merge commit", &current[..7]);
        }
        out.push(current);
        current = parsed.parents[0].clone();
    }
    out.reverse();
    Ok(out)
}
